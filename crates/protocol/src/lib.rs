//! Wire types for the Bantu daemon protocol (v1).
//!
//! Clients send [`Command`]s with `POST /v1/commands` and follow state with the
//! `GET /v1/events?since=<seq>` SSE stream of [`Event`]s. `GET /v1/state` returns a
//! [`StateSnapshot`] so a client can render immediately and then follow events
//! from `last_seq`.
//!
//! TypeScript bindings are generated into `packages/protocol/src/generated` by
//! `cargo test -p bantu-protocol export_bindings`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct Repo {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub default_branch: String,
    pub remote_url: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WorkspaceState {
    Creating,
    Ready,
    Failed,
    Archiving,
    Archived,
}

impl WorkspaceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Archiving => "archiving",
            Self::Archived => "archived",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "creating" => Self::Creating,
            "ready" => Self::Ready,
            "failed" => Self::Failed,
            "archiving" => Self::Archiving,
            "archived" => Self::Archived,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct Workspace {
    pub id: String,
    pub repo_id: String,
    /// Human name, e.g. a city ("lisbon").
    pub name: String,
    pub branch: String,
    /// What the branch was created from and what PRs target, e.g. `origin/main`.
    pub base_ref: String,
    pub path: String,
    /// First of ten ports reserved for this workspace (`BANTU_PORT` .. `BANTU_PORT+9`).
    pub port_base: u16,
    pub state: WorkspaceState,
    /// Set when `state` is `failed`.
    pub error: Option<String>,
    /// Commit holding uncommitted tracked changes at archive time (`refs/bantu/archive/<id>`).
    pub archive_commit: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AgentKind {
    Claude,
}

/// How much the agent may do without asking. Bantu can't answer permission
/// prompts yet, so anything that would prompt is denied.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PermissionMode {
    /// A classifier approves safe actions automatically.
    #[default]
    Auto,
    /// File edits are allowed; other risky actions are denied.
    AcceptEdits,
    /// Read-only planning; the agent proposes instead of changing files.
    Plan,
    /// Everything is allowed. Only for trusted repos.
    BypassPermissions,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::AcceptEdits => "accept_edits",
            Self::Plan => "plan",
            Self::BypassPermissions => "bypass_permissions",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "auto" => Self::Auto,
            "accept_edits" => Self::AcceptEdits,
            "plan" => Self::Plan,
            "bypass_permissions" => Self::BypassPermissions,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SessionStatus {
    Idle,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub agent: AgentKind,
    pub title: String,
    /// `None` uses the agent's default model.
    pub model: Option<String>,
    pub permission_mode: PermissionMode,
    pub status: SessionStatus,
    /// Messages waiting for the current turn to finish.
    pub queued: u32,
    /// Number of turns started so far.
    pub turns: u32,
    pub total_cost_usd: f64,
    pub created_at: String,
}

/// One entry in a chat transcript, normalized across agents.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum ChatItem {
    User {
        text: String,
    },
    Assistant {
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[ts(type = "unknown")]
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        /// Text output, truncated to keep the event log small.
        content: String,
        is_error: bool,
    },
    /// The agent finished a turn.
    TurnEnd {
        is_error: bool,
        #[ts(type = "number | null")]
        duration_ms: Option<u64>,
        cost_usd: Option<f64>,
        #[ts(type = "number | null")]
        input_tokens: Option<u64>,
        #[ts(type = "number | null")]
        output_tokens: Option<u64>,
    },
    /// Bantu-side problems: the agent couldn't start, crashed, or was cancelled.
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct FileDiff {
    pub path: String,
    /// Previous path for renames.
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
    /// Unified diff hunks for this file (starting at the first `@@`).
    pub patch: String,
    /// The patch was cut because the file's diff is very large.
    pub truncated: bool,
}

/// Everything the workspace changed relative to where it branched off,
/// including uncommitted and untracked files.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct WorkspaceDiff {
    pub workspace_id: String,
    pub base_ref: String,
    pub merge_base: String,
    /// Commits on the branch that aren't on `base_ref`.
    pub ahead: u32,
    /// Commits on `base_ref` that the branch doesn't have yet.
    pub behind: u32,
    /// Files with uncommitted changes.
    pub uncommitted: u32,
    pub files: Vec<FileDiff>,
}

/// A review note on a line of the workspace diff, to be sent to the agent.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct DiffComment {
    pub id: String,
    pub workspace_id: String,
    pub path: String,
    /// Line number in the new version of the file.
    pub line: u32,
    pub body: String,
    pub created_at: String,
    /// When the comment was sent to an agent; `None` while it's a draft.
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct PullRequest {
    pub number: u32,
    pub url: String,
    pub title: String,
    /// `OPEN`, `CLOSED` or `MERGED`.
    pub state: String,
    pub is_draft: bool,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, or `None`.
    pub review_decision: Option<String>,
    pub checks_passing: u32,
    pub checks_failing: u32,
    pub checks_pending: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TerminalKind {
    /// An interactive login shell.
    Shell,
    /// The repo's run script (`[scripts] run` in `.bantu/settings.toml`).
    Run,
}

/// A PTY in a workspace. Bytes flow over `GET /v1/terminals/{id}/ws`.
/// Terminals live in the daemon's memory and end when it restarts.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct Terminal {
    pub id: String,
    pub workspace_id: String,
    pub kind: TerminalKind,
    pub title: String,
    /// The command for `run` terminals.
    pub command: Option<String>,
    pub exited: bool,
    pub exit_code: Option<i32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(tag = "type")]
#[ts(export)]
pub enum Command {
    /// Register a local git repository (any path inside it).
    #[serde(rename = "repo.add")]
    RepoAdd { path: String },
    #[serde(rename = "repo.remove")]
    RepoRemove { repo_id: String },
    /// Create a worktree + branch. `name` defaults to an unused city name and
    /// `base_ref` to `origin/<default_branch>` (or the local default branch).
    #[serde(rename = "workspace.create")]
    WorkspaceCreate {
        repo_id: String,
        name: Option<String>,
        base_ref: Option<String>,
    },
    /// Run the archive script, save uncommitted tracked changes to a ref and
    /// remove the worktree. The branch is kept.
    #[serde(rename = "workspace.archive")]
    WorkspaceArchive { workspace_id: String },
    /// Start a new chat with an agent in a workspace. Every workspace gets one
    /// automatically when it becomes ready.
    #[serde(rename = "session.create")]
    SessionCreate {
        workspace_id: String,
        model: Option<String>,
        permission_mode: Option<PermissionMode>,
    },
    /// Send a user message. Queued if the agent is mid-turn.
    #[serde(rename = "session.send")]
    SessionSend { session_id: String, text: String },
    /// Stop the running turn and drop queued messages.
    #[serde(rename = "session.cancel")]
    SessionCancel { session_id: String },
    /// Change settings that apply from the next turn.
    #[serde(rename = "session.update")]
    SessionUpdate {
        session_id: String,
        title: Option<String>,
        model: Option<String>,
        permission_mode: Option<PermissionMode>,
    },
    /// Add a draft review comment on a line of the workspace diff.
    #[serde(rename = "comment.add")]
    CommentAdd {
        workspace_id: String,
        path: String,
        line: u32,
        body: String,
    },
    #[serde(rename = "comment.delete")]
    CommentDelete { comment_id: String },
    /// Send all draft comments to a session as one message.
    #[serde(rename = "comments.send")]
    CommentsSend {
        workspace_id: String,
        session_id: String,
    },
    /// Open an interactive shell in the workspace.
    #[serde(rename = "terminal.open")]
    TerminalOpen {
        workspace_id: String,
        cols: Option<u16>,
        rows: Option<u16>,
    },
    /// Kill the terminal's process group and forget it.
    #[serde(rename = "terminal.close")]
    TerminalClose { terminal_id: String },
    /// Start the repo's run script, or return it if it's already running.
    #[serde(rename = "workspace.run")]
    WorkspaceRun { workspace_id: String },
    /// Ask the session's agent to commit, push and open a pull request.
    #[serde(rename = "workspace.create_pr")]
    WorkspaceCreatePr {
        workspace_id: String,
        session_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(tag = "type")]
#[ts(export)]
pub enum EventData {
    #[serde(rename = "repo.added")]
    RepoAdded { repo: Repo },
    #[serde(rename = "repo.removed")]
    RepoRemoved { repo_id: String },
    /// Full workspace snapshot; sent on creation and on every change.
    #[serde(rename = "workspace.updated")]
    WorkspaceUpdated { workspace: Workspace },
    /// Output from setup/archive scripts and daemon progress messages.
    #[serde(rename = "workspace.log")]
    WorkspaceLog {
        workspace_id: String,
        stream: LogStream,
        line: String,
    },
    /// Full session snapshot; sent on creation and on every change.
    #[serde(rename = "session.updated")]
    SessionUpdated { session: Session },
    #[serde(rename = "comment.updated")]
    CommentUpdated { comment: DiffComment },
    #[serde(rename = "comment.deleted")]
    CommentDeleted {
        comment_id: String,
        workspace_id: String,
    },
    #[serde(rename = "terminal.updated")]
    TerminalUpdated { terminal: Terminal },
    #[serde(rename = "terminal.closed")]
    TerminalClosed {
        terminal_id: String,
        workspace_id: String,
    },
    /// A transcript entry. `turn` starts at 1.
    #[serde(rename = "session.item")]
    SessionItem {
        session_id: String,
        workspace_id: String,
        turn: u32,
        item: ChatItem,
    },
}

impl EventData {
    /// The workspace an event belongs to, for per-workspace history.
    pub fn workspace_id(&self) -> Option<&str> {
        match self {
            Self::RepoAdded { .. } | Self::RepoRemoved { .. } => None,
            Self::WorkspaceUpdated { workspace } => Some(&workspace.id),
            Self::WorkspaceLog { workspace_id, .. } | Self::SessionItem { workspace_id, .. } => {
                Some(workspace_id)
            }
            Self::SessionUpdated { session } => Some(&session.workspace_id),
            Self::CommentUpdated { comment } => Some(&comment.workspace_id),
            Self::CommentDeleted { workspace_id, .. } => Some(workspace_id),
            Self::TerminalUpdated { terminal } => Some(&terminal.workspace_id),
            Self::TerminalClosed { workspace_id, .. } => Some(workspace_id),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct Event {
    /// Monotonic, gap-free per daemon. Resume with `?since=<seq>`.
    #[ts(type = "number")]
    pub seq: i64,
    pub ts: String,
    pub data: EventData,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct CommandResponse {
    pub ok: bool,
    /// Command-specific payload, e.g. the created `Repo` or `Workspace`.
    #[ts(type = "unknown")]
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct StateSnapshot {
    pub protocol_version: u32,
    pub daemon_version: String,
    #[ts(type = "number")]
    pub last_seq: i64,
    pub repos: Vec<Repo>,
    pub workspaces: Vec<Workspace>,
    pub sessions: Vec<Session>,
    pub comments: Vec<DiffComment>,
    pub terminals: Vec<Terminal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct Health {
    pub ok: bool,
    pub protocol_version: u32,
    pub daemon_version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every generated binding must be re-exported by the TS package.
    #[test]
    fn index_exports_every_binding() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/protocol/src");
        let index = std::fs::read_to_string(format!("{root}/index.ts")).unwrap();
        for name in [
            "AgentKind",
            "ChatItem",
            "Command",
            "CommandResponse",
            "DiffComment",
            "Event",
            "EventData",
            "FileDiff",
            "FileStatus",
            "Health",
            "LogStream",
            "PermissionMode",
            "PullRequest",
            "Repo",
            "Session",
            "SessionStatus",
            "StateSnapshot",
            "Terminal",
            "TerminalKind",
            "Workspace",
            "WorkspaceDiff",
            "WorkspaceState",
        ] {
            assert!(
                index.contains(&format!(
                    "export type {{ {name} }} from \"./generated/{name}\";"
                )),
                "packages/protocol/src/index.ts doesn't export {name}"
            );
        }
    }

    #[test]
    fn command_wire_format() {
        let json = r#"{"type":"workspace.create","repo_id":"r1","name":null,"base_ref":null}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert_eq!(
            cmd,
            Command::WorkspaceCreate {
                repo_id: "r1".into(),
                name: None,
                base_ref: None
            }
        );
    }

    #[test]
    fn event_wire_format() {
        let ev = Event {
            seq: 7,
            ts: "t".into(),
            data: EventData::RepoRemoved {
                repo_id: "r1".into(),
            },
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["data"]["type"], "repo.removed");
        assert_eq!(v["seq"], 7);
    }
}
