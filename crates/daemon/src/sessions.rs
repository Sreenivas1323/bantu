//! Agent chat sessions.
//!
//! Each turn runs the user's own `claude` CLI in print mode with
//! `--output-format stream-json`, resuming the same conversation id every
//! turn. One process per turn keeps the daemon simple and means a daemon
//! restart never strands a half-attached process: the next turn just resumes.

use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use bantu_git as git;
use bantu_protocol::{
    AgentKind, ChatItem, PermissionMode, Session, SessionStatus, StreamDelta, Workspace,
    WorkspaceState,
};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, Notify};

use crate::codex;
use crate::ops::{new_id, CommandError, Daemon};
use crate::store::now;

/// Tool output beyond this is cut; the agent already saw the full text.
const MAX_TOOL_RESULT_CHARS: usize = 20_000;
const DEFAULT_TITLE: &str = "New chat";

/// In-memory state for sessions with a turn in flight.
pub struct Running {
    turns: Mutex<HashMap<String, RunningSession>>,
    /// Live text fragments; never persisted.
    deltas: tokio::sync::broadcast::Sender<StreamDelta>,
}

impl Default for Running {
    fn default() -> Self {
        Self {
            turns: Mutex::new(HashMap::new()),
            deltas: tokio::sync::broadcast::channel(1024).0,
        }
    }
}

struct RunningSession {
    queue: VecDeque<String>,
    stop: Arc<Notify>,
    stopped: bool,
}

type CmdResult<T> = Result<T, CommandError>;

impl Daemon {
    pub async fn create_session(
        &self,
        workspace_id: &str,
        agent: Option<AgentKind>,
        model: Option<String>,
        permission_mode: Option<PermissionMode>,
    ) -> CmdResult<Session> {
        let ws = self.require_workspace(workspace_id).await?;
        if ws.state != WorkspaceState::Ready {
            return Err(CommandError::Conflict(format!(
                "workspace is {}, not ready",
                ws.state.as_str()
            )));
        }
        let session = Session {
            id: new_id(),
            workspace_id: ws.id,
            agent: agent.unwrap_or(AgentKind::Claude),
            title: DEFAULT_TITLE.into(),
            model: model.filter(|m| !m.trim().is_empty()),
            permission_mode: permission_mode.unwrap_or_default(),
            status: SessionStatus::Idle,
            queued: 0,
            turns: 0,
            total_cost_usd: 0.0,
            created_at: now(),
        };
        let agent_session_id = uuid::Uuid::new_v4().to_string();
        self.store
            .upsert_session(&session, &agent_session_id)
            .await?;
        Ok(session)
    }

    pub async fn update_session(
        &self,
        session_id: &str,
        title: Option<String>,
        model: Option<String>,
        permission_mode: Option<PermissionMode>,
    ) -> CmdResult<Session> {
        let (mut s, agent_id) = self.require_session(session_id).await?;
        if let Some(t) = title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
        {
            s.title = t;
        }
        if let Some(m) = model {
            s.model = Some(m).filter(|m| !m.trim().is_empty());
        }
        if let Some(p) = permission_mode {
            s.permission_mode = p;
        }
        self.store.upsert_session(&s, &agent_id).await?;
        Ok(s)
    }

    pub async fn send_message(
        self: &Arc<Self>,
        session_id: &str,
        text: String,
    ) -> CmdResult<Session> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(CommandError::BadRequest("message is empty".into()));
        }
        let (mut s, agent_id) = self.require_session(session_id).await?;
        let ws = self.require_workspace(&s.workspace_id).await?;
        if ws.state != WorkspaceState::Ready {
            return Err(CommandError::Conflict(format!(
                "workspace is {}, not ready",
                ws.state.as_str()
            )));
        }

        let mut running = self.running.turns.lock().await;
        if let Some(r) = running.get_mut(&s.id) {
            r.queue.push_back(text);
            s.queued = r.queue.len() as u32;
            self.store.upsert_session(&s, &agent_id).await?;
            return Ok(s);
        }
        let stop = Arc::new(Notify::new());
        running.insert(
            s.id.clone(),
            RunningSession {
                queue: VecDeque::new(),
                stop: stop.clone(),
                stopped: false,
            },
        );
        drop(running);

        if s.title == DEFAULT_TITLE {
            s.title = title_from(&text);
            if let Err(e) = self.name_branch(&ws, &text).await {
                tracing::warn!("couldn't rename branch for {}: {e:#}", ws.id);
            }
        }
        let text = match self.store.last_session_item(&s.id).await? {
            Some(ChatItem::Rewound { to_turn }) => format!(
                "[Bantu] The user restored the workspace files to how they were before turn \
                 {to_turn}; changes made after that point were undone. Re-read files before \
                 relying on earlier edits.\n\n{text}"
            ),
            _ => text,
        };
        s.status = SessionStatus::Running;
        self.store.upsert_session(&s, &agent_id).await?;
        tokio::spawn(self.clone().drive(s.clone(), agent_id, text, stop));
        Ok(s)
    }

    pub async fn cancel_session(&self, session_id: &str) -> CmdResult<Session> {
        let (mut s, agent_id) = self.require_session(session_id).await?;
        // A turn blocked on a permission prompt must be released to stop.
        self.deny_session_permissions(session_id).await;
        if let Some(r) = self.running.turns.lock().await.get_mut(session_id) {
            r.stopped = true;
            r.queue.clear();
            r.stop.notify_one();
            s.queued = 0;
            self.store.upsert_session(&s, &agent_id).await?;
        }
        Ok(s)
    }

    pub async fn rewind_session(&self, session_id: &str, turn: u32) -> CmdResult<Session> {
        let (s, _) = self.require_session(session_id).await?;
        if s.status == SessionStatus::Running {
            return Err(CommandError::Conflict(
                "stop the agent before rewinding".into(),
            ));
        }
        if turn == 0 || turn > s.turns {
            return Err(CommandError::BadRequest(format!(
                "turn {turn} doesn't exist"
            )));
        }
        let ws = self.require_workspace(&s.workspace_id).await?;
        let path = std::path::Path::new(&ws.path);
        let r = checkpoint_ref(&s.id, turn);
        let sha = git::git_opt(path, &["rev-parse", "--verify", "--quiet", &r])
            .await
            .map_err(anyhow::Error::from)?
            .ok_or_else(|| CommandError::NotFound(format!("no checkpoint for turn {turn}")))?;
        if git::busy_state(path).await.map_err(anyhow::Error::from)? != git::BusyState::Clean {
            return Err(CommandError::Conflict(
                "a merge, rebase or cherry-pick is in progress".into(),
            ));
        }
        git::restore_checkpoint(path, &sha)
            .await
            .map_err(anyhow::Error::from)?;
        self.store
            .session_item(&s, s.turns, ChatItem::Rewound { to_turn: turn })
            .await?;
        Ok(s)
    }

    /// Replaces the placeholder branch (`bantu/<city>`) with one named after
    /// the task, e.g. `bantu/fix-login-redirect`, before anything is pushed.
    async fn name_branch(&self, ws: &Workspace, first_message: &str) -> anyhow::Result<()> {
        let placeholder = format!("{}{}", self.config.branch_prefix, ws.name);
        if ws.branch != placeholder {
            return Ok(());
        }
        let path = std::path::Path::new(&ws.path);
        if git::ref_exists(path, &format!("refs/remotes/origin/{}", ws.branch)).await? {
            return Ok(());
        }
        let Some(slug) = branch_slug(first_message) else {
            return Ok(());
        };
        let base = format!("{}{slug}", self.config.branch_prefix);
        let mut candidate = base.clone();
        let mut n = 2;
        while git::ref_exists(path, &format!("refs/heads/{candidate}")).await? {
            candidate = format!("{base}-{n}");
            n += 1;
        }
        git::git(path, &["branch", "-m", &ws.branch, &candidate]).await?;
        let mut ws = ws.clone();
        ws.branch = candidate;
        self.store.upsert_workspace(&ws).await?;
        Ok(())
    }

    /// Snapshot the worktree before a turn so it can be rewound. Best effort:
    /// a turn still runs if git is mid-merge or the snapshot fails.
    async fn checkpoint_turn(&self, s: &Session, turn: u32) {
        let Ok(ws) = self.require_workspace(&s.workspace_id).await else {
            return;
        };
        let path = std::path::Path::new(&ws.path);
        if !matches!(git::busy_state(path).await, Ok(git::BusyState::Clean)) {
            return;
        }
        if let Err(e) = git::checkpoint(path, &checkpoint_ref(&s.id, turn)).await {
            tracing::warn!("checkpoint for session {} turn {turn} failed: {e}", s.id);
        }
    }

    pub fn subscribe_deltas(&self) -> tokio::sync::broadcast::Receiver<StreamDelta> {
        self.running.deltas.subscribe()
    }

    /// Stops every session in a workspace (used before archiving).
    pub async fn cancel_workspace_sessions(&self, workspace_id: &str) -> anyhow::Result<()> {
        for s in self.store.sessions().await? {
            if s.workspace_id == workspace_id && s.status == SessionStatus::Running {
                let _ = self.cancel_session(&s.id).await;
            }
        }
        Ok(())
    }

    /// Sessions marked running belonged to a previous daemon process.
    pub async fn recover_sessions(&self) -> anyhow::Result<()> {
        for mut s in self.store.sessions().await? {
            if s.status == SessionStatus::Running || s.queued > 0 {
                let (_, agent_id) = self
                    .store
                    .session(&s.id)
                    .await?
                    .context("session vanished")?;
                s.status = SessionStatus::Idle;
                s.queued = 0;
                self.store.upsert_session(&s, &agent_id).await?;
                self.store
                    .session_item(
                        &s,
                        s.turns,
                        ChatItem::Error {
                            message: "Interrupted by a daemon restart".into(),
                        },
                    )
                    .await?;
            }
        }
        Ok(())
    }

    /// Runs turns until the queue is empty or the user stops the session.
    async fn drive(
        self: Arc<Self>,
        mut s: Session,
        agent_id: String,
        first: String,
        stop: Arc<Notify>,
    ) {
        let mut text = first;
        loop {
            s.turns += 1;
            if let Err(e) = self.store.upsert_session(&s, &agent_id).await {
                tracing::error!("session {}: {e:#}", s.id);
            }
            let turn = s.turns;
            let _ = self
                .store
                .session_item(&s, turn, ChatItem::User { text: text.clone() })
                .await;
            self.checkpoint_turn(&s, turn).await;

            match self.run_turn(&s, &agent_id, turn, &text, &stop).await {
                Ok(cost) => s.total_cost_usd += cost,
                Err(e) => {
                    let _ = self
                        .store
                        .session_item(
                            &s,
                            turn,
                            ChatItem::Error {
                                message: format!("{e:#}"),
                            },
                        )
                        .await;
                }
            }

            let mut running = self.running.turns.lock().await;
            let next = running
                .get_mut(&s.id)
                .filter(|r| !r.stopped)
                .and_then(|r| r.queue.pop_front());
            // Reload: title, model or mode may have changed mid-turn.
            if let Ok(Some((fresh, _))) = self.store.session(&s.id).await {
                s.title = fresh.title;
                s.model = fresh.model;
                s.permission_mode = fresh.permission_mode;
            }
            match next {
                Some(n) => {
                    s.queued = running.get(&s.id).map_or(0, |r| r.queue.len() as u32);
                    text = n;
                }
                None => {
                    running.remove(&s.id);
                    s.status = SessionStatus::Idle;
                    s.queued = 0;
                    if let Err(e) = self.store.upsert_session(&s, &agent_id).await {
                        tracing::error!("session {}: {e:#}", s.id);
                    }
                    return;
                }
            }
        }
    }

    /// One `claude -p` invocation. Returns the turn's cost in USD.
    async fn run_turn(
        &self,
        s: &Session,
        agent_id: &str,
        turn: u32,
        text: &str,
        stop: &Notify,
    ) -> anyhow::Result<f64> {
        let ws = self.require_workspace(&s.workspace_id).await?;
        let resume = self.store.session_has_agent_output(&s.id).await?;

        let (bin, mut cmd, prompt) = match s.agent {
            AgentKind::Claude => {
                let mut cmd = tokio::process::Command::new(&self.config.claude_bin);
                cmd.args(["-p", "--output-format", "stream-json", "--verbose"])
                    .arg("--include-partial-messages")
                    .args(["--permission-prompts", "none"])
                    .args([
                        "--permission-mode",
                        claude_permission_mode(s.permission_mode),
                    ])
                    .args([if resume { "--resume" } else { "--session-id" }, agent_id])
                    .args(["--append-system-prompt", &workspace_context(&ws)]);
                if let Some(m) = &s.model {
                    cmd.args(["--model", m]);
                }
                if let Some(settings) = self.permission_hook_settings(&s.id) {
                    cmd.args(["--settings", &settings])
                        .env("BANTU_HOOK_TOKEN", &self.permissions.hook_token);
                }
                (&self.config.claude_bin, cmd, text.to_string())
            }
            AgentKind::Codex => {
                let mut cmd = tokio::process::Command::new(&self.config.codex_bin);
                cmd.args(codex::args(
                    s.permission_mode,
                    s.model.as_deref(),
                    resume.then_some(agent_id),
                ));
                // Codex has no system-prompt flag; brief it on the first turn.
                let prompt = if resume {
                    text.to_string()
                } else {
                    format!("{}\n\n{text}", workspace_context(&ws))
                };
                (&self.config.codex_bin, cmd, prompt)
            }
        };
        let mut child = cmd
            .current_dir(&ws.path)
            .env("BANTU_PORT", ws.port_base.to_string())
            .env("BANTU_WORKSPACE_ID", &ws.id)
            .env("BANTU_WORKSPACE_NAME", &ws.name)
            .env("BANTU_WORKSPACE_PATH", &ws.path)
            .env("BANTU_SESSION_ID", &s.id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "couldn't start `{}` — is it installed and on PATH?",
                    bin.display()
                )
            })?;

        // The prompt goes over stdin so long messages never hit argv limits.
        let mut stdin = child.stdin.take().expect("piped");
        stdin.write_all(prompt.as_bytes()).await?;
        drop(stdin);

        let mut stderr = child.stderr.take().expect("piped");
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        });

        let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
        let mut cost = 0.0;
        let mut saw_result = false;
        let mut codex = codex::Parser::default();
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Some(line) = line? else { break };
                    let Ok(v) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let items = match s.agent {
                        AgentKind::Claude => {
                            if let Some(text) = text_delta(&v) {
                                let _ = self.running.deltas.send(StreamDelta {
                                    session_id: s.id.clone(),
                                    workspace_id: s.workspace_id.clone(),
                                    turn,
                                    text,
                                });
                                continue;
                            }
                            parse_stream_line(&v)
                        }
                        AgentKind::Codex => {
                            let items = codex.feed(&v);
                            if let Some(id) = codex.thread_id.take() {
                                if id != agent_id {
                                    self.store.set_agent_session_id(&s.id, &id).await?;
                                }
                            }
                            items
                        }
                    };
                    for item in items {
                        if let ChatItem::TurnEnd { cost_usd, .. } = &item {
                            cost = cost_usd.unwrap_or(0.0);
                            saw_result = true;
                        }
                        self.store.session_item(s, turn, item).await?;
                    }
                }
                _ = stop.notified() => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    return Err(anyhow!("Stopped"));
                }
            }
        }
        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        if !saw_result {
            let tail: String = stderr
                .trim()
                .chars()
                .rev()
                .take(2000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            return Err(anyhow!(
                "{} exited ({status}) without finishing the turn. {tail}",
                s.agent.as_str()
            ));
        }
        Ok(cost)
    }

    async fn require_session(&self, id: &str) -> CmdResult<(Session, String)> {
        self.store
            .session(id)
            .await?
            .ok_or_else(|| CommandError::NotFound(format!("no session {id}")))
    }

    pub(crate) async fn require_workspace(&self, id: &str) -> CmdResult<Workspace> {
        self.store
            .workspace(id)
            .await?
            .ok_or_else(|| CommandError::NotFound(format!("no workspace {id}")))
    }
}

pub fn checkpoint_ref(session_id: &str, turn: u32) -> String {
    format!("refs/bantu/cp/{session_id}/{turn}")
}

fn claude_permission_mode(m: PermissionMode) -> &'static str {
    match m {
        PermissionMode::Auto => "auto",
        PermissionMode::Ask => "default",
        PermissionMode::AcceptEdits => "acceptEdits",
        PermissionMode::Plan => "plan",
        PermissionMode::BypassPermissions => "bypassPermissions",
    }
}

fn workspace_context(ws: &Workspace) -> String {
    format!(
        "You are running inside Bantu, in an isolated git worktree created for this task.\n\
         - Workspace: {name}. Branch: {branch} (created from {base}). Commit work to this branch; \
         don't switch branches or touch other worktrees.\n\
         - Ports {p}-{p_end} are reserved for this workspace ($BANTU_PORT is the first). \
         Use them for dev servers so parallel workspaces don't collide.",
        name = ws.name,
        branch = ws.branch,
        base = ws.base_ref,
        p = ws.port_base,
        p_end = ws.port_base + 9,
    )
}

/// A short, readable branch name from a task description:
/// "Please fix the login redirect bug!" → `fix-login-redirect-bug`.
pub fn branch_slug(text: &str) -> Option<String> {
    const SKIP: &[&str] = &[
        "a", "an", "the", "to", "and", "of", "in", "on", "for", "with", "please", "can", "could",
        "you", "i", "me", "my", "we", "our", "this", "that", "it", "is", "are", "be", "so", "just",
        "hey", "hi", "let's", "lets", "some", "any",
    ];
    let words: Vec<String> = text
        .lines()
        .next()
        .unwrap_or("")
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '\'')
        .map(|w| w.replace('\'', ""))
        .filter(|w| !w.is_empty() && !SKIP.contains(&w.as_str()))
        .take(5)
        .collect();
    let mut slug = String::new();
    for w in words {
        if slug.len() + w.len() + 1 > 40 {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&w);
    }
    (slug.len() >= 3).then_some(slug)
}

fn title_from(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    if title.is_empty() {
        DEFAULT_TITLE.into()
    } else {
        title
    }
}

/// Text fragment from a top-level `--include-partial-messages` event.
/// Subagent output (`parent_tool_use_id` set) isn't streamed.
fn text_delta(v: &Value) -> Option<String> {
    if v["type"] != "stream_event" || !v["parent_tool_use_id"].is_null() {
        return None;
    }
    let delta = &v["event"]["delta"];
    (v["event"]["type"] == "content_block_delta" && delta["type"] == "text_delta")
        .then(|| delta["text"].as_str().map(str::to_string))
        .flatten()
}

/// Maps one line of Claude's `stream-json` output to transcript items.
/// Unknown or uninteresting lines (hooks, rate limits, init) map to nothing.
pub fn parse_stream_line(v: &Value) -> Vec<ChatItem> {
    let blocks = || {
        v.pointer("/message/content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    match v["type"].as_str() {
        Some("assistant") => blocks()
            .iter()
            .filter_map(|b| match b["type"].as_str()? {
                "text" => {
                    let text = b["text"].as_str()?.to_string();
                    (!text.trim().is_empty()).then_some(ChatItem::Assistant { text })
                }
                // Redacted thinking arrives empty with only a signature.
                "thinking" => {
                    let text = b["thinking"].as_str()?.to_string();
                    (!text.trim().is_empty()).then_some(ChatItem::Thinking { text })
                }
                "tool_use" => Some(ChatItem::ToolUse {
                    id: b["id"].as_str()?.to_string(),
                    name: b["name"].as_str()?.to_string(),
                    input: b["input"].clone(),
                }),
                _ => None,
            })
            .collect(),
        Some("user") => blocks()
            .iter()
            .filter(|b| b["type"] == "tool_result")
            .map(|b| ChatItem::ToolResult {
                tool_use_id: b["tool_use_id"].as_str().unwrap_or_default().to_string(),
                content: truncate(tool_result_text(&b["content"])),
                is_error: b["is_error"].as_bool().unwrap_or(false),
            })
            .collect(),
        Some("result") => {
            let usage = &v["usage"];
            let input = [
                "input_tokens",
                "cache_creation_input_tokens",
                "cache_read_input_tokens",
            ]
            .iter()
            .filter_map(|k| usage[k].as_u64())
            .reduce(|a, b| a + b);
            vec![ChatItem::TurnEnd {
                is_error: v["is_error"].as_bool().unwrap_or(false),
                duration_ms: v["duration_ms"].as_u64(),
                cost_usd: v["total_cost_usd"].as_f64(),
                input_tokens: input,
                output_tokens: usage["output_tokens"].as_u64(),
            }]
        }
        _ => vec![],
    }
}

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p["type"].as_str() {
                Some("text") => p["text"].as_str().map(str::to_string),
                Some("image") => Some("[image]".into()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn truncate(s: String) -> String {
    if s.chars().count() <= MAX_TOOL_RESULT_CHARS {
        return s;
    }
    let mut out: String = s.chars().take(MAX_TOOL_RESULT_CHARS).collect();
    out.push_str("\n… [truncated by Bantu]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_claude_stream_json() {
        let tool = json!({"type":"assistant","message":{"content":[
            {"type":"thinking","thinking":"","signature":"x"},
            {"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}});
        assert_eq!(
            parse_stream_line(&tool),
            vec![ChatItem::ToolUse {
                id: "t1".into(),
                name: "Bash".into(),
                input: json!({"command":"ls"})
            }]
        );

        let result = json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"a.txt"}],"is_error":false}]}});
        assert_eq!(
            parse_stream_line(&result),
            vec![ChatItem::ToolResult {
                tool_use_id: "t1".into(),
                content: "a.txt".into(),
                is_error: false
            }]
        );

        let end = json!({"type":"result","is_error":false,"duration_ms":1200,"total_cost_usd":0.01,
            "usage":{"input_tokens":4,"cache_read_input_tokens":100,"output_tokens":7}});
        assert_eq!(
            parse_stream_line(&end),
            vec![ChatItem::TurnEnd {
                is_error: false,
                duration_ms: Some(1200),
                cost_usd: Some(0.01),
                input_tokens: Some(104),
                output_tokens: Some(7)
            }]
        );

        assert!(parse_stream_line(&json!({"type":"system","subtype":"init"})).is_empty());

        let delta = json!({"type":"stream_event","parent_tool_use_id":null,
            "event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Riv"}}});
        assert_eq!(text_delta(&delta).as_deref(), Some("Riv"));
        let sub = json!({"type":"stream_event","parent_tool_use_id":"toolu_1",
            "event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"x"}}});
        assert_eq!(text_delta(&sub), None);
    }

    #[test]
    fn branch_slugs() {
        assert_eq!(
            branch_slug("Please fix the login redirect bug!").as_deref(),
            Some("fix-login-redirect-bug")
        );
        assert_eq!(
            branch_slug("Add dark mode to settings page and footer").as_deref(),
            Some("add-dark-mode-settings-page")
        );
        assert_eq!(
            branch_slug("Don't crash on empty config").as_deref(),
            Some("dont-crash-empty-config")
        );
        assert_eq!(branch_slug("修正"), None);
        assert_eq!(branch_slug("hi"), None);
    }

    #[test]
    fn titles_and_truncation() {
        assert_eq!(title_from("  fix the login bug\nmore"), "fix the login bug");
        assert_eq!(title_from(&"x".repeat(80)).chars().count(), 61);
        assert!(truncate("y".repeat(MAX_TOOL_RESULT_CHARS + 5)).ends_with("[truncated by Bantu]"));
    }
}
