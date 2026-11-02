//! Command handlers: repositories and workspace lifecycle.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context};
use bantu_git as git;
use bantu_protocol::{Command, LogStream, Repo, Workspace, WorkspaceState};
use globset::{Glob, GlobSet, GlobSetBuilder};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;

use crate::store::{now, Store};
use crate::{names, settings, Config};

const PORT_RANGE_START: u16 = 55000;
const PORTS_PER_WORKSPACE: u16 = 10;
/// Copied into new worktrees when gitignored, in addition to `.worktreeinclude`.
const DEFAULT_INCLUDES: &[&str] = &[".env", ".env.*", "**/.env", "**/.env.*"];

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

type CmdResult<T> = Result<T, CommandError>;

pub struct Daemon {
    pub store: Store,
    pub config: Config,
    /// Serializes name/port allocation so concurrent creates never collide.
    alloc_lock: Mutex<()>,
    pub(crate) running: crate::sessions::Running,
    pub(crate) terminals: crate::terminals::Terminals,
    pub(crate) permissions: crate::permissions::Permissions,
}

impl Daemon {
    pub fn new(store: Store, config: Config) -> Self {
        Self {
            store,
            config,
            alloc_lock: Mutex::new(()),
            running: Default::default(),
            terminals: Default::default(),
            permissions: Default::default(),
        }
    }

    pub async fn execute(self: &Arc<Self>, cmd: Command) -> CmdResult<serde_json::Value> {
        let value = match cmd {
            Command::RepoAdd { path } => serde_json::to_value(self.add_repo(&path).await?),
            Command::RepoRemove { repo_id } => {
                self.remove_repo(&repo_id).await?;
                Ok(serde_json::Value::Null)
            }
            Command::WorkspaceCreate {
                repo_id,
                name,
                base_ref,
            } => serde_json::to_value(self.create_workspace(&repo_id, name, base_ref).await?),
            Command::WorkspaceArchive { workspace_id } => {
                serde_json::to_value(self.archive_workspace(&workspace_id).await?)
            }
            Command::SessionCreate {
                workspace_id,
                model,
                permission_mode,
            } => serde_json::to_value(
                self.create_session(&workspace_id, model, permission_mode)
                    .await?,
            ),
            Command::SessionSend { session_id, text } => {
                serde_json::to_value(self.send_message(&session_id, text).await?)
            }
            Command::SessionCancel { session_id } => {
                serde_json::to_value(self.cancel_session(&session_id).await?)
            }
            Command::SessionRewind { session_id, turn } => {
                serde_json::to_value(self.rewind_session(&session_id, turn).await?)
            }
            Command::PermissionRespond {
                request_id,
                allow,
                message,
            } => {
                self.respond_permission(&request_id, allow, message).await?;
                Ok(serde_json::Value::Null)
            }
            Command::CommentAdd {
                workspace_id,
                path,
                line,
                body,
            } => serde_json::to_value(self.add_comment(&workspace_id, path, line, body).await?),
            Command::CommentDelete { comment_id } => {
                self.delete_comment(&comment_id).await?;
                Ok(serde_json::Value::Null)
            }
            Command::CommentsSend {
                workspace_id,
                session_id,
            } => serde_json::to_value(self.send_comments(&workspace_id, &session_id).await?),
            Command::TerminalOpen {
                workspace_id,
                cols,
                rows,
            } => serde_json::to_value(self.open_terminal(&workspace_id, cols, rows).await?),
            Command::TerminalClose { terminal_id } => {
                self.close_terminal(&terminal_id).await?;
                Ok(serde_json::Value::Null)
            }
            Command::WorkspaceRun { workspace_id } => {
                serde_json::to_value(self.run_workspace(&workspace_id).await?)
            }
            Command::WorkspaceCreatePr {
                workspace_id,
                session_id,
            } => serde_json::to_value(self.create_pr(&workspace_id, &session_id).await?),
            Command::SessionUpdate {
                session_id,
                title,
                model,
                permission_mode,
            } => serde_json::to_value(
                self.update_session(&session_id, title, model, permission_mode)
                    .await?,
            ),
        };
        value.map_err(|e| CommandError::Internal(e.into()))
    }

    /// Workspaces left mid-operation by a crash or restart can't resume, so
    /// surface them as failed rather than spinning forever.
    pub async fn recover_interrupted(&self) -> anyhow::Result<()> {
        for mut ws in self.store.workspaces().await? {
            if matches!(
                ws.state,
                WorkspaceState::Creating | WorkspaceState::Archiving
            ) {
                ws.state = WorkspaceState::Failed;
                ws.error = Some("interrupted by a daemon restart".into());
                self.store.upsert_workspace(&ws).await?;
            }
        }
        Ok(())
    }

    // ---- repos ------------------------------------------------------------

    async fn add_repo(&self, path: &str) -> CmdResult<Repo> {
        let path = expand_home(path);
        if !path.is_dir() {
            return Err(CommandError::BadRequest(format!(
                "{} is not a directory",
                path.display()
            )));
        }
        let root = git::repo_root(&path).await.map_err(|_| {
            CommandError::BadRequest(format!("{} is not inside a git repository", path.display()))
        })?;
        let root_str = root.to_string_lossy().to_string();
        if let Some(existing) = self.store.repo_by_root(&root_str).await? {
            return Ok(existing);
        }
        let repo = Repo {
            id: new_id(),
            name: root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "repo".into()),
            default_branch: git::default_branch(&root)
                .await
                .map_err(anyhow::Error::from)?,
            remote_url: git::remote_url(&root, "origin")
                .await
                .map_err(anyhow::Error::from)?,
            root_path: root_str,
            created_at: now(),
        };
        self.store.insert_repo(&repo).await?;
        Ok(repo)
    }

    async fn remove_repo(&self, repo_id: &str) -> CmdResult<()> {
        self.require_repo(repo_id).await?;
        let active = self
            .store
            .workspaces()
            .await?
            .into_iter()
            .filter(|w| w.repo_id == repo_id && w.state != WorkspaceState::Archived)
            .count();
        if active > 0 {
            return Err(CommandError::Conflict(format!(
                "archive the repository's {active} workspace(s) first"
            )));
        }
        self.store.delete_repo(repo_id).await?;
        Ok(())
    }

    async fn require_repo(&self, repo_id: &str) -> CmdResult<Repo> {
        self.store
            .repo(repo_id)
            .await?
            .ok_or_else(|| CommandError::NotFound(format!("no repository {repo_id}")))
    }

    // ---- workspaces -------------------------------------------------------

    async fn create_workspace(
        self: &Arc<Self>,
        repo_id: &str,
        name: Option<String>,
        base_ref: Option<String>,
    ) -> CmdResult<Workspace> {
        let repo = self.require_repo(repo_id).await?;
        let root = PathBuf::from(&repo.root_path);
        if let Some(n) = &name {
            if !names::is_valid(n) {
                return Err(CommandError::BadRequest(
                    "workspace names use lowercase letters, digits and dashes".into(),
                ));
            }
        }

        let _alloc = self.alloc_lock.lock().await;
        let all = self.store.workspaces().await?;
        let in_repo: Vec<String> = all
            .iter()
            .filter(|w| w.repo_id == repo.id)
            .map(|w| w.name.clone())
            .collect();
        let name = match name {
            Some(n) if in_repo.contains(&n) => {
                return Err(CommandError::Conflict(format!(
                    "workspace {n} already exists"
                )))
            }
            Some(n) => n,
            None => self.unused_name(&root, &repo, in_repo).await?,
        };
        let branch = format!("{}{name}", self.config.branch_prefix);
        if git::ref_exists(&root, &format!("refs/heads/{branch}"))
            .await
            .map_err(anyhow::Error::from)?
        {
            return Err(CommandError::Conflict(format!(
                "branch {branch} already exists"
            )));
        }
        let path = self.config.workspaces_dir.join(&repo.name).join(&name);
        if path.exists() {
            return Err(CommandError::Conflict(format!(
                "{} already exists",
                path.display()
            )));
        }
        let used_ports: Vec<u16> = all
            .iter()
            .filter(|w| w.state != WorkspaceState::Archived)
            .map(|w| w.port_base)
            .collect();
        let port_base = (0..)
            .map(|i| PORT_RANGE_START + i * PORTS_PER_WORKSPACE)
            .find(|p| !used_ports.contains(p))
            .expect("port range exhausted");

        let base_ref = match base_ref {
            Some(b) => b,
            None if repo.remote_url.is_some() => format!("origin/{}", repo.default_branch),
            None => repo.default_branch.clone(),
        };
        let ws = Workspace {
            id: new_id(),
            repo_id: repo.id.clone(),
            name,
            branch,
            base_ref,
            path: path.to_string_lossy().to_string(),
            port_base,
            state: WorkspaceState::Creating,
            error: None,
            archive_commit: None,
            created_at: now(),
        };
        self.store.upsert_workspace(&ws).await?;
        drop(_alloc);

        tokio::spawn(self.clone().provision(ws.clone(), repo));
        Ok(ws)
    }

    async fn unused_name(
        &self,
        root: &Path,
        repo: &Repo,
        mut taken: Vec<String>,
    ) -> CmdResult<String> {
        loop {
            let candidate = names::pick(&taken);
            let branch_taken = git::ref_exists(
                root,
                &format!("refs/heads/{}{candidate}", self.config.branch_prefix),
            )
            .await
            .map_err(anyhow::Error::from)?;
            let dir_taken = self
                .config
                .workspaces_dir
                .join(&repo.name)
                .join(&candidate)
                .exists();
            if !branch_taken && !dir_taken {
                return Ok(candidate);
            }
            taken.push(candidate);
        }
    }

    async fn provision(self: Arc<Self>, mut ws: Workspace, repo: Repo) {
        let result = self.provision_inner(&mut ws, &repo).await;
        match result {
            Ok(()) => {
                ws.state = WorkspaceState::Ready;
                self.store
                    .log(&ws.id, LogStream::System, "Workspace ready")
                    .await;
            }
            Err(e) => {
                ws.state = WorkspaceState::Failed;
                ws.error = Some(format!("{e:#}"));
                self.store
                    .log(&ws.id, LogStream::System, format!("Failed: {e:#}"))
                    .await;
            }
        }
        if let Err(e) = self.store.upsert_workspace(&ws).await {
            tracing::error!("failed to record workspace {}: {e:#}", ws.id);
            return;
        }
        // Every ready workspace starts with one chat, like opening a new tab.
        if ws.state == WorkspaceState::Ready {
            if let Err(e) = self.create_session(&ws.id, None, None).await {
                tracing::error!("failed to create session for {}: {e:#}", ws.id);
            }
        }
    }

    async fn provision_inner(&self, ws: &mut Workspace, repo: &Repo) -> anyhow::Result<()> {
        let root = PathBuf::from(&repo.root_path);
        let path = PathBuf::from(&ws.path);
        let sys = |line: String| self.store.log(&ws.id, LogStream::System, line);

        if repo.remote_url.is_some() {
            sys("Fetching origin…".into()).await;
            if let Err(e) = git::fetch(&root, "origin").await {
                sys(format!("Fetch failed, continuing with local refs: {e}")).await;
            }
        }
        if !git::ref_exists(&root, &ws.base_ref).await? {
            let fallback = repo.default_branch.clone();
            if fallback == ws.base_ref || !git::ref_exists(&root, &fallback).await? {
                bail!(
                    "{} does not exist; does the repository have any commits?",
                    ws.base_ref
                );
            }
            sys(format!("{} not found, using {fallback}", ws.base_ref)).await;
            ws.base_ref = fallback;
            self.store.upsert_workspace(ws).await?;
        }

        sys(format!(
            "Creating worktree {} from {}",
            ws.branch, ws.base_ref
        ))
        .await;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        git::worktree_add(&root, &path, &ws.branch, &ws.base_ref).await?;

        let copied = copy_included_files(&root, &path).await?;
        if !copied.is_empty() {
            sys(format!("Copied {}", copied.join(", "))).await;
        }

        let settings = settings::load(&[&path, &root]).map_err(|e| anyhow!(e))?;
        if let Some(script) = settings.scripts.setup.as_deref() {
            sys(format!("Running setup script: {script}")).await;
            self.run_script(ws, repo, script)
                .await
                .context("setup script")?;
        }
        Ok(())
    }

    async fn archive_workspace(self: &Arc<Self>, id: &str) -> CmdResult<Workspace> {
        let mut ws = self
            .store
            .workspace(id)
            .await?
            .ok_or_else(|| CommandError::NotFound(format!("no workspace {id}")))?;
        match ws.state {
            WorkspaceState::Archived => return Ok(ws),
            WorkspaceState::Creating | WorkspaceState::Archiving => {
                return Err(CommandError::Conflict(format!(
                    "workspace is {}; try again when it settles",
                    ws.state.as_str()
                )))
            }
            WorkspaceState::Ready | WorkspaceState::Failed => {}
        }
        let repo = self.require_repo(&ws.repo_id).await?;
        self.cancel_workspace_sessions(&ws.id).await?;
        self.close_workspace_terminals(&ws.id).await?;
        ws.state = WorkspaceState::Archiving;
        ws.error = None;
        self.store.upsert_workspace(&ws).await?;

        let this = self.clone();
        let mut bg = ws.clone();
        tokio::spawn(async move {
            match this.archive_inner(&mut bg, &repo).await {
                Ok(()) => {
                    bg.state = WorkspaceState::Archived;
                    this.store
                        .log(&bg.id, LogStream::System, "Workspace archived")
                        .await;
                }
                Err(e) => {
                    bg.state = WorkspaceState::Failed;
                    bg.error = Some(format!("archive failed: {e:#}"));
                    this.store
                        .log(&bg.id, LogStream::System, format!("Archive failed: {e:#}"))
                        .await;
                }
            }
            if let Err(e) = this.store.upsert_workspace(&bg).await {
                tracing::error!("failed to record workspace {}: {e:#}", bg.id);
            }
        });
        Ok(ws)
    }

    async fn archive_inner(&self, ws: &mut Workspace, repo: &Repo) -> anyhow::Result<()> {
        let root = PathBuf::from(&repo.root_path);
        let path = PathBuf::from(&ws.path);
        let sys = |line: String| self.store.log(&ws.id, LogStream::System, line);

        if path.is_dir() {
            let settings = settings::load(&[&path, &root]).map_err(|e| anyhow!(e))?;
            if let Some(script) = settings.scripts.archive.as_deref() {
                sys(format!("Running archive script: {script}")).await;
                if let Err(e) = self.run_script(ws, repo, script).await {
                    // A broken archive script shouldn't trap the workspace forever.
                    sys(format!("Archive script failed, continuing: {e:#}")).await;
                }
            }
            if let Some(sha) = git::stash_create(&path).await? {
                let r = format!("refs/bantu/archive/{}", ws.id);
                git::update_ref(&root, &r, &sha).await?;
                sys(format!("Saved uncommitted changes to {r}")).await;
                ws.archive_commit = Some(sha);
            }
        }
        sys("Removing worktree".into()).await;
        git::worktree_remove(&root, &path).await?;
        // Checkpoint refs only matter while the worktree exists.
        for s in self.store.sessions().await? {
            if s.workspace_id == ws.id {
                let prefix = format!("refs/bantu/cp/{}/", s.id);
                let refs =
                    git::git(&root, &["for-each-ref", "--format=%(refname)", &prefix]).await?;
                for r in refs.lines().filter(|r| !r.is_empty()) {
                    git::git(&root, &["update-ref", "-d", r]).await?;
                }
            }
        }
        Ok(())
    }

    /// Runs `script` in the user's login shell inside the workspace, streaming
    /// output into the workspace log.
    async fn run_script(&self, ws: &Workspace, repo: &Repo, script: &str) -> anyhow::Result<()> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut child = tokio::process::Command::new(&shell)
            .arg("-lc")
            .arg(script)
            .current_dir(&ws.path)
            .env("BANTU_PORT", ws.port_base.to_string())
            .env("BANTU_WORKSPACE_ID", &ws.id)
            .env("BANTU_WORKSPACE_NAME", &ws.name)
            .env("BANTU_WORKSPACE_PATH", &ws.path)
            .env("BANTU_ROOT_PATH", &repo.root_path)
            .env("BANTU_BASE_REF", &ws.base_ref)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("starting {shell}"))?;

        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let pump = |reader, stream: LogStream| async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                self.store.log(&ws.id, stream.clone(), line).await;
            }
        };
        let (status, _, _) = tokio::join!(
            child.wait(),
            pump(
                Box::new(stdout) as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
                LogStream::Stdout
            ),
            pump(
                Box::new(stderr) as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
                LogStream::Stderr
            ),
        );
        let status = status?;
        if !status.success() {
            return Err(anyhow!("exited with {status}"));
        }
        Ok(())
    }
}

/// Copies gitignored files that match `.worktreeinclude` (or the default `.env*`
/// patterns) from the main checkout into the new worktree. Returns what was copied.
async fn copy_included_files(root: &Path, worktree: &Path) -> anyhow::Result<Vec<String>> {
    let mut builder = GlobSetBuilder::new();
    for pattern in DEFAULT_INCLUDES {
        builder.add(Glob::new(pattern)?);
    }
    if let Ok(text) = tokio::fs::read_to_string(root.join(".worktreeinclude")).await {
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.trim_start_matches('/').trim_end_matches('/');
            builder.add(Glob::new(line).with_context(|| format!(".worktreeinclude: {line}"))?);
        }
    }
    let globs: GlobSet = builder.build()?;

    let mut copied = Vec::new();
    for rel in git::ignored_paths(root).await? {
        let trimmed = rel.trim_end_matches('/');
        if !globs.is_match(trimmed) {
            continue;
        }
        let (from, to) = (root.join(trimmed), worktree.join(trimmed));
        tokio::task::spawn_blocking(move || copy_recursive(&from, &to)).await??;
        copied.push(trimmed.to_string());
    }
    Ok(copied)
}

fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else if meta.file_type().is_symlink() {
        let target = std::fs::read_link(from)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, to)?;
    } else {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to)?;
    }
    Ok(())
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ if path == "~" => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default(),
        _ => PathBuf::from(path),
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
