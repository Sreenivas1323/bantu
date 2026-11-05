//! Agent Client Protocol driver for OpenCode, Gemini CLI and Cursor.
//!
//! ACP is JSON-RPC 2.0 over stdio with one JSON message per line. Like the
//! other drivers this runs one process per turn: `initialize`, then
//! `session/new` or `session/load`, then `session/prompt`. The conversation
//! lives with the agent, and the daemon keeps only its session id.
//!
//! Bantu doesn't advertise the `fs` or `terminal` client capabilities, so
//! agents use their own file and shell tools inside the worktree.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use bantu_protocol::{AgentKind, ChatItem, PermissionMode, Session, StreamDelta};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Notify};

use crate::ops::Daemon;
use crate::permissions::Decision;
use crate::sessions::{truncate, workspace_context};
use crate::Config;

const PROTOCOL_VERSION: u32 = 1;
const CANCEL_GRACE: Duration = Duration::from_secs(5);
/// An agent that isn't signed in may wait forever without answering.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);
/// How much of the transcript to replay to an agent that can't load sessions.
const REPLAY_CHARS: usize = 20_000;

/// The binary and arguments that start an agent in ACP mode.
pub fn command(config: &Config, agent: AgentKind) -> Option<(PathBuf, &'static [&'static str])> {
    match agent {
        AgentKind::Opencode => Some((config.opencode_bin.clone(), &["acp"])),
        AgentKind::Gemini => Some((config.gemini_bin.clone(), &["--acp"])),
        AgentKind::Cursor => Some((config.cursor_bin.clone(), &["acp"])),
        AgentKind::Claude | AgentKind::Codex => None,
    }
}

fn request(id: u64, method: &str, params: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

fn response(id: &Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn error_response(id: &Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

/// The agent mode that best matches a Bantu permission mode. Mode ids aren't
/// standardized, so this only picks from what the agent advertises.
fn mode_for(permission_mode: PermissionMode, modes: &Value) -> Option<String> {
    let wanted: &[&str] = match permission_mode {
        PermissionMode::Plan => &["plan"],
        PermissionMode::AcceptEdits => &["acceptEdits", "autoEdit", "auto_edit"],
        PermissionMode::BypassPermissions => &["yolo", "bypassPermissions"],
        PermissionMode::Auto | PermissionMode::Ask => &["default"],
    };
    let current = modes["currentModeId"].as_str();
    modes["availableModes"]
        .as_array()?
        .iter()
        .filter_map(|m| m["id"].as_str())
        .find(|id| wanted.iter().any(|w| id.eq_ignore_ascii_case(w)) && Some(*id) != current)
        .map(str::to_string)
}

/// What the permission mode decides without asking the user.
fn auto_decision(mode: PermissionMode, kind: &str) -> Option<bool> {
    match mode {
        PermissionMode::BypassPermissions => Some(true),
        PermissionMode::Plan => Some(matches!(kind, "read" | "search" | "fetch" | "think")),
        PermissionMode::AcceptEdits if kind == "edit" => Some(true),
        PermissionMode::AcceptEdits | PermissionMode::Auto | PermissionMode::Ask => None,
    }
}

/// Picks the matching one-time option, falling back to the "always" variant.
/// Without a matching option the request is reported as cancelled.
fn permission_outcome(options: &Value, allow: bool) -> Value {
    let kinds: [&str; 2] = if allow {
        ["allow_once", "allow_always"]
    } else {
        ["reject_once", "reject_always"]
    };
    let option = kinds.iter().find_map(|kind| {
        options
            .as_array()?
            .iter()
            .find(|o| o["kind"] == *kind)?
            .get("optionId")?
            .as_str()
    });
    match option {
        Some(id) => json!({"outcome": {"outcome": "selected", "optionId": id}}),
        None => json!({"outcome": {"outcome": "cancelled"}}),
    }
}

/// A tool card named like Claude's tools, so the UI's summaries (command,
/// file path, query) work for every agent.
fn tool_use(update: &Value) -> ChatItem {
    let raw = &update["rawInput"];
    let title = update["title"].as_str().unwrap_or("Tool");
    let location = update["locations"][0]["path"].as_str();
    let mut input = if raw.is_object() {
        raw.clone()
    } else {
        json!({})
    };
    let name = match update["kind"].as_str().unwrap_or("other") {
        "execute" => {
            if input["command"].is_null() {
                input["command"] = json!(title);
            }
            "Bash"
        }
        "edit" | "delete" | "move" => {
            if let (Some(path), true) = (location, input["file_path"].is_null()) {
                input["file_path"] = json!(path);
            }
            "Edit"
        }
        "read" => {
            if let (Some(path), true) = (location, input["file_path"].is_null()) {
                input["file_path"] = json!(path);
            }
            "Read"
        }
        "search" => "Grep",
        "fetch" => "WebFetch",
        _ => title,
    };
    input["title"] = json!(title);
    ChatItem::ToolUse {
        id: update["toolCallId"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        name: name.to_string(),
        input,
    }
}

/// The readable part of a finished tool call.
fn tool_output(update: &Value) -> String {
    let mut parts = Vec::new();
    for item in update["content"].as_array().into_iter().flatten() {
        match item["type"].as_str() {
            Some("content") => {
                if let Some(text) = item["content"]["text"].as_str() {
                    parts.push(text.to_string());
                }
            }
            Some("diff") => parts.push(format!(
                "edited {}",
                item["path"].as_str().unwrap_or("a file")
            )),
            _ => {}
        }
    }
    if parts.is_empty() {
        return match &update["rawOutput"] {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
    }
    parts.join("\n")
}

/// Normalizes `session/update` notifications into transcript items. Text
/// arrives in chunks, so it's held until the kind of update changes.
#[derive(Default)]
struct Updates {
    text: String,
    thought: String,
    /// Tool calls already shown, so a repeated start isn't drawn twice.
    tools: HashMap<String, bool>,
    cost_usd: Option<f64>,
    plans: u32,
}

impl Updates {
    fn flush(&mut self) -> Vec<ChatItem> {
        let mut out = Vec::new();
        let thought = std::mem::take(&mut self.thought);
        if !thought.trim().is_empty() {
            out.push(ChatItem::Thinking { text: thought });
        }
        let text = std::mem::take(&mut self.text);
        if !text.trim().is_empty() {
            out.push(ChatItem::Assistant { text });
        }
        out
    }

    /// Returns finished items and, for message text, the fragment to stream.
    fn feed(&mut self, update: &Value) -> (Vec<ChatItem>, Option<String>) {
        match update["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => {
                let mut out = Vec::new();
                if !self.thought.is_empty() {
                    out = self.flush();
                }
                let chunk = update["content"]["text"].as_str().unwrap_or_default();
                self.text.push_str(chunk);
                (out, (!chunk.is_empty()).then(|| chunk.to_string()))
            }
            "agent_thought_chunk" => {
                let mut out = Vec::new();
                if !self.text.is_empty() {
                    out = self.flush();
                }
                self.thought
                    .push_str(update["content"]["text"].as_str().unwrap_or_default());
                (out, None)
            }
            "tool_call" | "tool_call_update" => {
                let mut out = self.flush();
                let id = update["toolCallId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                if let std::collections::hash_map::Entry::Vacant(e) = self.tools.entry(id) {
                    e.insert(false);
                    out.push(tool_use(update));
                }
                out.extend(self.finish_tool(update));
                (out, None)
            }
            // Same shape as Claude's todo list, so the Checks tab can read it.
            "plan" => {
                let mut out = self.flush();
                self.plans += 1;
                let id = format!("plan-{}", self.plans);
                out.push(ChatItem::ToolUse {
                    id: id.clone(),
                    name: "TodoWrite".into(),
                    input: json!({"todos": update["entries"]}),
                });
                out.push(ChatItem::ToolResult {
                    tool_use_id: id,
                    content: String::new(),
                    is_error: false,
                });
                (out, None)
            }
            "usage_update" => {
                let cost = &update["cost"];
                if cost["currency"] == "USD" {
                    self.cost_usd = cost["amount"].as_f64().or(self.cost_usd);
                }
                (vec![], None)
            }
            _ => (vec![], None),
        }
    }

    fn finish_tool(&mut self, update: &Value) -> Option<ChatItem> {
        let status = update["status"].as_str().unwrap_or_default();
        if status != "completed" && status != "failed" {
            return None;
        }
        let id = update["toolCallId"].as_str().unwrap_or_default();
        let done = self.tools.get_mut(id)?;
        if *done {
            return None;
        }
        *done = true;
        Some(ChatItem::ToolResult {
            tool_use_id: id.to_string(),
            content: truncate(tool_output(update)),
            is_error: status == "failed",
        })
    }
}

/// Where the handshake is. Each stage waits for one response.
enum Stage {
    Initialize,
    /// Waiting for `session/load` (true) or `session/new` (false).
    Open {
        loading: bool,
    },
    /// Optional mode and model changes, then the prompt.
    Configure,
    Prompt,
}

const ID_INITIALIZE: u64 = 1;
const ID_OPEN: u64 = 2;
const ID_CONFIGURE: u64 = 3;
const ID_PROMPT: u64 = 4;

fn friendly_failure(agent: AgentKind, error: &Value) -> anyhow::Error {
    let message = error["message"].as_str().unwrap_or("request failed");
    // -32000 is ACP's "authentication required".
    if error["code"] == -32000 || message.to_ascii_lowercase().contains("auth") {
        anyhow!(
            "{} isn't signed in ({message}). Sign in from Agent accounts.",
            agent.as_str()
        )
    } else {
        anyhow!("{} failed: {message}", agent.as_str())
    }
}

impl Daemon {
    pub(crate) async fn run_acp_turn(
        self: &Arc<Self>,
        s: &Session,
        agent_id: &str,
        turn: u32,
        text: &str,
        stop: &Notify,
    ) -> anyhow::Result<f64> {
        let ws = self.require_workspace(&s.workspace_id).await?;
        let (bin, args) = command(&self.config, s.agent).context("not an ACP agent")?;
        let resume = self.can_resume(&s.id).await?;
        let mut child = tokio::process::Command::new(crate::platform::program(&bin))
            .args(args)
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
        let mut stdin = child.stdin.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        });
        let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();

        // The token goes over stdin, so it never appears in `ps`.
        let mcp_servers = |http: bool| -> Value {
            match (http, self.listen_base_url()) {
                (true, Some(base)) => json!([{
                    "type": "http",
                    "name": "bantu",
                    "url": format!("{base}/v1/mcp?session={}", s.id),
                    "headers": [{"name": "Authorization", "value": format!("Bearer {}", self.hook_token())}],
                }]),
                _ => json!([]),
            }
        };
        let send = |line: String| line + "\n";

        // Permission answers come from spawned tasks so the turn keeps
        // reading output while a request waits for the user.
        let (reply_tx, mut reply_rx) = mpsc::channel::<String>(16);
        let mut stage = Stage::Initialize;
        let mut acp_session: Option<String> = None;
        let mut configure: VecDeque<(String, Value)> = VecDeque::new();
        let mut updates = Updates::default();
        let mut http_mcp = false;
        let mut prompt_text = text.to_string();
        let mut first_contact = !resume;
        let handshake = tokio::time::sleep(HANDSHAKE_TIMEOUT);
        tokio::pin!(handshake);

        stdin
            .write_all(
                send(request(
                    ID_INITIALIZE,
                    "initialize",
                    json!({
                        "protocolVersion": PROTOCOL_VERSION,
                        "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                        "clientInfo": {"name": "bantu", "version": env!("CARGO_PKG_VERSION")},
                    }),
                ))
                .as_bytes(),
            )
            .await?;
        stdin.flush().await?;

        loop {
            let mut outgoing: Vec<String> = Vec::new();
            tokio::select! {
                line = lines.next_line() => {
                    let Some(line) = line? else {
                        for item in updates.flush() {
                            self.store.session_item(s, turn, item).await?;
                        }
                        let status = child.wait().await?;
                        let tail = stderr_tail(stderr_task.await.unwrap_or_default());
                        bail!("{} exited ({status}) without finishing the turn. {tail}", s.agent.as_str());
                    };
                    let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };

                    if let Some(method) = msg["method"].as_str() {
                        if !msg["id"].is_null() {
                            self.acp_request(s, method, &msg, &reply_tx, &mut outgoing);
                        } else if method == "session/update" && matches!(stage, Stage::Prompt) {
                            let (items, delta) = updates.feed(&msg["params"]["update"]);
                            for item in items {
                                self.store.session_item(s, turn, item).await?;
                            }
                            if let Some(text) = delta {
                                let _ = self.running.deltas.send(StreamDelta {
                                    session_id: s.id.clone(),
                                    workspace_id: s.workspace_id.clone(),
                                    turn,
                                    text,
                                });
                            }
                        }
                    } else {
                        let id = msg["id"].as_u64().unwrap_or(0);
                        let failed = !msg["error"].is_null();
                        match (&stage, id) {
                            (Stage::Initialize, ID_INITIALIZE) => {
                                if failed {
                                    return Err(friendly_failure(s.agent, &msg["error"]));
                                }
                                let caps = &msg["result"]["agentCapabilities"];
                                http_mcp = caps["mcpCapabilities"]["http"].as_bool().unwrap_or(false);
                                let can_load = caps["loadSession"].as_bool().unwrap_or(false);
                                if resume && can_load {
                                    stage = Stage::Open { loading: true };
                                    outgoing.push(request(ID_OPEN, "session/load", json!({
                                        "sessionId": agent_id,
                                        "cwd": ws.path,
                                        "mcpServers": mcp_servers(http_mcp),
                                    })));
                                } else {
                                    if resume {
                                        first_contact = true;
                                    }
                                    stage = Stage::Open { loading: false };
                                    outgoing.push(request(ID_OPEN, "session/new", json!({
                                        "cwd": ws.path,
                                        "mcpServers": mcp_servers(http_mcp),
                                    })));
                                }
                            }
                            (Stage::Open { loading }, ID_OPEN) => {
                                if failed && *loading {
                                    // The agent lost the conversation: start over,
                                    // with the transcript as context.
                                    first_contact = true;
                                    stage = Stage::Open { loading: false };
                                    outgoing.push(request(ID_OPEN, "session/new", json!({
                                        "cwd": ws.path,
                                        "mcpServers": mcp_servers(http_mcp),
                                    })));
                                } else if failed {
                                    return Err(friendly_failure(s.agent, &msg["error"]));
                                } else {
                                    let result = &msg["result"];
                                    let id = match result["sessionId"].as_str() {
                                        Some(id) => {
                                            if id != agent_id {
                                                self.store.set_agent_session_id(&s.id, id).await?;
                                            }
                                            id.to_string()
                                        }
                                        // `session/load` answers with `{}`.
                                        None if *loading => agent_id.to_string(),
                                        None => bail!("{} didn't return a session id", s.agent.as_str()),
                                    };
                                    if first_contact {
                                        prompt_text = self
                                            .acp_first_prompt(s, resume, &workspace_context(&ws), text)
                                            .await;
                                    }
                                    if let Some(mode) = mode_for(s.permission_mode, &result["modes"]) {
                                        configure.push_back(("session/set_mode".into(), json!({"sessionId": id, "modeId": mode})));
                                    }
                                    if let (Some(model), true) = (&s.model, !result["models"].is_null()) {
                                        configure.push_back(("session/set_model".into(), json!({"sessionId": id, "modelId": model})));
                                    }
                                    acp_session = Some(id);
                                    stage = Stage::Configure;
                                    outgoing.push(next_setup(&mut configure, &mut stage, acp_session.as_deref(), &prompt_text));
                                }
                            }
                            // A mode or model the agent refuses shouldn't stop the turn.
                            (Stage::Configure, ID_CONFIGURE) => {
                                outgoing.push(next_setup(&mut configure, &mut stage, acp_session.as_deref(), &prompt_text));
                            }
                            (Stage::Prompt, ID_PROMPT) => {
                                for item in updates.flush() {
                                    self.store.session_item(s, turn, item).await?;
                                }
                                if failed {
                                    return Err(friendly_failure(s.agent, &msg["error"]));
                                }
                                let result = &msg["result"];
                                let usage = &result["usage"];
                                let stop_reason = result["stopReason"].as_str().unwrap_or("end_turn");
                                let cost = updates.cost_usd;
                                if stop_reason == "refusal" {
                                    self.store.session_item(s, turn, ChatItem::Error {
                                        message: format!("{} refused the request.", s.agent.as_str()),
                                    }).await?;
                                }
                                self.store.session_item(s, turn, ChatItem::TurnEnd {
                                    is_error: stop_reason == "refusal",
                                    duration_ms: None,
                                    cost_usd: cost,
                                    input_tokens: usage["inputTokens"].as_u64(),
                                    output_tokens: usage["outputTokens"].as_u64(),
                                }).await?;
                                let _ = child.start_kill();
                                let _ = child.wait().await;
                                return Ok(cost.unwrap_or(0.0));
                            }
                            _ => {}
                        }
                    }
                }
                Some(line) = reply_rx.recv() => outgoing.push(line),
                _ = &mut handshake, if !matches!(stage, Stage::Prompt) => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    let tail = stderr_tail(stderr_task.await.unwrap_or_default());
                    bail!(
                        "{} didn't respond within {} seconds; it may need signing in. {tail}",
                        s.agent.as_str(),
                        HANDSHAKE_TIMEOUT.as_secs()
                    );
                }
                _ = stop.notified() => {
                    if let Some(id) = &acp_session {
                        let cancel = json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": id}});
                        let _ = stdin.write_all(send(cancel.to_string()).as_bytes()).await;
                        let _ = stdin.flush().await;
                    }
                    // Give the agent a moment to wind down, then insist.
                    let _ = tokio::time::timeout(CANCEL_GRACE, async {
                        while let Ok(Some(_)) = lines.next_line().await {}
                    }).await;
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    for item in updates.flush() {
                        let _ = self.store.session_item(s, turn, item).await;
                    }
                    return Err(anyhow!("Stopped"));
                }
            }
            for line in outgoing {
                stdin.write_all(send(line).as_bytes()).await?;
            }
            stdin.flush().await?;
        }
    }

    /// Answers a request from the agent. Only permission prompts are
    /// supported, because Bantu advertises no other client capabilities.
    fn acp_request(
        self: &Arc<Self>,
        s: &Session,
        method: &str,
        msg: &Value,
        reply: &mpsc::Sender<String>,
        outgoing: &mut Vec<String>,
    ) {
        let id = msg["id"].clone();
        if method != "session/request_permission" {
            outgoing.push(error_response(&id, -32601, "not supported by Bantu"));
            return;
        }
        let params = msg["params"].clone();
        let call = &params["toolCall"];
        let kind = call["kind"].as_str().unwrap_or("other").to_string();
        if let Some(allow) = auto_decision(s.permission_mode, &kind) {
            outgoing.push(response(&id, permission_outcome(&params["options"], allow)));
            return;
        }
        let (tool_name, tool_input) = match tool_use(call) {
            ChatItem::ToolUse { name, input, .. } => (name, input),
            _ => ("tool".to_string(), json!({})),
        };
        let this = self.clone();
        let session = s.clone();
        let reply = reply.clone();
        tokio::spawn(async move {
            let Decision { allow, .. } = this
                .wait_for_permission(&session, tool_name, tool_input)
                .await;
            let _ = reply
                .send(response(&id, permission_outcome(&params["options"], allow)))
                .await;
        });
    }

    /// ACP has no system prompt flag, so the first prompt carries the workspace
    /// briefing. An agent that lost the conversation also gets the transcript.
    async fn acp_first_prompt(
        &self,
        s: &Session,
        resuming: bool,
        briefing: &str,
        text: &str,
    ) -> String {
        if !resuming {
            return format!("{briefing}\n\n{text}");
        }
        // The newest message is the one being sent now.
        let earlier = self.earlier_conversation(s, true).await;
        format!(
            "{briefing}\n\n[Bantu] This agent couldn't reopen the earlier conversation. Here's \
             what was said:\n\n{earlier}\n\n[Bantu] The new message:\n\n{text}"
        )
    }

    /// The last part of a session's user and assistant messages.
    pub(crate) async fn earlier_conversation(&self, s: &Session, skip_newest: bool) -> String {
        let items = self.store.session_items(&s.id).await.unwrap_or_default();
        let mut lines: Vec<String> = items
            .into_iter()
            .filter_map(|(_, item)| match item {
                ChatItem::User { text } => Some(format!("User: {text}")),
                ChatItem::Assistant { text } => Some(format!("Assistant: {text}")),
                _ => None,
            })
            .collect();
        if skip_newest {
            lines.pop();
        }
        let joined = lines.join("\n\n");
        let count = joined.chars().count();
        joined
            .chars()
            .skip(count.saturating_sub(REPLAY_CHARS))
            .collect()
    }
}

fn stderr_tail(stderr: String) -> String {
    let chars: Vec<char> = stderr.trim().chars().collect();
    chars[chars.len().saturating_sub(2000)..].iter().collect()
}

/// The next setup request, or the prompt once setup is done.
fn next_setup(
    configure: &mut VecDeque<(String, Value)>,
    stage: &mut Stage,
    session: Option<&str>,
    prompt: &str,
) -> String {
    if let Some((method, params)) = configure.pop_front() {
        return request(ID_CONFIGURE, &method, params);
    }
    *stage = Stage::Prompt;
    request(
        ID_PROMPT,
        "session/prompt",
        json!({
            "sessionId": session,
            "prompt": [{"type": "text", "text": prompt}],
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_chunks_and_maps_tools() {
        let mut u = Updates::default();
        let (items, delta) = u.feed(&json!({"sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": "hmm"}}));
        assert!(items.is_empty());
        assert_eq!(delta, None);
        let (items, delta) = u.feed(&json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Hel"}}));
        assert_eq!(items, vec![ChatItem::Thinking { text: "hmm".into() }]);
        assert_eq!(delta.as_deref(), Some("Hel"));
        u.feed(&json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "lo"}}));

        let (items, _) = u.feed(&json!({"sessionUpdate": "tool_call", "toolCallId": "c1",
            "title": "Run ls", "kind": "execute", "status": "pending"}));
        assert_eq!(
            items[0],
            ChatItem::Assistant {
                text: "Hello".into()
            }
        );
        assert!(matches!(&items[1], ChatItem::ToolUse { name, input, .. }
            if name == "Bash" && input["command"] == "Run ls"));

        let (items, _) = u.feed(
            &json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1",
            "status": "completed", "content": [{"type": "content",
            "content": {"type": "text", "text": "a.txt"}}]}),
        );
        assert_eq!(
            items,
            vec![ChatItem::ToolResult {
                tool_use_id: "c1".into(),
                content: "a.txt".into(),
                is_error: false
            }]
        );
        // A repeated completion isn't reported twice.
        let (items, _) = u.feed(
            &json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1",
            "status": "completed"}),
        );
        assert!(items.is_empty());
    }

    #[test]
    fn reads_edit_locations_diffs_and_usage() {
        let mut u = Updates::default();
        let (items, _) = u.feed(&json!({"sessionUpdate": "tool_call", "toolCallId": "e1",
            "title": "Edit a.rs", "kind": "edit", "status": "failed",
            "locations": [{"path": "/w/a.rs"}],
            "content": [{"type": "diff", "path": "/w/a.rs", "newText": "x"}]}));
        assert!(matches!(&items[0], ChatItem::ToolUse { name, input, .. }
            if name == "Edit" && input["file_path"] == "/w/a.rs"));
        assert!(
            matches!(&items[1], ChatItem::ToolResult { content, is_error: true, .. }
            if content == "edited /w/a.rs")
        );
        u.feed(
            &json!({"sessionUpdate": "usage_update", "used": 5, "size": 9,
            "cost": {"amount": 0.5, "currency": "USD"}}),
        );
        assert_eq!(u.cost_usd, Some(0.5));
        u.feed(
            &json!({"sessionUpdate": "usage_update", "used": 6, "size": 9,
            "cost": {"amount": 70.0, "currency": "JPY"}}),
        );
        assert_eq!(u.cost_usd, Some(0.5));
    }

    #[test]
    fn answers_permissions_by_mode() {
        use PermissionMode::*;
        assert_eq!(auto_decision(BypassPermissions, "execute"), Some(true));
        assert_eq!(auto_decision(Plan, "read"), Some(true));
        assert_eq!(auto_decision(Plan, "edit"), Some(false));
        assert_eq!(auto_decision(AcceptEdits, "edit"), Some(true));
        assert_eq!(auto_decision(AcceptEdits, "execute"), None);
        assert_eq!(auto_decision(Ask, "read"), None);

        let options = json!([
            {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            {"optionId": "always", "name": "Always", "kind": "allow_always"},
            {"optionId": "no", "name": "Reject", "kind": "reject_once"},
        ]);
        assert_eq!(
            permission_outcome(&options, true),
            json!({"outcome": {"outcome": "selected", "optionId": "yes"}})
        );
        assert_eq!(
            permission_outcome(&options, false),
            json!({"outcome": {"outcome": "selected", "optionId": "no"}})
        );
        assert_eq!(
            permission_outcome(&json!([]), true),
            json!({"outcome": {"outcome": "cancelled"}})
        );
    }

    #[test]
    fn picks_modes_the_agent_offers() {
        let modes = json!({"currentModeId": "default", "availableModes": [
            {"id": "default"}, {"id": "autoEdit"}, {"id": "yolo"}, {"id": "plan"}]});
        assert_eq!(
            mode_for(PermissionMode::Plan, &modes).as_deref(),
            Some("plan")
        );
        assert_eq!(
            mode_for(PermissionMode::AcceptEdits, &modes).as_deref(),
            Some("autoEdit")
        );
        assert_eq!(mode_for(PermissionMode::Auto, &modes), None);
        assert_eq!(mode_for(PermissionMode::Plan, &Value::Null), None);
    }
}
