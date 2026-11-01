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
