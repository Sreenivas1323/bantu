//! OpenAI Codex driver: one `codex exec --json` process per turn, resumed
//! with `codex exec resume <thread_id>`.
//!
//! Codex reports work as items (`agent_message`, `command_execution`,
//! `file_change`, …) that start and complete; they're mapped onto the same
//! [`ChatItem`]s Claude produces so the UI doesn't care which agent ran.

use std::collections::HashSet;
use std::time::Instant;

use bantu_protocol::{ChatItem, PermissionMode};
use serde_json::{json, Value};

/// Arguments for one turn. The prompt is read from stdin (`-`).
pub fn args(
    permission_mode: PermissionMode,
    model: Option<&str>,
    thread_id: Option<&str>,
) -> Vec<String> {
    let mut a: Vec<String> = vec!["exec".into()];
    if let Some(id) = thread_id {
        a.extend(["resume".into(), id.into()]);
    }
    a.push("--json".into());
    match permission_mode {
        // Codex can't route approvals to us in exec mode, so the sandbox is
        // the safety boundary: writes stay inside the worktree.
        PermissionMode::Auto | PermissionMode::Ask | PermissionMode::AcceptEdits => {
            a.extend(["--sandbox".into(), "workspace-write".into()]);
        }
        PermissionMode::Plan => a.extend(["--sandbox".into(), "read-only".into()]),
        PermissionMode::BypassPermissions => {
            a.push("--dangerously-bypass-approvals-and-sandbox".into());
        }
    }
    if let Some(m) = model {
        a.extend(["--model".into(), m.into()]);
    }
    a.push("-".into());
    a
}

/// Turns a stream of `codex exec --json` lines into transcript items.
pub struct Parser {
    /// Items already shown when they started (commands), so completion only
    /// adds the result.
    started: HashSet<String>,
    pub thread_id: Option<String>,
    pub finished: bool,
    began: Instant,
}

impl Default for Parser {
    fn default() -> Self {
        Self {
            started: HashSet::new(),
            thread_id: None,
            finished: false,
            began: Instant::now(),
        }
    }
}

impl Parser {
    pub fn feed(&mut self, v: &Value) -> Vec<ChatItem> {
        match v["type"].as_str() {
            Some("thread.started") => {
                self.thread_id = v["thread_id"].as_str().map(str::to_string);
                vec![]
            }
            Some("item.started") => {
                let item = &v["item"];
                match tool_use(item) {
                    Some(use_) if item["type"] == "command_execution" => {
                        self.started.insert(item_id(item));
                        vec![use_]
                    }
                    _ => vec![],
                }
            }
            Some("item.completed") => self.completed(&v["item"]),
            Some("turn.completed") => {
                self.finished = true;
                let usage = &v["usage"];
                vec![ChatItem::TurnEnd {
                    is_error: false,
                    duration_ms: Some(self.began.elapsed().as_millis() as u64),
                    // ChatGPT-plan usage has no per-turn price.
                    cost_usd: None,
                    input_tokens: usage["input_tokens"].as_u64(),
                    output_tokens: usage["output_tokens"].as_u64(),
                }]
            }
            Some("turn.failed") => {
                self.finished = true;
                let message = v["error"]["message"]
                    .as_str()
                    .unwrap_or("Codex turn failed");
                vec![
                    ChatItem::Error {
                        message: friendly_error(message),
                    },
                    ChatItem::TurnEnd {
                        is_error: true,
                        duration_ms: Some(self.began.elapsed().as_millis() as u64),
                        cost_usd: None,
                        input_tokens: None,
                        output_tokens: None,
                    },
                ]
            }
            // Transient transport retries; the final outcome arrives as turn.*.
            _ => vec![],
        }
    }

    fn completed(&mut self, item: &Value) -> Vec<ChatItem> {
        let id = item_id(item);
        match item["type"].as_str().unwrap_or_default() {
            "agent_message" => text_item(item, |text| ChatItem::Assistant { text }),
            "reasoning" => text_item(item, |text| ChatItem::Thinking { text }),
            "error" => {
                let message = item["message"].as_str().unwrap_or_default();
                // Codex narrates its own transport fallbacks as items.
                if message.starts_with("Falling back") {
                    vec![]
                } else {
                    vec![ChatItem::Error {
                        message: friendly_error(message),
                    }]
                }
            }
            "command_execution" => {
                let mut out = vec![];
                if !self.started.remove(&id) {
                    out.extend(tool_use(item));
                }
                let exit = item["exit_code"].as_i64();
                out.push(ChatItem::ToolResult {
                    tool_use_id: id,
                    content: item["aggregated_output"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    is_error: exit.is_some_and(|c| c != 0) || item["status"] == "failed",
                });
                out
            }
            "file_change" | "mcp_tool_call" | "web_search" | "todo_list" => {
                let mut out: Vec<ChatItem> = tool_use(item).into_iter().collect();
                out.push(ChatItem::ToolResult {
                    tool_use_id: id,
                    content: result_summary(item),
                    is_error: item["status"] == "failed",
                });
                out
            }
            _ => vec![],
        }
    }
}

fn item_id(item: &Value) -> String {
    item["id"].as_str().unwrap_or_default().to_string()
}

fn text_item(item: &Value, make: impl Fn(String) -> ChatItem) -> Vec<ChatItem> {
    match item["text"].as_str().map(str::trim) {
        Some(t) if !t.is_empty() => vec![make(t.to_string())],
        _ => vec![],
    }
}

/// Tool-call card for a Codex item, named like Claude's tools so the UI's
/// summaries (command, file path, query…) just work.
fn tool_use(item: &Value) -> Option<ChatItem> {
    let (name, input) = match item["type"].as_str()? {
        "command_execution" => ("Bash".to_string(), json!({"command": item["command"]})),
        "file_change" => {
            let changes = item["changes"].as_array().cloned().unwrap_or_default();
            let first = changes
                .first()
                .and_then(|c| c["path"].as_str())
                .unwrap_or("");
            (
                "Edit".to_string(),
                json!({"file_path": first, "changes": changes}),
            )
        }
        "mcp_tool_call" => (
            format!(
                "mcp__{}__{}",
                item["server"].as_str().unwrap_or("mcp"),
                item["tool"].as_str().unwrap_or("tool")
            ),
            item["arguments"].clone(),
        ),
        "web_search" => ("WebSearch".to_string(), json!({"query": item["query"]})),
        "todo_list" => ("TodoWrite".to_string(), json!({"todos": item["items"]})),
        _ => return None,
    };
    Some(ChatItem::ToolUse {
        id: item_id(item),
        name,
        input,
    })
}

fn result_summary(item: &Value) -> String {
    match item["type"].as_str() {
        Some("file_change") => item["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                format!(
                    "{} {}",
                    c["kind"].as_str().unwrap_or("update"),
                    c["path"].as_str().unwrap_or("?")
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some("todo_list") => item["items"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                let done = t["completed"].as_bool().unwrap_or(false);
                format!(
                    "[{}] {}",
                    if done { "x" } else { " " },
                    t["text"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => item["result"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| item["status"].as_str().unwrap_or("done").to_string()),
    }
}

/// Codex errors are long transport dumps; lead with what the user can act on.
fn friendly_error(message: &str) -> String {
    if message.contains("402") || message.contains("deactivated_workspace") {
        return format!(
            "Codex refused the request: the ChatGPT workspace you're signed into isn't active \
             (402). Run `codex login` with an active plan or an API key.\n\n{message}"
        );
    }
    if message.contains("401") {
        return format!("Codex isn't signed in. Run `codex login`.\n\n{message}");
    }
    message.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(lines: &[Value]) -> (Parser, Vec<ChatItem>) {
        let mut p = Parser::default();
        let items = lines.iter().flat_map(|l| p.feed(l)).collect();
        (p, items)
    }

    #[test]
    fn maps_codex_items() {
        let (p, items) = feed_all(&[
            json!({"type":"thread.started","thread_id":"th_1"}),
            json!({"type":"turn.started"}),
            json!({"type":"item.completed","item":{"id":"i0","type":"reasoning","text":"Looking around"}}),
            json!({"type":"item.started","item":{"id":"i1","type":"command_execution","command":"ls","status":"in_progress"}}),
            json!({"type":"item.completed","item":{"id":"i1","type":"command_execution","command":"ls",
                "aggregated_output":"README.md\n","exit_code":0,"status":"completed"}}),
            json!({"type":"item.completed","item":{"id":"i2","type":"file_change",
                "changes":[{"path":"src/a.rs","kind":"update"}],"status":"completed"}}),
            json!({"type":"item.completed","item":{"id":"i3","type":"agent_message","text":"Done."}}),
            json!({"type":"turn.completed","usage":{"input_tokens":120,"cached_input_tokens":0,"output_tokens":30}}),
        ]);
        assert_eq!(p.thread_id.as_deref(), Some("th_1"));
        assert!(p.finished);
        assert_eq!(
            items[0],
            ChatItem::Thinking {
                text: "Looking around".into()
            }
        );
        assert_eq!(
            items[1],
            ChatItem::ToolUse {
                id: "i1".into(),
                name: "Bash".into(),
                input: json!({"command": "ls"})
            }
        );
        assert_eq!(
            items[2],
            ChatItem::ToolResult {
                tool_use_id: "i1".into(),
                content: "README.md\n".into(),
                is_error: false
            }
        );
        assert!(matches!(&items[3], ChatItem::ToolUse { name, input, .. }
            if name == "Edit" && input["file_path"] == "src/a.rs"));
        assert!(
            matches!(&items[4], ChatItem::ToolResult { content, .. } if content == "update src/a.rs")
        );
        assert_eq!(
            items[5],
            ChatItem::Assistant {
                text: "Done.".into()
            }
        );
        assert!(matches!(
            items[6],
            ChatItem::TurnEnd {
                is_error: false,
                input_tokens: Some(120),
                output_tokens: Some(30),
                ..
            }
        ));
    }

    #[test]
    fn failed_turns_explain_themselves() {
        let (p, items) = feed_all(&[
            json!({"type":"thread.started","thread_id":"th_2"}),
            json!({"type":"error","message":"Reconnecting... 1/5 (unexpected status 402)"}),
            json!({"type":"item.completed","item":{"id":"e","type":"error","message":"Falling back from WebSockets to HTTPS transport."}}),
            json!({"type":"turn.failed","error":{"message":"unexpected status 402 Payment Required, auth error code: deactivated_workspace"}}),
        ]);
        assert!(p.finished);
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(
            matches!(&items[0], ChatItem::Error { message } if message.starts_with("Codex refused the request"))
        );
        assert!(matches!(items[1], ChatItem::TurnEnd { is_error: true, .. }));
    }

    #[test]
    fn args_by_mode() {
        let a = args(PermissionMode::Auto, Some("gpt-5"), Some("th_1"));
        assert_eq!(a[..4], ["exec", "resume", "th_1", "--json"]);
        assert!(a.windows(2).any(|w| w == ["--sandbox", "workspace-write"]));
        assert!(a.windows(2).any(|w| w == ["--model", "gpt-5"]));
        assert_eq!(a.last().map(String::as_str), Some("-"));
        assert!(args(PermissionMode::Plan, None, None)
            .windows(2)
            .any(|w| w == ["--sandbox", "read-only"]));
        assert!(args(PermissionMode::BypassPermissions, None, None)
            .contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
    }
}
