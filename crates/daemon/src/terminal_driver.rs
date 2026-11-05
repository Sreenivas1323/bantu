//! Moving a chat between its structured view and the agent's own terminal UI.
//!
//! `session.open_terminal` starts `claude --resume <id>` (or `codex resume`)
//! in a PTY. The chat can't take turns while that's open, because two processes
//! must not write to one conversation. When the terminal ends, whatever happened
//! there is read from Claude's transcript file and added to the chat, so both
//! views share one history.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bantu_protocol::{AgentKind, ChatItem, SessionStatus, Terminal, TerminalKind};
use serde_json::Value;

use crate::agents::shell_quote;
use crate::ops::{CommandError, Daemon};
use crate::sessions::{parse_stream_line, workspace_context};
use crate::terminals::TerminalSpec;

type CmdResult<T> = Result<T, CommandError>;

impl Daemon {
    /// The live terminal this chat is open in, if any.
    pub(crate) async fn session_terminal(&self, session_id: &str) -> Option<Terminal> {
        self.terminals()
            .await
            .into_iter()
            .find(|t| t.session_id.as_deref() == Some(session_id) && !t.exited)
    }

    pub async fn open_session_terminal(
        self: &Arc<Self>,
        session_id: &str,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> CmdResult<Terminal> {
        let (s, agent_id) = self
            .store
            .session(session_id)
            .await?
            .ok_or_else(|| CommandError::NotFound(format!("no session {session_id}")))?;
        if let Some(t) = self.session_terminal(session_id).await {
            return Ok(t);
        }
        if s.status == SessionStatus::Running {
            return Err(CommandError::Conflict(
                "wait for the agent's turn to finish, or stop it first".into(),
            ));
        }
        let ws = self.ready_workspace(&s.workspace_id).await?;
        let resume = self.store.session_has_agent_output(&s.id).await?;
        let (command, transcript_offset) = match s.agent {
            AgentKind::Claude => {
                let bin = shell_quote(&self.config.claude_bin.to_string_lossy());
                let mut parts = vec![
                    bin,
                    (if resume { "--resume" } else { "--session-id" }).to_string(),
                    shell_quote(&agent_id),
                    "--permission-mode".to_string(),
                    crate::sessions::claude_permission_mode(s.permission_mode).to_string(),
                    "--append-system-prompt".to_string(),
                    shell_quote(&workspace_context(&ws)),
                ];
                if let Some(m) = &s.model {
                    parts.extend(["--model".to_string(), shell_quote(m)]);
                }
                if !self.store.preferences().await?.claude_user_setup {
                    parts.extend([
                        "--setting-sources".to_string(),
                        "project,local".to_string(),
                        "--strict-mcp-config".to_string(),
                    ]);
                }
                if let Some(mcp) = self.mcp_config(&s.id) {
                    parts.extend(["--mcp-config".to_string(), shell_quote(&mcp)]);
                }
                let offset = find_transcript(&self.config.claude_config_dir, &agent_id)
                    .and_then(|p| std::fs::metadata(p).ok())
                    .map_or(0, |m| m.len());
                (parts.join(" "), offset)
            }
            AgentKind::Codex => {
                let bin = shell_quote(&self.config.codex_bin.to_string_lossy());
                let command = if resume {
                    format!("{bin} resume {}", shell_quote(&agent_id))
                } else {
                    bin
                };
                (command, 0)
            }
            AgentKind::Opencode | AgentKind::Gemini | AgentKind::Cursor => {
                return Err(CommandError::BadRequest(format!(
                    "{} can't reopen this conversation in a terminal yet; open its CLI from the \
                     Terminal tab instead",
                    s.agent.as_str()
                )))
            }
        };
        self.spawn_terminal(
            Some(&ws),
            TerminalSpec {
                kind: TerminalKind::Agent,
                title: s.title.clone(),
                command: Some(command),
                cols,
                rows,
                session_id: Some(s.id.clone()),
                transcript_offset,
                env: vec![("BANTU_HOOK_TOKEN".into(), self.hook_token().to_string())],
            },
        )
        .await
    }

    /// Adds what happened in the terminal to the chat's transcript.
    pub(crate) async fn finish_session_terminal(self: &Arc<Self>, session_id: &str, offset: u64) {
        let Ok(Some((mut s, agent_id))) = self.store.session(session_id).await else {
            return;
        };
        let mut items = Vec::new();
        if s.agent == AgentKind::Claude {
            if let Some(path) = find_transcript(&self.config.claude_config_dir, &agent_id) {
                items = read_transcript(&path, offset).await.unwrap_or_default();
            }
        }
        let imported = items.len() as u32;
        s.turns += 1;
        let turn = s.turns;
        if let Err(e) = self.store.upsert_session(&s, &agent_id).await {
            tracing::warn!("couldn't record terminal turn for {session_id}: {e:#}");
            return;
        }
        for item in items {
            let _ = self.store.session_item(&s, turn, item).await;
        }
        let _ = self
            .store
            .session_item(&s, turn, ChatItem::TerminalSession { imported })
            .await;
        if let Ok(ws) = self.require_workspace(&s.workspace_id).await {
            self.rescan_conflicts(&ws.repo_id);
        }
    }
}

/// Claude keeps each conversation at `projects/<encoded cwd>/<session id>.jsonl`.
/// The cwd encoding isn't a documented format, so look in every project.
pub fn find_transcript(claude_config_dir: &Path, session_id: &str) -> Option<PathBuf> {
    let file = format!("{session_id}.jsonl");
    std::fs::read_dir(claude_config_dir.join("projects"))
        .ok()?
        .flatten()
        .map(|project| project.path().join(&file))
        .find(|path| path.is_file())
}

async fn read_transcript(path: &Path, offset: u64) -> std::io::Result<Vec<ChatItem>> {
    let bytes = tokio::fs::read(path).await?;
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    Ok(parse_transcript(&String::from_utf8_lossy(&bytes[start..])))
}

/// Maps transcript lines to chat items. Assistant messages and tool results
/// share the `stream-json` shape; typed prompts are also stored as plain strings.
pub fn parse_transcript(text: &str) -> Vec<ChatItem> {
    let mut items = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // Subagent work and injected context aren't part of the conversation.
        if v["isSidechain"] == true || v["isMeta"] == true {
            continue;
        }
        if v["type"] == "user" {
            let content = &v["message"]["content"];
            if let Some(text) = content.as_str() {
                items.extend(typed_prompt(text));
            } else if let Some(blocks) = content.as_array() {
                for b in blocks.iter().filter(|b| b["type"] == "text") {
                    items.extend(typed_prompt(b["text"].as_str().unwrap_or_default()));
                }
            }
        }
        items.extend(parse_stream_line(&v));
    }
    items
}

/// Slash commands are stored as XML-like wrappers, which aren't prompts.
fn typed_prompt(text: &str) -> Option<ChatItem> {
    let text = text.trim();
    let wrapper = text.starts_with("<command-") || text.starts_with("<local-command-");
    (!text.is_empty() && !wrapper).then(|| ChatItem::User {
        text: text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claude_transcripts() {
        let text = r#"{"type":"user","message":{"role":"user","content":"fix the bug"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"On it"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"a.txt"}]}}
{"type":"user","isMeta":true,"message":{"content":"Caveat: injected"}}
{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"subagent"}]}}
{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}
{"type":"summary","summary":"x"}
not json
"#;
        let items = parse_transcript(text);
        assert_eq!(items.len(), 4, "{items:?}");
        assert_eq!(
            items[0],
            ChatItem::User {
                text: "fix the bug".into()
            }
        );
        assert_eq!(
            items[1],
            ChatItem::Assistant {
                text: "On it".into()
            }
        );
        assert!(matches!(&items[2], ChatItem::ToolUse { name, .. } if name == "Bash"));
        assert!(matches!(&items[3], ChatItem::ToolResult { content, .. } if content == "a.txt"));
    }

    #[test]
    fn finds_transcripts_in_any_project() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_transcript(dir.path(), "abc"), None);
        let project = dir.path().join("projects/-some-path");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("abc.jsonl"), "{}").unwrap();
        assert_eq!(
            find_transcript(dir.path(), "abc"),
            Some(project.join("abc.jsonl"))
        );
    }
}
