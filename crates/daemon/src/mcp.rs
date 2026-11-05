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
    AgentKind, ChatItem, EventData, Session, SessionStatus, Workspace, WorkspaceState,
};
use serde_json::{json, Value};

use crate::ops::{CommandError, Daemon};

const SERVER_NAME: &str = "bantu";
const MAX_DIFF_CHARS: usize = 30_000;
/// Agents can start workspaces, so cap how many a repo may have at once to
/// keep a runaway orchestrator from spending the user's quota.
const MAX_ACTIVE_WORKSPACES: usize = 10;

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
                    them, wait for them, review their diffs and send follow-ups."
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
            other => Err(format!("unknown tool {other}")),
        }
    }

    async fn create_workspace_for_tool(
        self: &Arc<Self>,
        me: &Workspace,
        name: Option<String>,
    ) -> Result<Workspace, CommandError> {
        let value = self
            .execute(bantu_protocol::Command::WorkspaceCreate {
                repo_id: me.repo_id.clone(),
                name,
                base_ref: None,
                from: None,
            })
            .await?;
        serde_json::from_value(value).map_err(|e| CommandError::Internal(e.into()))
    }

    async fn wait_ready(&self, id: &str, timeout: Duration) -> Result<Workspace, String> {
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

    async fn first_session(&self, workspace_id: &str) -> Option<Session> {
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

    async fn last_answer(&self, ws: &Workspace) -> Option<String> {
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
            "name": "get_diff",
            "description": "Get a workspace's changes against its base branch as unified diffs (including uncommitted work).",
            "inputSchema": {"type": "object", "properties": {"workspace": workspace}, "required": ["workspace"]}
        }
    ])
}
