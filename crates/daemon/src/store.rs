//! SQLite-backed event log plus the `repos`/`workspaces` projections.
//!
//! Every state change goes through [`Store`]: the projection write and the
//! event insert share one transaction, and events are broadcast in `seq` order
//! (writes are serialized by `write_lock`), so SSE subscribers never see gaps
//! or reordering.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use bantu_protocol::{
    AgentKind, ChatItem, DiffComment, Event, EventData, LogStream, PermissionMode, Repo, Session,
    SessionStatus, Workspace, WorkspaceState,
};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tokio::sync::{broadcast, Mutex, MutexGuard};

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub struct Store {
    pool: SqlitePool,
    events_tx: broadcast::Sender<Event>,
    write_lock: Mutex<()>,
}

impl Store {
    pub async fn open(db_path: &Path) -> Result<Self> {
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts)
            .await
            .with_context(|| format!("opening {}", db_path.display()))?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        let (events_tx, _) = broadcast::channel(1024);
        Ok(Self {
            pool,
            events_tx,
            write_lock: Mutex::new(()),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events_tx.subscribe()
    }

    // ---- writes -----------------------------------------------------------

    pub async fn insert_repo(&self, repo: &Repo) -> Result<Event> {
        let (guard, mut tx) = self.begin().await?;
        sqlx::query(
            "INSERT INTO repos (id, name, root_path, default_branch, remote_url, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&repo.id)
        .bind(&repo.name)
        .bind(&repo.root_path)
        .bind(&repo.default_branch)
        .bind(&repo.remote_url)
        .bind(&repo.created_at)
        .execute(&mut *tx)
        .await?;
        let data = EventData::RepoAdded { repo: repo.clone() };
        self.finish(guard, tx, data).await
    }

    pub async fn delete_repo(&self, repo_id: &str) -> Result<Event> {
        let (guard, mut tx) = self.begin().await?;
        sqlx::query("DELETE FROM workspaces WHERE repo_id = ?")
            .bind(repo_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM repos WHERE id = ?")
            .bind(repo_id)
            .execute(&mut *tx)
            .await?;
        let data = EventData::RepoRemoved {
            repo_id: repo_id.to_string(),
        };
        self.finish(guard, tx, data).await
    }

    pub async fn upsert_workspace(&self, ws: &Workspace) -> Result<Event> {
        let (guard, mut tx) = self.begin().await?;
        sqlx::query(
            "INSERT INTO workspaces (id, repo_id, name, branch, base_ref, path, port_base,
                                     state, error, archive_commit, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name, branch = excluded.branch,
                base_ref = excluded.base_ref, path = excluded.path,
                port_base = excluded.port_base, state = excluded.state,
                error = excluded.error, archive_commit = excluded.archive_commit",
        )
        .bind(&ws.id)
        .bind(&ws.repo_id)
        .bind(&ws.name)
        .bind(&ws.branch)
        .bind(&ws.base_ref)
        .bind(&ws.path)
        .bind(ws.port_base as i64)
        .bind(ws.state.as_str())
        .bind(&ws.error)
        .bind(&ws.archive_commit)
        .bind(&ws.created_at)
        .execute(&mut *tx)
        .await?;
        let data = EventData::WorkspaceUpdated {
            workspace: ws.clone(),
        };
        self.finish(guard, tx, data).await
    }

    pub async fn log(&self, workspace_id: &str, stream: LogStream, line: impl Into<String>) {
        let data = EventData::WorkspaceLog {
            workspace_id: workspace_id.to_string(),
            stream,
            line: line.into(),
        };
        let result = async {
            let (guard, tx) = self.begin().await?;
            self.finish(guard, tx, data).await
        }
        .await;
        if let Err(e) = result {
            tracing::warn!("failed to record workspace log: {e:#}");
        }
    }

    /// `agent_session_id` is only written on insert; it never changes.
    pub async fn upsert_session(&self, s: &Session, agent_session_id: &str) -> Result<Event> {
        let (guard, mut tx) = self.begin().await?;
        sqlx::query(
            "INSERT INTO sessions (id, workspace_id, agent, title, model, permission_mode, status,
                                   queued, turns, total_cost_usd, agent_session_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title, model = excluded.model,
                permission_mode = excluded.permission_mode, status = excluded.status,
                queued = excluded.queued, turns = excluded.turns,
                total_cost_usd = excluded.total_cost_usd",
        )
        .bind(&s.id)
        .bind(&s.workspace_id)
        .bind("claude")
        .bind(&s.title)
        .bind(&s.model)
        .bind(s.permission_mode.as_str())
        .bind(status_str(s.status))
        .bind(s.queued as i64)
        .bind(s.turns as i64)
        .bind(s.total_cost_usd)
        .bind(agent_session_id)
        .bind(&s.created_at)
        .execute(&mut *tx)
        .await?;
        let data = EventData::SessionUpdated { session: s.clone() };
        self.finish(guard, tx, data).await
    }

    pub async fn session_item(&self, s: &Session, turn: u32, item: ChatItem) -> Result<Event> {
        let (guard, tx) = self.begin().await?;
        let data = EventData::SessionItem {
            session_id: s.id.clone(),
            workspace_id: s.workspace_id.clone(),
            turn,
            item,
        };
        self.finish(guard, tx, data).await
    }

    pub async fn upsert_comment(&self, c: &DiffComment) -> Result<Event> {
        let (guard, mut tx) = self.begin().await?;
        sqlx::query(
            "INSERT INTO diff_comments (id, workspace_id, path, line, body, created_at, sent_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET body = excluded.body, sent_at = excluded.sent_at",
        )
        .bind(&c.id)
        .bind(&c.workspace_id)
        .bind(&c.path)
        .bind(c.line as i64)
        .bind(&c.body)
        .bind(&c.created_at)
        .bind(&c.sent_at)
        .execute(&mut *tx)
        .await?;
        let data = EventData::CommentUpdated { comment: c.clone() };
        self.finish(guard, tx, data).await
    }

    pub async fn delete_comment(&self, c: &DiffComment) -> Result<Event> {
        let (guard, mut tx) = self.begin().await?;
        sqlx::query("DELETE FROM diff_comments WHERE id = ?")
            .bind(&c.id)
            .execute(&mut *tx)
            .await?;
        let data = EventData::CommentDeleted {
            comment_id: c.id.clone(),
            workspace_id: c.workspace_id.clone(),
        };
        self.finish(guard, tx, data).await
    }

    /// Records an event that has no projection (e.g. in-memory terminals).
    pub async fn emit(&self, data: EventData) -> Result<Event> {
        let (guard, tx) = self.begin().await?;
        self.finish(guard, tx, data).await
    }

    async fn begin(&self) -> Result<(MutexGuard<'_, ()>, Transaction<'static, Sqlite>)> {
        let guard = self.write_lock.lock().await;
        let tx = self.pool.begin().await?;
        Ok((guard, tx))
    }

    /// Appends `data` to the event log, commits, then broadcasts while still
    /// holding the write lock so subscribers see events in `seq` order.
    async fn finish(
        &self,
        _guard: MutexGuard<'_, ()>,
        mut tx: Transaction<'static, Sqlite>,
        data: EventData,
    ) -> Result<Event> {
        let ts = now();
        let payload = serde_json::to_string(&data)?;
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO events (ts, kind, payload, workspace_id) VALUES (?, ?, ?, ?) RETURNING seq",
        )
        .bind(&ts)
        .bind(event_kind(&data))
        .bind(&payload)
        .bind(data.workspace_id())
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        let event = Event { seq, ts, data };
        // No subscribers is fine; they replay from the DB when they connect.
        let _ = self.events_tx.send(event.clone());
        Ok(event)
    }

    // ---- reads ------------------------------------------------------------

    pub async fn last_seq(&self) -> Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM events")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn events_since(&self, since: i64, limit: i64) -> Result<Vec<Event>> {
        let rows =
            sqlx::query("SELECT seq, ts, payload FROM events WHERE seq > ? ORDER BY seq LIMIT ?")
                .bind(since)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Event {
                    seq: r.get("seq"),
                    ts: r.get("ts"),
                    data: serde_json::from_str(r.get::<&str, _>("payload"))?,
                })
            })
            .collect()
    }

    pub async fn repos(&self) -> Result<Vec<Repo>> {
        let rows = sqlx::query("SELECT * FROM repos ORDER BY created_at")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(repo_from_row).collect())
    }

    pub async fn repo(&self, id: &str) -> Result<Option<Repo>> {
        let row = sqlx::query("SELECT * FROM repos WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(repo_from_row))
    }

    pub async fn repo_by_root(&self, root_path: &str) -> Result<Option<Repo>> {
        let row = sqlx::query("SELECT * FROM repos WHERE root_path = ?")
            .bind(root_path)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(repo_from_row))
    }

    pub async fn workspaces(&self) -> Result<Vec<Workspace>> {
        let rows = sqlx::query("SELECT * FROM workspaces ORDER BY created_at")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(workspace_from_row).collect()
    }

    pub async fn workspace(&self, id: &str) -> Result<Option<Workspace>> {
        let row = sqlx::query("SELECT * FROM workspaces WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(workspace_from_row).transpose()
    }

    pub async fn sessions(&self) -> Result<Vec<Session>> {
        let rows = sqlx::query("SELECT * FROM sessions ORDER BY created_at")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(session_from_row).collect()
    }

    /// The session plus the agent's own conversation id.
    pub async fn session(&self, id: &str) -> Result<Option<(Session, String)>> {
        let row = sqlx::query("SELECT * FROM sessions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| Ok((session_from_row(&r)?, r.get("agent_session_id"))))
            .transpose()
    }

    /// Whether the agent has produced anything for this session yet, i.e.
    /// whether its conversation exists and can be resumed.
    pub async fn session_has_agent_output(&self, session_id: &str) -> Result<bool> {
        let found: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM events
             WHERE kind = 'session.item'
               AND json_extract(payload, '$.session_id') = ?
               AND json_extract(payload, '$.item.kind') IN ('assistant', 'tool_use', 'turn_end')
             LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(found.is_some())
    }

    /// The most recent `limit` events for one workspace, oldest first.
    pub async fn workspace_history(&self, workspace_id: &str, limit: i64) -> Result<Vec<Event>> {
        let rows = sqlx::query(
            "SELECT seq, ts, payload FROM (
                SELECT seq, ts, payload FROM events WHERE workspace_id = ?
                ORDER BY seq DESC LIMIT ?
             ) ORDER BY seq",
        )
        .bind(workspace_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Event {
                    seq: r.get("seq"),
                    ts: r.get("ts"),
                    data: serde_json::from_str(r.get::<&str, _>("payload"))?,
                })
            })
            .collect()
    }

    pub async fn comments(&self) -> Result<Vec<DiffComment>> {
        let rows = sqlx::query("SELECT * FROM diff_comments ORDER BY created_at")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(comment_from_row).collect())
    }

    pub async fn comment(&self, id: &str) -> Result<Option<DiffComment>> {
        let row = sqlx::query("SELECT * FROM diff_comments WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(comment_from_row))
    }

    /// Terminals the log says are open. They can't survive a restart, so the
    /// daemon closes them on startup.
    pub async fn open_terminals_in_log(&self) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query(
            "SELECT json_extract(payload, '$.terminal.id') AS id, workspace_id FROM events
             WHERE kind = 'terminal.updated'
             EXCEPT
             SELECT json_extract(payload, '$.terminal_id'), workspace_id FROM events
             WHERE kind = 'terminal.closed'",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get("id"), r.get("workspace_id")))
            .collect())
    }

    // ---- idempotency ------------------------------------------------------

    pub async fn idempotent_response(&self, key: &str) -> Result<Option<(u16, String)>> {
        let row = sqlx::query(
            "SELECT status, response FROM idempotency
             WHERE key = ? AND created_at > datetime('now', '-1 day')",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| (r.get::<i64, _>("status") as u16, r.get("response"))))
    }

    pub async fn save_idempotent_response(&self, key: &str, status: u16, body: &str) -> Result<()> {
        sqlx::query("DELETE FROM idempotency WHERE created_at <= datetime('now', '-1 day')")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "INSERT OR REPLACE INTO idempotency (key, status, response, created_at)
             VALUES (?, ?, ?, datetime('now'))",
        )
        .bind(key)
        .bind(status as i64)
        .bind(body)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn event_kind(data: &EventData) -> &'static str {
    match data {
        EventData::RepoAdded { .. } => "repo.added",
        EventData::RepoRemoved { .. } => "repo.removed",
        EventData::WorkspaceUpdated { .. } => "workspace.updated",
        EventData::WorkspaceLog { .. } => "workspace.log",
        EventData::SessionUpdated { .. } => "session.updated",
        EventData::SessionItem { .. } => "session.item",
        EventData::CommentUpdated { .. } => "comment.updated",
        EventData::CommentDeleted { .. } => "comment.deleted",
        EventData::TerminalUpdated { .. } => "terminal.updated",
        EventData::TerminalClosed { .. } => "terminal.closed",
    }
}

fn repo_from_row(r: &SqliteRow) -> Repo {
    Repo {
        id: r.get("id"),
        name: r.get("name"),
        root_path: r.get("root_path"),
        default_branch: r.get("default_branch"),
        remote_url: r.get("remote_url"),
        created_at: r.get("created_at"),
    }
}

fn workspace_from_row(r: &SqliteRow) -> Result<Workspace> {
    let state: String = r.get("state");
    Ok(Workspace {
        id: r.get("id"),
        repo_id: r.get("repo_id"),
        name: r.get("name"),
        branch: r.get("branch"),
        base_ref: r.get("base_ref"),
        path: r.get("path"),
        port_base: r.get::<i64, _>("port_base") as u16,
        state: WorkspaceState::parse(&state)
            .with_context(|| format!("unknown workspace state {state:?}"))?,
        error: r.get("error"),
        archive_commit: r.get("archive_commit"),
        created_at: r.get("created_at"),
    })
}

fn status_str(s: SessionStatus) -> &'static str {
    match s {
        SessionStatus::Idle => "idle",
        SessionStatus::Running => "running",
    }
}

fn session_from_row(r: &SqliteRow) -> Result<Session> {
    let mode: String = r.get("permission_mode");
    let status: String = r.get("status");
    Ok(Session {
        id: r.get("id"),
        workspace_id: r.get("workspace_id"),
        agent: AgentKind::Claude,
        title: r.get("title"),
        model: r.get("model"),
        permission_mode: PermissionMode::parse(&mode)
            .with_context(|| format!("unknown permission mode {mode:?}"))?,
        status: match status.as_str() {
            "running" => SessionStatus::Running,
            _ => SessionStatus::Idle,
        },
        queued: r.get::<i64, _>("queued") as u32,
        turns: r.get::<i64, _>("turns") as u32,
        total_cost_usd: r.get("total_cost_usd"),
        created_at: r.get("created_at"),
    })
}

fn comment_from_row(r: &SqliteRow) -> DiffComment {
    DiffComment {
        id: r.get("id"),
        workspace_id: r.get("workspace_id"),
        path: r.get("path"),
        line: r.get::<i64, _>("line") as u32,
        body: r.get("body"),
        created_at: r.get("created_at"),
        sent_at: r.get("sent_at"),
    }
}
