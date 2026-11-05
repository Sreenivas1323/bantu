//! Command handlers: repositories and workspace lifecycle.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context};
use bantu_git as git;
use bantu_protocol::{Command, LogStream, Repo, Workspace, WorkspaceSource, WorkspaceState};
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
    pub(crate) inbox: crate::inbox::Inbox,
    pub(crate) radar: crate::radar::Radar,
    pub(crate) activity: crate::activity::Tracker,
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
            inbox: Default::default(),
            radar: Default::default(),
            activity: Default::default(),
        }
    }

    pub async fn execute(self: &Arc<Self>, cmd: Command) -> CmdResult<serde_json::Value> {
        self.activity.touch();
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
                from,
            } => serde_json::to_value(
                self.create_workspace(&repo_id, name, base_ref, from)
                    .await?,
            ),
            Command::WorkspaceArchive { workspace_id } => {
                serde_json::to_value(self.archive_workspace(&workspace_id).await?)
            }
            Command::SessionCreate {
                workspace_id,
                agent,
                model,
                permission_mode,
            } => serde_json::to_value(
                self.create_session(&workspace_id, agent, model, permission_mode)
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
            Command::PreferencesUpdate { claude_user_setup } => {
                let mut p = self.store.preferences().await?;
                if let Some(v) = claude_user_setup {
                    p.claude_user_setup = v;
                }
                self.store.set_preferences(&p).await?;
                serde_json::to_value(p)
            }
            Command::AgentLogin { agent, method } => {
                serde_json::to_value(self.agent_login(agent, method).await?)
            }
            Command::PermissionRespond {
                request_id,
                allow,
                message,
            } => {
                self.respond_permission(&request_id, allow, message).await?;
                Ok(serde_json::Value::Null)
            }
            Command::InboxRespond { item_id, answer } => {
                self.respond_inbox(&item_id, answer).await?;
                Ok(serde_json::Value::Null)
            }
            Command::InboxDismiss { item_id } => {
                self.dismiss_inbox(&item_id).await?;
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
                command,
            } => serde_json::to_value(
                self.open_terminal(&workspace_id, cols, rows, command)
                    .await?,
            ),
            Command::SessionOpenTerminal {
                session_id,
                cols,
                rows,
            } => serde_json::to_value(self.open_session_terminal(&session_id, cols, rows).await?),
            Command::TerminalClose { terminal_id } => {
                self.close_terminal(&terminal_id).await?;
                Ok(serde_json::Value::Null)
            }
            Command::WorkspaceRun { workspace_id } => {
                serde_json::to_value(self.run_workspace(&workspace_id).await?)
            }
            Command::WorkspaceFixChecks {
                workspace_id,
                session_id,
            } => serde_json::to_value(self.fix_checks(&workspace_id, &session_id).await?),
            Command::WorkspaceResolveConflicts {
                workspace_id,
                session_id,
            } => serde_json::to_value(self.resolve_conflicts(&workspace_id, &session_id).await?),
            Command::WorkspaceRenameBranch {
                workspace_id,
                session_id,
            } => serde_json::to_value(self.rename_branch(&workspace_id, &session_id).await?),
            Command::WorkspaceAddressComments {
                workspace_id,
                session_id,
            } => serde_json::to_value(self.address_pr_comments(&workspace_id, &session_id).await?),
            Command::WorkspaceReview {
                workspace_id,
                agent,
            } => serde_json::to_value(self.review_workspace(&workspace_id, agent).await?),
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
        let path = match clone_source(path) {
            Some(url) => self.clone_repo(&url).await?,
            None => expand_home(path),
        };
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

    /// Clones into `<repos_dir>/<name>`, or reuses an earlier clone there.
    async fn clone_repo(&self, url: &str) -> CmdResult<PathBuf> {
        let name = repo_name_from_url(url).ok_or_else(|| {
            CommandError::BadRequest(format!("can't tell the repo name from {url}"))
        })?;
        let parent = self.config.repos_dir.clone();
        let dest = parent.join(&name);
        if dest.join(".git").exists() {
            return Ok(dest);
        }
        tokio::fs::create_dir_all(&parent)
            .await
            .map_err(anyhow::Error::from)?;
        let out = tokio::process::Command::new("git")
            .args(["clone", "--quiet", url])
            .arg(&dest)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .await
            .map_err(anyhow::Error::from)?;
        if !out.status.success() {
            return Err(CommandError::BadRequest(format!(
                "git clone failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(dest)
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
        from: Option<WorkspaceSource>,
    ) -> CmdResult<Workspace> {
        let repo = self.require_repo(repo_id).await?;
        let root = PathBuf::from(&repo.root_path);
        let plan = self.plan_source(&repo, from).await?;
        let name = name.or(plan.suggested_name.clone());
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
            Some(n) if in_repo.contains(&n) && plan.suggested_name.as_ref() == Some(&n) => {
                // e.g. a second workspace for the same PR: fall back to a city.
                self.unused_name(&root, &repo, in_repo).await?
            }
            Some(n) if in_repo.contains(&n) => {
                return Err(CommandError::Conflict(format!(
                    "workspace {n} already exists"
                )))
            }
            Some(n) => n,
            None => self.unused_name(&root, &repo, in_repo).await?,
        };
        let branch = match &plan.branch {
            Some(b) => b.clone(),
            None => format!("{}{name}", self.config.branch_prefix),
        };
        let exists = git::ref_exists(&root, &format!("refs/heads/{branch}"))
            .await
            .map_err(anyhow::Error::from)?;
        if exists && matches!(plan.checkout, Checkout::NewBranch) {
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

        let base_ref = match base_ref.or(plan.base_ref.clone()) {
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

        tokio::spawn(self.clone().provision(ws.clone(), repo, plan));
        Ok(ws)
    }

    /// Works out the branch, how to check it out, and any first task for a
    /// workspace created from a branch, PR or issue.
    async fn plan_source(
        &self,
        repo: &Repo,
        from: Option<WorkspaceSource>,
    ) -> CmdResult<SourcePlan> {
        let root = Path::new(&repo.root_path);
        let Some(from) = from else {
            return Ok(SourcePlan::default());
        };
        match from {
            WorkspaceSource::Branch { branch } => {
                let branch = branch.trim().to_string();
                if !git::valid_branch_name(root, &branch)
                    .await
                    .map_err(anyhow::Error::from)?
                {
                    return Err(CommandError::BadRequest(format!(
                        "{branch:?} isn't a valid branch name"
                    )));
                }
                if let Some(at) = git::branch_checked_out_at(root, &branch)
                    .await
                    .map_err(anyhow::Error::from)?
                {
                    return Err(CommandError::Conflict(format!(
                        "{branch} is already checked out at {at}"
                    )));
                }
                Ok(SourcePlan {
                    branch: Some(branch),
                    checkout: Checkout::Existing,
                    ..Default::default()
                })
            }
            WorkspaceSource::PullRequest { number } => {
                let pr = gh_json(
                    root,
                    &[
                        "pr",
                        "view",
                        &number.to_string(),
                        "--json",
                        "headRefName,baseRefName,isCrossRepository",
                    ],
                )
                .await
                .ok_or_else(|| {
                    CommandError::BadRequest(format!("couldn't load PR #{number} with gh"))
                })?;
                let cross = pr["isCrossRepository"].as_bool().unwrap_or(false);
                let head = pr["headRefName"].as_str().unwrap_or_default().to_string();
                let base = pr["baseRefName"]
                    .as_str()
                    .unwrap_or(&repo.default_branch)
                    .to_string();
                Ok(SourcePlan {
                    // Fork PRs get a local name; their head lives in another repo.
                    branch: Some(if cross { format!("pr-{number}") } else { head }),
                    checkout: if cross {
                        Checkout::PullHead(number)
                    } else {
                        Checkout::Existing
                    },
                    base_ref: Some(format!("origin/{base}")),
                    suggested_name: Some(format!("pr-{number}")),
                    first_message: None,
                })
            }
            WorkspaceSource::Issue { number } => {
                let issue = gh_json(
                    root,
                    &[
                        "issue",
                        "view",
                        &number.to_string(),
                        "--json",
                        "title,body,url",
                    ],
                )
                .await
                .ok_or_else(|| {
                    CommandError::BadRequest(format!("couldn't load issue #{number} with gh"))
                })?;
                let title = issue["title"].as_str().unwrap_or_default();
                let slug = crate::sessions::branch_slug(title)
                    .unwrap_or_else(|| format!("issue-{number}"));
                let base = format!("{}{number}-{slug}", self.config.branch_prefix);
                let mut branch = base.clone();
                let mut n = 2;
                while git::ref_exists(root, &format!("refs/heads/{branch}"))
                    .await
                    .map_err(anyhow::Error::from)?
                {
                    branch = format!("{base}-{n}");
                    n += 1;
                }
                Ok(SourcePlan {
                    branch: Some(branch),
                    first_message: Some(issue_message(number, &issue)),
                    ..Default::default()
                })
            }
        }
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

    async fn provision(self: Arc<Self>, mut ws: Workspace, repo: Repo, plan: SourcePlan) {
        let result = self.provision_inner(&mut ws, &repo, &plan.checkout).await;
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
        self.rescan_conflicts(&ws.repo_id);
        // Every ready workspace starts with one chat, like opening a new tab.
        if ws.state == WorkspaceState::Ready {
            match self.create_session(&ws.id, None, None, None).await {
                Ok(session) => {
                    if let Some(task) = plan.first_message {
                        if let Err(e) = self.send_message(&session.id, task).await {
                            tracing::error!("failed to start the first task for {}: {e:#}", ws.id);
                        }
                    }
                }
                Err(e) => tracing::error!("failed to create session for {}: {e:#}", ws.id),
            }
        }
    }

    async fn provision_inner(
        &self,
        ws: &mut Workspace,
        repo: &Repo,
        checkout: &Checkout,
    ) -> anyhow::Result<()> {
        let root = PathBuf::from(&repo.root_path);
        let path = PathBuf::from(&ws.path);
        let sys = |line: String| self.store.log(&ws.id, LogStream::System, line);

        if repo.remote_url.is_some() {
            sys("Fetching origin…".into()).await;
            if let Err(e) = git::fetch(&root, "origin").await {
                sys(format!("Fetch failed, continuing with local refs: {e}")).await;
                if let Some(hint) = fetch_hint(&e.to_string()) {
                    sys(hint.into()).await;
                }
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

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        match checkout {
            Checkout::NewBranch => {
                sys(format!(
                    "Creating worktree {} from {}",
                    ws.branch, ws.base_ref
                ))
                .await;
                git::worktree_add(&root, &path, &ws.branch, &ws.base_ref).await?;
            }
            Checkout::Existing => {
                let local = git::ref_exists(&root, &format!("refs/heads/{}", ws.branch)).await?;
                let remote = format!("origin/{}", ws.branch);
                if local {
                    sys(format!("Checking out {}", ws.branch)).await;
                    git::worktree_add_existing(&root, &path, &ws.branch).await?;
                } else if git::ref_exists(&root, &format!("refs/remotes/{remote}")).await? {
                    sys(format!("Checking out {} from {remote}", ws.branch)).await;
                    git::worktree_add_tracking(&root, &path, &ws.branch, &remote).await?;
                } else {
                    bail!("branch {} doesn't exist locally or on origin", ws.branch);
                }
            }
            Checkout::PullHead(number) => {
                sys(format!("Fetching pull request #{number}")).await;
                let refspec = format!("+pull/{number}/head:refs/heads/{}", ws.branch);
                git::git(&root, &["fetch", "origin", &refspec]).await?;
                git::worktree_add_existing(&root, &path, &ws.branch).await?;
            }
        }

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
            this.rescan_conflicts(&bg.repo_id);
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

/// Plain-language fixes for the fetch failures people actually hit.
fn fetch_hint(err: &str) -> Option<&'static str> {
    if err.contains("git-credential") && err.contains("No such file") {
        Some(
            "Hint: git is configured to use a credential helper that no longer exists \
             (often an old `gh` path). Run `gh auth setup-git` to fix it.",
        )
    } else if err.contains("could not read Username") || err.contains("Authentication failed") {
        Some("Hint: git has no GitHub credentials. Run `gh auth login`, then `gh auth setup-git`.")
    } else if err.contains("Permission denied (publickey)") {
        Some("Hint: your SSH key isn't accepted. Check `ssh -T git@github.com`.")
    } else {
        None
    }
}

/// A clone URL if `input` names a remote repo rather than a local path:
/// `https://…`, `git@host:owner/repo`, `ssh://…`, or GitHub shorthand `owner/repo`.
fn clone_source(input: &str) -> Option<String> {
    let input = input.trim();
    if ["https://", "ssh://", "git@", "file://"]
        .iter()
        .any(|p| input.starts_with(p))
    {
        return Some(input.to_string());
    }
    let shorthand = input.split('/').count() == 2
        && !input.starts_with(['.', '/', '~'])
        && input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'));
    (shorthand && !Path::new(input).exists()).then(|| format!("https://github.com/{input}.git"))
}

fn repo_name_from_url(url: &str) -> Option<String> {
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty() && name != "." && name != "..").then(|| name.to_string())
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

/// How a new workspace gets its branch.
#[derive(Debug, Clone, Default)]
pub enum Checkout {
    /// `git worktree add -b <branch> <path> <base>`.
    #[default]
    NewBranch,
    /// The branch exists locally or on origin.
    Existing,
    /// A fork's PR head, fetched from `pull/<n>/head`.
    PullHead(u32),
}

#[derive(Debug, Clone, Default)]
pub struct SourcePlan {
    branch: Option<String>,
    checkout: Checkout,
    base_ref: Option<String>,
    suggested_name: Option<String>,
    /// Sent to the agent once the workspace is ready.
    first_message: Option<String>,
}

async fn gh_json(dir: &Path, args: &[&str]) -> Option<serde_json::Value> {
    let out = tokio::process::Command::new("gh")
        .args(args)
        .current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1")
        .output()
        .await
        .ok()?;
    out.status.success().then_some(())?;
    serde_json::from_slice(&out.stdout).ok()
}

fn issue_message(number: u32, issue: &serde_json::Value) -> String {
    let title = issue["title"].as_str().unwrap_or_default();
    let url = issue["url"].as_str().unwrap_or_default();
    let body = issue["body"].as_str().unwrap_or_default().trim();
    let mut msg = format!("Work on GitHub issue #{number}: {title}\n{url}\n");
    if !body.is_empty() {
        msg.push_str(&format!("\n{body}\n"));
    }
    msg.push_str(
        "\nStart by understanding the problem and the relevant code, then make the change \
         with tests where it makes sense. Mention `Fixes #",
    );
    msg.push_str(&format!("{number}` in your summary."));
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_fetch_failures() {
        let stale = "git fetch --prune origin failed: /usr/local/bin/gh auth git-credential get: \
                     /usr/local/bin/gh: No such file or directory";
        assert!(fetch_hint(stale).unwrap().contains("gh auth setup-git"));
        assert!(fetch_hint("fatal: could not read Username for 'https://github.com'").is_some());
        assert_eq!(fetch_hint("network unreachable"), None);
    }

    #[test]
    fn recognizes_clone_sources() {
        assert_eq!(
            clone_source("Sreenivas1323/bantu").as_deref(),
            Some("https://github.com/Sreenivas1323/bantu.git")
        );
        assert_eq!(
            clone_source("git@github.com:o/r.git").as_deref(),
            Some("git@github.com:o/r.git")
        );
        assert_eq!(
            clone_source("https://gitlab.com/o/r").as_deref(),
            Some("https://gitlab.com/o/r")
        );
        assert_eq!(clone_source("~/code/bantu"), None);
        assert_eq!(clone_source("/Users/me/code"), None);
        assert_eq!(clone_source("code/app/x"), None);
        assert_eq!(
            repo_name_from_url("https://github.com/o/my-app.git").as_deref(),
            Some("my-app")
        );
        assert_eq!(
            repo_name_from_url("git@github.com:o/r").as_deref(),
            Some("r")
        );
    }

    #[test]
    fn issue_task_message() {
        let issue = serde_json::json!({"title": "Login loops forever", "url": "https://github.com/o/r/issues/12",
            "body": "Steps: log in twice."});
        let msg = issue_message(12, &issue);
        assert!(msg.starts_with(
            "Work on GitHub issue #12: Login loops forever\nhttps://github.com/o/r/issues/12\n"
        ));
        assert!(msg.contains("Steps: log in twice."));
        assert!(msg.ends_with("Mention `Fixes #12` in your summary."));
    }
}
