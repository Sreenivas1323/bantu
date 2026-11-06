//! Moving to Bantu from Conductor or Pane without losing history.
//!
//! Their databases are opened read-only, and their worktrees are adopted where
//! they are: a workspace imported this way is `external`, so archiving it forgets
//! it and leaves the files for the tool that made them. Only active workspaces
//! come across, each chat with its most recent messages. Claude and Codex chats
//! keep their conversation ids, so they can carry on.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use bantu_git as git;
use bantu_protocol::{
    AgentKind, ChatItem, EventData, ImportPreview, ImportSource, ImportStatus, PermissionMode,
    Session, SessionStatus,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use crate::names;
use crate::ops::{new_id, CommandError, Daemon};
use crate::sessions::parse_stream_line;

/// Long-lived chats have hundreds of thousands of rows; recent ones matter most.
const MAX_MESSAGES_PER_CHAT: i64 = 1000;

type CmdResult<T> = Result<T, CommandError>;

struct SourceWorkspace {
    repo_root: String,
    path: String,
    name: String,
    /// What the branch was started from, if the source knows.
    target_branch: Option<String>,
    id: String,
}

struct SourceChat {
    id: String,
    agent: AgentKind,
    conversation_id: String,
    title: String,
    model: Option<String>,
    permission_mode: PermissionMode,
    created_at: String,
}

fn default_path(source: ImportSource) -> Option<PathBuf> {
    let home = crate::platform::home_dir()?;
    Some(match source {
        ImportSource::Conductor => {
            home.join("Library/Application Support/com.conductor.app/conductor.db")
        }
        ImportSource::Pane => home.join(".pane/sessions.db"),
    })
}

/// Never writes. Even a read-only connection to a WAL database can create empty
/// `-wal` and `-shm` files beside it, so `immutable` comes first when there's no
/// pending WAL content to miss. With pending content those files already exist,
/// and a plain read-only open sees the latest data.
async fn open(path: &Path) -> Result<SqlitePool> {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let pending_wal = std::fs::metadata(&wal).is_ok_and(|m| m.len() > 0);
    let order = if pending_wal {
        [false, true]
    } else {
        [true, false]
    };
    for immutable in order {
        let options = SqliteConnectOptions::from_str("sqlite://")?
            .filename(path)
            .read_only(true)
            .immutable(immutable)
            .create_if_missing(false);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await;
        if let Ok(pool) = pool {
            let readable: Result<i64, _> = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master")
                .fetch_one(&pool)
                .await;
            if readable.is_ok() {
                return Ok(pool);
            }
        }
    }
    bail!("couldn't open {} read-only", path.display())
}

/// Lowercase letters, digits and dashes, which is what workspace names allow.
fn clean_name(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() || !names::is_valid(&out) {
        "imported".into()
    } else {
        out
    }
}

/// Permission modes as Claude Code names them.
fn parse_mode(raw: Option<&str>) -> PermissionMode {
    match raw {
        Some("plan") => PermissionMode::Plan,
        Some("acceptEdits") => PermissionMode::AcceptEdits,
        Some("bypassPermissions") => PermissionMode::BypassPermissions,
        Some("auto") => PermissionMode::Auto,
        // Asking is the safe reading of anything else.
        _ => PermissionMode::Ask,
    }
}

async fn read_workspaces(pool: &SqlitePool, source: ImportSource) -> Result<Vec<SourceWorkspace>> {
    let sql = match source {
        ImportSource::Conductor => {
            "SELECT w.local_id AS id, r.root_path AS repo_root, w.workspace_path AS path,
                    COALESCE(w.workspace_name, w.directory_name) AS name,
                    COALESCE(w.intended_target_branch, w.initialization_parent_branch, r.default_branch) AS target
             FROM workspaces w JOIN repos r ON r.id = w.repository_id
             WHERE w.state = 'ready'
               AND w.workspace_path IS NOT NULL AND r.root_path IS NOT NULL"
        }
        ImportSource::Pane => {
            "SELECT s.id AS id, p.path AS repo_root, s.worktree_path AS path, s.worktree_name AS name,
                    COALESCE(s.base_branch, p.main_branch) AS target
             FROM sessions s JOIN projects p ON p.id = s.project_id
             WHERE COALESCE(s.archived, 0) = 0 AND COALESCE(s.is_main_repo, 0) = 0"
        }
    };
    let rows = sqlx::query(sql)
        .fetch_all(pool)
        .await
        .context("this version's database has a layout Bantu doesn't know")?;
    Ok(rows
        .iter()
        .map(|r| SourceWorkspace {
            id: r.get("id"),
            repo_root: r.get("repo_root"),
            path: r.get("path"),
            name: r.get::<Option<String>, _>("name").unwrap_or_default(),
            target_branch: r.get("target"),
        })
        .collect())
}

/// Chats that can carry on. One without a conversation id has nothing to resume.
async fn read_chats(
    pool: &SqlitePool,
    source: ImportSource,
    workspace_id: &str,
) -> Result<Vec<SourceChat>> {
    let sql = match source {
        ImportSource::Conductor => {
            "SELECT id, claude_session_id AS conversation, agent_type AS agent, title, model,
                    permission_mode AS mode, CAST(created_at AS TEXT) AS created_at
             FROM sessions
             WHERE workspace_id = ? AND COALESCE(is_hidden, 0) = 0
               AND claude_session_id IS NOT NULL"
        }
        ImportSource::Pane => {
            "SELECT id, claude_session_id AS conversation, tool_type AS agent, name AS title,
                    NULL AS model, NULL AS mode, CAST(created_at AS TEXT) AS created_at
             FROM sessions
             WHERE id = ? AND claude_session_id IS NOT NULL"
        }
    };
    let rows = sqlx::query(sql).bind(workspace_id).fetch_all(pool).await?;
    let mut chats = Vec::new();
    for r in &rows {
        let agent = match r.get::<Option<String>, _>("agent").as_deref() {
            None | Some("claude") | Some("none") => AgentKind::Claude,
            Some("codex") => AgentKind::Codex,
            // Their other agents' conversations can't be resumed from here.
            Some(_) => continue,
        };
        let title: Option<String> = r.get("title");
        chats.push(SourceChat {
            id: r.get("id"),
            agent,
            conversation_id: r.get("conversation"),
            title: title
                .filter(|t| !t.trim().is_empty() && t != "Untitled")
                .unwrap_or_else(|| "Imported chat".into()),
            model: r.get("model"),
            permission_mode: parse_mode(r.get::<Option<String>, _>("mode").as_deref()),
            created_at: r.get("created_at"),
        });
    }
    Ok(chats)
}

/// Rows as `(role, content)`, oldest first, keeping only the newest ones.
async fn read_messages(
    pool: &SqlitePool,
    source: ImportSource,
    chat_id: &str,
) -> Result<Vec<(String, String)>> {
    let sql = match source {
        ImportSource::Conductor => {
            "SELECT role, content FROM (
                SELECT rowid AS position, role, content FROM session_messages
                WHERE session_id = ? AND cancelled_at IS NULL AND content IS NOT NULL
                ORDER BY rowid DESC LIMIT ?
             ) ORDER BY position"
        }
        ImportSource::Pane => {
            "SELECT role, content FROM (
                SELECT id AS position, message_type AS role, content FROM conversation_messages
                WHERE session_id = ?
                ORDER BY id DESC LIMIT ?
             ) ORDER BY position"
        }
    };
    let rows = sqlx::query(sql)
        .bind(chat_id)
        .bind(MAX_MESSAGES_PER_CHAT)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get("role"), r.get("content")))
        .collect())
}

/// Turns stored rows into transcript items. Conductor stores assistant rows as
/// `stream-json` lines, and Pane as plain text.
pub fn to_items(rows: Vec<(String, String)>) -> Vec<(u32, ChatItem)> {
    let mut turn = 0;
    let mut items = Vec::new();
    for (role, content) in rows {
        if role == "user" {
            turn += 1;
            items.push((turn, ChatItem::User { text: content }));
            continue;
        }
        // History cut off mid-conversation can start before any user message.
        let at = turn.max(1);
        match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(value) if value.is_object() => {
                items.extend(parse_stream_line(&value).into_iter().map(|i| (at, i)));
            }
            _ if !content.trim().is_empty() => {
                items.push((at, ChatItem::Assistant { text: content }));
            }
            _ => {}
        }
    }
    items
}

impl Daemon {
    pub async fn import_scan(
        &self,
        source: ImportSource,
        path: Option<String>,
    ) -> CmdResult<ImportPreview> {
        let path = path
            .map(PathBuf::from)
            .or_else(|| default_path(source))
            .ok_or_else(|| CommandError::BadRequest("HOME isn't set".into()))?;
        let mut preview = ImportPreview {
            source,
            path: path.to_string_lossy().to_string(),
            found: path.is_file(),
            workspaces: 0,
            chats: 0,
            missing_worktrees: 0,
        };
        if !preview.found {
            return Ok(preview);
        }
        let pool = open(&path).await?;
        for ws in read_workspaces(&pool, source).await? {
            if Path::new(&ws.path).is_dir() {
                preview.workspaces += 1;
                preview.chats += read_chats(&pool, source, &ws.id).await?.len() as u32;
            } else {
                preview.missing_worktrees += 1;
            }
        }
        pool.close().await;
        Ok(preview)
    }

    /// Starts importing in the background. Progress arrives as `import.updated`.
    pub async fn import_run(
        self: &Arc<Self>,
        source: ImportSource,
        path: Option<String>,
    ) -> CmdResult<ImportPreview> {
        let preview = self.import_scan(source, path).await?;
        if !preview.found {
            return Err(CommandError::NotFound(format!(
                "nothing to import at {}",
                preview.path
            )));
        }
        let this = self.clone();
        let from = PathBuf::from(&preview.path);
        tokio::spawn(async move {
            let mut status = ImportStatus {
                source,
                running: true,
                workspaces: 0,
                chats: 0,
                items: 0,
                skipped: 0,
                error: None,
            };
            let _ = this
                .store
                .emit(EventData::ImportUpdated {
                    status: status.clone(),
                })
                .await;
            if let Err(e) = this.import_all(source, &from, &mut status).await {
                status.error = Some(format!("{e:#}"));
            }
            status.running = false;
            let _ = this.store.emit(EventData::ImportUpdated { status }).await;
        });
        Ok(preview)
    }

    async fn import_all(
        &self,
        source: ImportSource,
        path: &Path,
        status: &mut ImportStatus,
    ) -> Result<()> {
        let pool = open(path).await?;
        for ws in read_workspaces(&pool, source).await? {
            match self.import_workspace(&pool, source, &ws, status).await {
                Ok(true) => status.workspaces += 1,
                Ok(false) => status.skipped += 1,
                Err(e) => {
                    tracing::warn!("couldn't import {}: {e:#}", ws.path);
                    status.skipped += 1;
                }
            }
            let _ = self
                .store
                .emit(EventData::ImportUpdated {
                    status: status.clone(),
                })
                .await;
        }
        pool.close().await;
        Ok(())
    }

    /// `Ok(false)` when there's nothing to adopt: the worktree is gone, or it's
    /// already a Bantu workspace (so importing twice is safe).
    async fn import_workspace(
        &self,
        pool: &SqlitePool,
        source: ImportSource,
        src: &SourceWorkspace,
        status: &mut ImportStatus,
    ) -> Result<bool> {
        let path = Path::new(&src.path);
        if !path.is_dir() || !Path::new(&src.repo_root).is_dir() {
            return Ok(false);
        }
        let path_string = crate::platform::canonical(path)?
            .to_string_lossy()
            .to_string();
        let existing = self.store.workspaces().await?;
        if existing.iter().any(|w| w.path == path_string) {
            return Ok(false);
        }
        // Only adopt a real worktree of the repo the source says it belongs to.
        let actual_root = git::repo_root(path).await?;
        if crate::platform::canonical(&actual_root)?
            != crate::platform::canonical(Path::new(&src.repo_root))?
        {
            return Ok(false);
        }
        let Some(branch) = git::git_opt(path, &["symbolic-ref", "--short", "HEAD"]).await? else {
            return Ok(false);
        };
        let repo = self.adopt_repo(&src.repo_root).await?;
        let target = src
            .target_branch
            .clone()
            .unwrap_or_else(|| repo.default_branch.clone());
        let target = target
            .strip_prefix("origin/")
            .unwrap_or(&target)
            .to_string();
        let remote = format!("origin/{target}");
        let base_ref = if git::ref_exists(path, &format!("refs/remotes/{remote}")).await? {
            remote
        } else if git::ref_exists(path, &format!("refs/heads/{target}")).await? {
            target
        } else {
            repo.default_branch.clone()
        };

        let ws = self
            .adopt_workspace(&repo, &clean_name(&src.name), path_string, branch, base_ref)
            .await?;
        for chat in read_chats(pool, source, &src.id).await? {
            let rows = read_messages(pool, source, &chat.id).await?;
            let items = to_items(rows);
            let mut session = Session {
                id: new_id(),
                workspace_id: ws.id.clone(),
                agent: chat.agent,
                title: chat.title,
                model: chat.model,
                permission_mode: chat.permission_mode,
                effort: None,
                fast: false,
                forked_from: None,
                status: SessionStatus::Idle,
                queued: 0,
                turns: items.iter().map(|(turn, _)| *turn).max().unwrap_or(0),
                total_cost_usd: 0.0,
                created_at: chat.created_at,
            };
            for (_, item) in &items {
                if let ChatItem::TurnEnd {
                    cost_usd: Some(cost),
                    ..
                } = item
                {
                    session.total_cost_usd += cost;
                }
            }
            self.store
                .upsert_session(&session, &chat.conversation_id)
                .await?;
            status.items += items.len() as u32;
            for (turn, item) in items {
                self.store.session_item(&session, turn, item).await?;
            }
            status.chats += 1;
        }
        // Every workspace has a chat, even if nothing could come across.
        if self.first_session(&ws.id).await.is_none() {
            self.create_session(&ws.id, None, None, None).await?;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn opening_leaves_no_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("tool.db");
        // A WAL database as a tool leaves it: closing checkpoints and removes the
        // sidecar files.
        let options = SqliteConnectOptions::new()
            .filename(&db)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        let writer = SqlitePool::connect_with(options).await.unwrap();
        sqlx::query("CREATE TABLE t (x)")
            .execute(&writer)
            .await
            .unwrap();
        writer.close().await;
        let names = || {
            let mut names: Vec<String> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
                .collect();
            names.sort();
            names
        };
        assert_eq!(names(), ["tool.db"]);
        let before = std::fs::read(&db).unwrap();

        let pool = open(&db).await.unwrap();
        let tables: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(tables, 1);
        assert_eq!(names(), ["tool.db"], "no sidecar files while open");
        pool.close().await;
        assert_eq!(names(), ["tool.db"]);
        assert_eq!(std::fs::read(&db).unwrap(), before);
        assert!(open(&dir.path().join("missing.db")).await.is_err());
    }

    #[test]
    fn cleans_names() {
        assert_eq!(clean_name("Ashgabat"), "ashgabat");
        assert_eq!(clean_name("Fix the login!"), "fix-the-login");
        assert_eq!(clean_name("  --x--  "), "x");
        assert_eq!(clean_name("修正"), "imported");
    }

    #[test]
    fn maps_modes_safely() {
        assert_eq!(parse_mode(Some("plan")), PermissionMode::Plan);
        assert_eq!(
            parse_mode(Some("bypassPermissions")),
            PermissionMode::BypassPermissions
        );
        assert_eq!(parse_mode(Some("default")), PermissionMode::Ask);
        assert_eq!(parse_mode(Some("something new")), PermissionMode::Ask);
        assert_eq!(parse_mode(None), PermissionMode::Ask);
    }

    #[test]
    fn reads_stored_rows() {
        let assistant =
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"On it"}]}});
        let result = json!({"type":"result","is_error":false,"total_cost_usd":0.5,"usage":{}});
        let hook = json!({"type":"system","subtype":"hook_started"});
        let items = to_items(vec![
            ("assistant".into(), "cut off from an earlier turn".into()),
            ("user".into(), "fix it".into()),
            ("assistant".into(), assistant.to_string()),
            ("assistant".into(), hook.to_string()),
            ("assistant".into(), result.to_string()),
            ("user".into(), "and again".into()),
            ("assistant".into(), "plain text reply".into()),
            ("assistant".into(), "   ".into()),
        ]);
        assert_eq!(items.len(), 6, "{items:?}");
        assert_eq!(items[0].0, 1);
        assert_eq!(
            items[1],
            (
                1,
                ChatItem::User {
                    text: "fix it".into()
                }
            )
        );
        assert_eq!(
            items[2],
            (
                1,
                ChatItem::Assistant {
                    text: "On it".into()
                }
            )
        );
        assert!(matches!(
            items[3],
            (
                1,
                ChatItem::TurnEnd {
                    cost_usd: Some(_),
                    ..
                }
            )
        ));
        assert_eq!(
            items[4],
            (
                2,
                ChatItem::User {
                    text: "and again".into()
                }
            )
        );
        assert_eq!(
            items[5],
            (
                2,
                ChatItem::Assistant {
                    text: "plain text reply".into()
                }
            )
        );
    }
}
