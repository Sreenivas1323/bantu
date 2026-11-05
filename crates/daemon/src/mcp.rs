//! Bantu's own MCP server, injected into every agent turn.
//!
//! It turns any chat into an orchestrator: the agent can start workspaces
//! with their own agents and tasks, check on them, wait for them, read their
//! diffs and send follow-ups. Speaks MCP's Streamable HTTP transport with
//! plain JSON responses at `POST /v1/mcp?session=<id>`, authenticated with
//! the per-process hook token (like the permission hook).

use std::sync::Arc;
use std::time::{Duration, Instant};

use bantu_protocol::{
    AgentKind, ChatItem, EventData, Session, SessionStatus, TerminalKind, Workspace, WorkspaceState,
};
use serde_json::{json, Value};

use crate::inbox::Answer;
use crate::ops::{CommandError, Daemon, WorkspaceOrigin};
use crate::review::{strip_ansi, tail_lines};

const SERVER_NAME: &str = "bantu";
const MAX_DIFF_CHARS: usize = 30_000;
/// Agents can start workspaces, so cap how many a repo may have at once to
/// keep a runaway orchestrator from spending the user's quota.
pub(crate) const MAX_ACTIVE_WORKSPACES: usize = 10;

impl Daemon {
    /// `--mcp-config` JSON for one session, or `None` before the listener is bound.
    pub fn mcp_config(&self, session_id: &str) -> Option<String> {
        let base = self.listen_base_url()?;
        Some(
            json!({
                "mcpServers": {
                    SERVER_NAME: {
                        "type": "http",
                        "url": format!("{base}/v1/mcp?session={session_id}"),
                        "headers": {"Authorization": "Bearer ${BANTU_HOOK_TOKEN}"}
                    }
                }
            })
            .to_string(),
        )
    }

    /// Handles one JSON-RPC message. `None` for notifications (no reply).
    pub async fn mcp_handle(self: &Arc<Self>, caller: &Session, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned()?;
        let method = msg["method"].as_str().unwrap_or_default();
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Bantu runs coding agents in parallel git worktrees. Use these tools \
                    to split work across workspaces (each gets its own branch and agent), check on \
                    them, wait for them, review their diffs and send follow-ups; to read terminal \
                    output and review comments; and to notify or ask the user."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tool_list()})),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or_default();
                let args = &msg["params"]["arguments"];
                Ok(match self.call_tool(caller, name, args).await {
                    Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
                    Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
                })
            }
            _ => Err(json!({"code": -32601, "message": format!("unknown method {method}")})),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        })
    }

    async fn call_tool(
        self: &Arc<Self>,
        caller: &Session,
        name: &str,
        args: &Value,
    ) -> Result<String, String> {
        let me = self
            .require_workspace(&caller.workspace_id)
            .await
            .map_err(err)?;
        match name {
            "list_workspaces" => {
                let mut lines = Vec::new();
                for ws in self.repo_workspaces(&me.repo_id).await? {
                    let marker = if ws.id == me.id { " (this one)" } else { "" };
                    let (status, title) = self.agent_summary(&ws).await;
                    lines.push(format!(
                        "- {}{marker}: branch {}, {}, agent {status}{}",
                        ws.name,
                        ws.branch,
                        ws.state.as_str(),
                        title.map(|t| format!(", chat \"{t}\"")).unwrap_or_default()
                    ));
                }
                Ok(if lines.is_empty() {
                    "No workspaces.".into()
                } else {
                    lines.join("\n")
                })
            }
            "create_workspace" => {
                let task = args["task"].as_str().unwrap_or_default().trim().to_string();
                if task.is_empty() {
                    return Err("`task` is required: describe what the new agent should do".into());
                }
                let agent = match args["agent"].as_str() {
                    None => AgentKind::Claude,
                    Some(name) => AgentKind::parse(name).ok_or_else(|| {
                        format!(
                            "unknown agent {name:?}; use claude, codex, opencode, gemini or cursor"
                        )
                    })?,
                };
                let name = args["name"].as_str().map(str::to_string);
                if self.repo_workspaces(&me.repo_id).await?.len() >= MAX_ACTIVE_WORKSPACES {
                    return Err(format!(
                        "this repo already has {MAX_ACTIVE_WORKSPACES} active workspaces; \
                         finish or archive some first"
                    ));
                }
                let ws = self
                    .create_workspace_for_tool(&me, name)
                    .await
                    .map_err(err)?;
                let ws = self.wait_ready(&ws.id, Duration::from_secs(300)).await?;
                let session = match agent {
                    AgentKind::Claude => self.first_session(&ws.id).await,
                    _ => None,
                };
                // Children work like their parent: same model (for the same
                // agent) and the same permission mode.
                let model = (agent == caller.agent)
                    .then(|| caller.model.clone())
                    .flatten();
                // Children work like their parent, including how hard it thinks.
                let (effort, fast) = if agent == caller.agent {
                    (caller.effort.clone(), Some(caller.fast))
                } else {
                    (None, None)
                };
                let session = match session {
                    Some(s) => self
                        .update_session(&s.id, None, model, Some(caller.permission_mode))
                        .await
                        .map_err(err)?,
                    None => self
                        .create_session(&ws.id, Some(agent), model, Some(caller.permission_mode))
                        .await
                        .map_err(err)?,
                };
                self.tune_session(&session.id, effort, fast)
                    .await
                    .map_err(err)?;
                self.send_message(&session.id, task).await.map_err(err)?;
                Ok(format!(
                    "Started workspace {} on branch {} with a {} agent working on the task. \
                     Use wait_for_workspace or workspace_status to follow it.",
                    ws.name,
                    ws.branch,
                    agent.as_str()
                ))
            }
            "workspace_status" => {
                let ws = self.find_workspace(&me, args).await?;
                self.status_report(&ws).await
            }
            "wait_for_workspace" => {
                let ws = self.find_workspace(&me, args).await?;
                let secs = args["timeout_seconds"]
                    .as_u64()
                    .unwrap_or(600)
                    .clamp(10, 1800);
                let deadline = Instant::now() + Duration::from_secs(secs);
                loop {
                    let busy = self
                        .store
                        .sessions()
                        .await
                        .map_err(err)?
                        .iter()
                        .any(|s| s.workspace_id == ws.id && s.status == SessionStatus::Running);
                    if !busy {
                        return self.status_report(&ws).await;
                    }
                    if Instant::now() >= deadline {
                        let report = self.status_report(&ws).await?;
                        return Ok(format!("Still working after {secs}s.\n\n{report}"));
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
            "send_message" => {
                let ws = self.find_workspace(&me, args).await?;
                if ws.id == me.id {
                    return Err("that's this workspace; just keep working".into());
                }
                let text = args["text"].as_str().unwrap_or_default().to_string();
                let session = self
                    .first_session(&ws.id)
                    .await
                    .ok_or_else(|| format!("{} has no chat", ws.name))?;
                let s = self.send_message(&session.id, text).await.map_err(err)?;
                Ok(if s.queued > 0 {
                    format!(
                        "Queued for {} (it's busy; {} message(s) waiting).",
                        ws.name, s.queued
                    )
                } else {
                    format!("Sent to {}.", ws.name)
                })
            }
            "get_diff" => {
                let ws = self.find_workspace(&me, args).await?;
                let diff = self.workspace_diff(&ws.id).await.map_err(err)?;
                let mut out = format!(
                    "{} vs {}: {} file(s), {} commit(s) ahead, {} uncommitted\n",
                    ws.name,
                    diff.base_ref,
                    diff.files.len(),
                    diff.ahead,
                    diff.uncommitted
                );
                for f in &diff.files {
                    out.push_str(&format!(
                        "\n--- {} (+{} -{})\n{}\n",
                        f.path, f.additions, f.deletions, f.patch
                    ));
                    if out.len() > MAX_DIFF_CHARS {
                        out.truncate(MAX_DIFF_CHARS);
                        out.push_str("\n… [diff truncated]");
                        break;
                    }
                }
                Ok(out)
            }
            "list_terminals" => {
                let ws = self.tool_workspace(&me, args).await?;
                let lines: Vec<String> = self
                    .terminals()
                    .await
                    .into_iter()
                    .filter(|t| t.workspace_id.as_deref() == Some(ws.id.as_str()))
                    .map(|t| {
                        format!(
                            "- {} ({}, {})",
                            t.title,
                            terminal_kind(t.kind),
                            if t.exited { "exited" } else { "running" }
                        )
                    })
                    .collect();
                Ok(if lines.is_empty() {
                    format!("{} has no terminals open.", ws.name)
                } else {
                    lines.join("\n")
                })
            }
            "get_terminal_output" => {
                let ws = self.tool_workspace(&me, args).await?;
                let terminals: Vec<_> = self
                    .terminals()
                    .await
                    .into_iter()
                    .filter(|t| t.workspace_id.as_deref() == Some(ws.id.as_str()))
                    .collect();
                let wanted = args["terminal"].as_str().unwrap_or_default();
                // By default, the dev server if there is one, else the newest.
                let chosen = if wanted.is_empty() {
                    terminals
                        .iter()
                        .rev()
                        .find(|t| t.kind == TerminalKind::Run && !t.exited)
                        .or(terminals.last())
                } else {
                    terminals
                        .iter()
                        .find(|t| t.title == wanted || t.id == wanted)
                };
                let info = chosen.ok_or_else(|| {
                    format!("no terminal {wanted:?} in {}; see list_terminals", ws.name)
                })?;
                let term = self
                    .terminal(&info.id)
                    .await
                    .ok_or("that terminal just closed")?;
                let (backlog, _) = term.attach();
                let text = strip_ansi(&String::from_utf8_lossy(&backlog));
                let lines = args["lines"].as_u64().unwrap_or(200).clamp(1, 2000) as usize;
                let tail = tail_lines(&text, lines);
                let chars = tail.chars().count();
                let tail: String = tail
                    .chars()
                    .skip(chars.saturating_sub(MAX_DIFF_CHARS))
                    .collect();
                Ok(if tail.trim().is_empty() {
                    format!("{} has printed nothing yet.", info.title)
                } else {
                    format!("{}:\n{tail}", info.title)
                })
            }
            "list_diff_comments" => {
                let ws = self.tool_workspace(&me, args).await?;
                let include_sent = args["include_sent"].as_bool().unwrap_or(false);
                let lines: Vec<String> = self
                    .store
                    .comments()
                    .await
                    .map_err(err)?
                    .into_iter()
                    .filter(|c| c.workspace_id == ws.id && (include_sent || c.sent_at.is_none()))
                    .map(|c| {
                        format!(
                            "- `{}:{}`{}: {}",
                            c.path,
                            c.line,
                            if c.sent_at.is_some() { " (sent)" } else { "" },
                            c.body
                        )
                    })
                    .collect();
                Ok(if lines.is_empty() {
                    format!("{} has no review comments.", ws.name)
                } else {
                    lines.join("\n")
                })
            }
            "add_diff_comment" => {
                let ws = self.tool_workspace(&me, args).await?;
                let path = args["path"].as_str().unwrap_or_default().to_string();
                let line = args["line"]
                    .as_u64()
                    .ok_or("`line` is required: a line number in the current file")?;
                let body = args["body"].as_str().unwrap_or_default().to_string();
                self.add_comment(&ws.id, path, line as u32, body)
                    .await
                    .map_err(err)?;
                Ok(format!(
                    "Added a draft comment to {}. The user decides when to send it.",
                    ws.name
                ))
            }
            "notify_user" => {
                let title = args["title"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if title.is_empty() {
                    return Err("`title` is required".into());
                }
                let body = args["body"].as_str().map(str::to_string);
                self.notify_user(caller, title, body).await;
                Ok("The user was notified.".into())
            }
            "ask_user" => {
                let question = args["question"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if question.is_empty() {
                    return Err("`question` is required".into());
                }
                let options = args["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|o| o.as_str().map(str::to_string))
                    .collect();
                let secs = args["timeout_seconds"]
                    .as_u64()
                    .unwrap_or(600)
                    .clamp(10, 1800);
                let answer = self
                    .ask_user(caller, question, options, Duration::from_secs(secs))
                    .await;
                Ok(match answer {
                    Some(Answer::Text(text)) => format!("The user answered: {text}"),
                    Some(Answer::Dismissed) => {
                        "The user dismissed the question without answering. Use your judgment, \
                         or stop if it's not safe to continue."
                            .into()
                    }
                    Some(Answer::Stopped) => "The user stopped this agent.".into(),
                    None => format!(
                        "Nobody answered within {secs} seconds. Use your judgment, or stop if \
                         it's not safe to continue."
                    ),
                })
            }
            "integrate_workspaces" => {
                // The caller is mid-turn and reads the result, so it handles
                // any conflict itself.
                let result = self
                    .integrate_children(&me.id, None, Some(&caller.id))
                    .await;
                match result {
                    Ok(result) => Ok(crate::orchestrate::describe(&result)),
                    Err(CommandError::Conflict(reason)) => Err(reason),
                    Err(e) => Err(err(e)),
                }
            }
            other => Err(format!("unknown tool {other}")),
        }
    }

    /// The workspace named in `args`, or the caller's own.
    async fn tool_workspace(&self, me: &Workspace, args: &Value) -> Result<Workspace, String> {
        if args["workspace"].as_str().unwrap_or_default().is_empty() {
            Ok(me.clone())
        } else {
            self.find_workspace(me, args).await
        }
    }

    async fn create_workspace_for_tool(
        self: &Arc<Self>,
        me: &Workspace,
        name: Option<String>,
    ) -> Result<Workspace, CommandError> {
        self.create_workspace(
            &me.repo_id,
            name,
            None,
            None,
            WorkspaceOrigin {
                parent_id: Some(me.id.clone()),
                loop_id: None,
            },
        )
        .await
    }

    pub(crate) async fn wait_ready(
        &self,
        id: &str,
        timeout: Duration,
    ) -> Result<Workspace, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let ws = self.require_workspace(id).await.map_err(err)?;
            match ws.state {
                WorkspaceState::Ready => {
                    // The first chat is created right after the workspace turns ready.
                    if self.first_session(&ws.id).await.is_some() || Instant::now() >= deadline {
                        return Ok(ws);
                    }
                }
                WorkspaceState::Failed => {
                    return Err(format!(
                        "workspace {} failed: {}",
                        ws.name,
                        ws.error.unwrap_or_default()
                    ))
                }
                _ if Instant::now() >= deadline => {
                    return Err(format!("workspace {} is still setting up", ws.name))
                }
                _ => {}
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }

    async fn repo_workspaces(&self, repo_id: &str) -> Result<Vec<Workspace>, String> {
        Ok(self
            .store
            .workspaces()
            .await
            .map_err(err)?
            .into_iter()
            .filter(|w| w.repo_id == repo_id && w.state != WorkspaceState::Archived)
            .collect())
    }

    async fn find_workspace(&self, me: &Workspace, args: &Value) -> Result<Workspace, String> {
        let key = args["workspace"].as_str().unwrap_or_default();
        if key.is_empty() {
            return Err("`workspace` is required (a name from list_workspaces)".into());
        }
        self.repo_workspaces(&me.repo_id)
            .await?
            .into_iter()
            .find(|w| w.name == key || w.id == key || w.branch == key)
            .ok_or_else(|| format!("no workspace {key:?} in this repo; see list_workspaces"))
    }

    pub(crate) async fn first_session(&self, workspace_id: &str) -> Option<Session> {
        self.store
            .sessions()
            .await
            .ok()?
            .into_iter()
            .find(|s| s.workspace_id == workspace_id)
    }

    async fn agent_summary(&self, ws: &Workspace) -> (&'static str, Option<String>) {
        let sessions: Vec<Session> = self
            .store
            .sessions()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.workspace_id == ws.id)
            .collect();
        let status = if sessions.iter().any(|s| s.status == SessionStatus::Running) {
            "working"
        } else {
            "idle"
        };
        let title = sessions
            .iter()
            .rev()
            .map(|s| s.title.clone())
            .find(|t| t != "New chat");
        (status, title)
    }

    async fn status_report(&self, ws: &Workspace) -> Result<String, String> {
        let (status, title) = self.agent_summary(ws).await;
        let mut out = format!(
            "{}: branch {}, workspace {}, agent {status}{}\n",
            ws.name,
            ws.branch,
            ws.state.as_str(),
            title.map(|t| format!(", chat \"{t}\"")).unwrap_or_default()
        );
        if self
            .pending_permissions()
            .await
            .iter()
            .any(|p| p.workspace_id == ws.id)
        {
            out.push_str("Waiting for the user to approve an action.\n");
        }
        if let Ok(diff) = self.workspace_diff(&ws.id).await {
            let (adds, dels) = diff
                .files
                .iter()
                .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
            out.push_str(&format!(
                "Changes: {} file(s), +{adds} -{dels}, {} commit(s) ahead of {}, {} uncommitted\n",
                diff.files.len(),
                diff.ahead,
                diff.base_ref,
                diff.uncommitted
            ));
        }
        if let Some(text) = self.last_answer(ws).await {
            out.push_str(&format!("\nLatest reply from its agent:\n{text}\n"));
        }
        Ok(out)
    }

    pub(crate) async fn last_answer(&self, ws: &Workspace) -> Option<String> {
        let events = self.store.workspace_history(&ws.id, 500).await.ok()?;
        events.iter().rev().find_map(|e| match &e.data {
            EventData::SessionItem {
                item: ChatItem::Assistant { text },
                ..
            } => Some(text.chars().take(2_000).collect()),
            _ => None,
        })
    }
}

fn terminal_kind(kind: TerminalKind) -> &'static str {
    match kind {
        TerminalKind::Shell => "shell",
        TerminalKind::Run => "run script",
        TerminalKind::Login => "sign-in",
        TerminalKind::Agent => "CLI",
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn tool_list() -> Value {
    let workspace =
        json!({"type": "string", "description": "Workspace name (from list_workspaces)"});
    json!([
        {
            "name": "list_workspaces",
            "description": "List this repository's Bantu workspaces with their branch, state and whether their agent is working.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "create_workspace",
            "description": "Start a new workspace (its own git worktree and branch) with an agent working on `task`. Use it to split work into independent parallel pieces; give each a complete, self-contained task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "Everything the new agent needs to know to do the work"},
                    "agent": {"type": "string", "enum": ["claude", "codex", "opencode", "gemini", "cursor"], "description": "Defaults to claude"},
                    "name": {"type": "string", "description": "Optional workspace name (lowercase, dashes)"}
                },
                "required": ["task"]
            }
        },
        {
            "name": "workspace_status",
            "description": "Show a workspace's state, its agent's latest reply, and a summary of its changes.",
            "inputSchema": {"type": "object", "properties": {"workspace": workspace}, "required": ["workspace"]}
        },
        {
            "name": "wait_for_workspace",
            "description": "Wait until a workspace's agent finishes (or the timeout passes), then report its status.",
            "inputSchema": {
                "type": "object",
                "properties": {"workspace": workspace, "timeout_seconds": {"type": "integer", "description": "10-1800, default 600"}},
                "required": ["workspace"]
            }
        },
        {
            "name": "send_message",
            "description": "Send a follow-up message to another workspace's agent (queued if it's busy).",
            "inputSchema": {
                "type": "object",
                "properties": {"workspace": workspace, "text": {"type": "string"}},
                "required": ["workspace", "text"]
            }
        },
        {
            "name": "integrate_workspaces",
            "description": "Merge the workspaces you started into this workspace's branch, locally. Children must have committed their work and finished. On a conflict that merge is aborted and you're told which one, so you can merge it yourself. Commit your own work first.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "list_terminals",
            "description": "List the terminals open in a workspace (this one by default), such as its dev server.",
            "inputSchema": {"type": "object", "properties": {"workspace": workspace}}
        },
        {
            "name": "get_terminal_output",
            "description": "Read recent output from a terminal, such as dev server logs. Defaults to this workspace's run script, or its newest terminal.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace": workspace,
                    "terminal": {"type": "string", "description": "A terminal title from list_terminals"},
                    "lines": {"type": "integer", "description": "How many lines from the end, 1-2000, default 200"}
                }
            }
        },
        {
            "name": "list_diff_comments",
            "description": "List the user's review comments on a workspace's changes (this one by default). Drafts only unless include_sent is true.",
            "inputSchema": {
                "type": "object",
                "properties": {"workspace": workspace, "include_sent": {"type": "boolean"}}
            }
        },
        {
            "name": "add_diff_comment",
            "description": "Add a draft review comment on a line of a workspace's changes. The user sees it in the Changes tab and decides whether to send it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace": workspace,
                    "path": {"type": "string", "description": "File path relative to the repo root"},
                    "line": {"type": "integer", "description": "Line number in the current file"},
                    "body": {"type": "string"}
                },
                "required": ["path", "line", "body"]
            }
        },
        {
            "name": "notify_user",
            "description": "Send the user a notice without waiting, e.g. a long task finished or something needs a look later.",
            "inputSchema": {
                "type": "object",
                "properties": {"title": {"type": "string"}, "body": {"type": "string"}},
                "required": ["title"]
            }
        },
        {
            "name": "ask_user",
            "description": "Ask the user a question and wait for the answer. Use it for decisions you can't make yourself; it also reaches them when they're away.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": {"type": "string"},
                    "options": {"type": "array", "items": {"type": "string"}, "description": "Suggested answers; they can also type their own"},
                    "timeout_seconds": {"type": "integer", "description": "10-1800, default 600"}
                },
                "required": ["question"]
            }
        },
        {
            "name": "get_diff",
            "description": "Get a workspace's changes against its base branch as unified diffs (including uncommitted work).",
            "inputSchema": {"type": "object", "properties": {"workspace": workspace}, "required": ["workspace"]}
        }
    ])
}
