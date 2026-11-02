//! End-to-end: a real daemon on a random port, a real git repo with an
//! `origin`, driven purely through the HTTP API.

use std::path::Path;
use std::process::Command as Proc;
use std::time::Duration;

use bantu_protocol::{
    ChatItem, Event, EventData, Repo, Session, SessionStatus, StateSnapshot, Workspace,
    WorkspaceState,
};
use futures::StreamExt;
use serde_json::{json, Value};

/// Stands in for Claude Code: logs its arguments, echoes the prompt back as
/// stream-json, and sleeps when asked to so tests can queue and cancel.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
{ printf '%s ' "$@" | tr '\n' ' '; echo; } >> "CALLS"
prompt=$(cat)
case "$prompt" in *slow*) sleep 3 ;; esac
echo '{"type":"system","subtype":"init"}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}'
echo '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"README.md","is_error":false}]}}'
printf '{"type":"assistant","message":{"content":[{"type":"text","text":"echo: %s"}]}}\n' "$prompt"
echo '{"type":"result","is_error":false,"duration_ms":5,"total_cost_usd":0.25,"usage":{"input_tokens":3,"output_tokens":2}}'
"#;

struct Harness {
    base: String,
    token: String,
    /// Arguments of every fake `claude` invocation, one line each.
    claude_calls: std::path::PathBuf,
    http: reqwest::Client,
    _dirs: Vec<tempfile::TempDir>,
}

impl Harness {
    async fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        let workspaces = tempfile::tempdir().unwrap();
        let claude_calls = home.path().join("claude-calls.log");
        let claude_bin = home.path().join("fake-claude");
        std::fs::write(
            &claude_bin,
            FAKE_CLAUDE.replace("CALLS", claude_calls.to_str().unwrap()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let config = bantud::Config {
            home: home.path().to_path_buf(),
            workspaces_dir: workspaces.path().to_path_buf(),
            web_dir: None,
            branch_prefix: "bantu/".into(),
            claude_bin,
        };
        let started = bantud::start(config, false).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, started.router).await.unwrap() });
        Self {
            base: format!("http://{addr}"),
            token: started.token,
            claude_calls,
            http: reqwest::Client::new(),
            _dirs: vec![home, workspaces],
        }
    }

    async fn command(&self, cmd: Value, key: Option<&str>) -> (u16, Value) {
        let mut req = self
            .http
            .post(format!("{}/v1/commands", self.base))
            .bearer_auth(&self.token)
            .json(&cmd);
        if let Some(k) = key {
            req = req.header("Idempotency-Key", k);
        }
        let res = req.send().await.unwrap();
        (res.status().as_u16(), res.json().await.unwrap())
    }

    async fn state(&self) -> StateSnapshot {
        self.http
            .get(format!("{}/v1/state", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn history(&self, workspace_id: &str) -> Vec<Event> {
        self.http
            .get(format!(
                "{}/v1/workspaces/{workspace_id}/history",
                self.base
            ))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn wait_session(&self, id: &str, done: impl Fn(&Session) -> bool) -> Session {
        for _ in 0..200 {
            let s = self
                .state()
                .await
                .sessions
                .into_iter()
                .find(|s| s.id == id)
                .unwrap();
            if done(&s) {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("session {id} never reached the expected state");
    }

    async fn wait_for(&self, id: &str, want: WorkspaceState) -> Workspace {
        for _ in 0..200 {
            let ws = self
                .state()
                .await
                .workspaces
                .into_iter()
                .find(|w| w.id == id)
                .unwrap();
            if ws.state == want {
                return ws;
            }
            assert_ne!(
                ws.state,
                WorkspaceState::Failed,
                "workspace failed: {:?}",
                ws.error
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("workspace {id} never reached {want:?}");
    }

    /// Reads SSE events from `since` until `n` events have arrived.
    async fn events(&self, since: i64, n: usize) -> Vec<Event> {
        let res = self
            .http
            .get(format!(
                "{}/v1/events?since={since}&token={}",
                self.base, self.token
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let mut stream = res.bytes_stream();
        let mut buf = String::new();
        let mut out = Vec::new();
        while out.len() < n {
            let chunk = tokio::time::timeout(Duration::from_secs(10), stream.next())
                .await
                .expect("timed out waiting for events")
                .unwrap()
                .unwrap();
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = buf.find("\n\n") {
                let frame: String = buf.drain(..end + 2).collect();
                for line in frame.lines() {
                    if let Some(data) = line.strip_prefix("data:") {
                        out.push(serde_json::from_str(data.trim()).unwrap());
                    }
                }
            }
        }
        out
    }
}

fn sh(dir: &Path, args: &[&str]) {
    let st = Proc::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// A bare `origin` plus a clone with `.env` (ignored) and a setup script.
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let clone = dir.path().join("app");
    sh(
        dir.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    sh(
        dir.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    sh(&clone, &["config", "user.email", "t@example.com"]);
    sh(&clone, &["config", "user.name", "t"]);
    std::fs::write(clone.join(".gitignore"), ".env\nsetup-ran.txt\n").unwrap();
    std::fs::create_dir_all(clone.join(".bantu")).unwrap();
    std::fs::write(
        clone.join(".bantu/settings.toml"),
        "[scripts]\nsetup = \"echo port=$BANTU_PORT > setup-ran.txt; echo hello-from-setup\"\n",
    )
    .unwrap();
    std::fs::write(clone.join("README.md"), "hi\n").unwrap();
    sh(&clone, &["add", "."]);
    sh(&clone, &["commit", "-qm", "init"]);
    sh(&clone, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(clone.join(".env"), "SECRET=1\n").unwrap();
    (dir, clone)
}

#[tokio::test]
async fn auth_and_host_checks() {
    let h = Harness::start().await;
    let health: Value = h
        .http
        .get(format!("{}/v1/health", h.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["ok"], true);

    let res = h
        .http
        .get(format!("{}/v1/state", h.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    let res = h
        .http
        .get(format!("{}/v1/state", h.base))
        .bearer_auth("nope")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);

    let res = h
        .http
        .get(format!("{}/v1/health", h.base))
        .header("Host", "evil.example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn workspace_lifecycle() {
    let h = Harness::start().await;
    let (_fixture, clone) = fixture();

    // Adding a repo is idempotent by root path, even from a subdirectory.
    let (status, body) = h
        .command(json!({"type": "repo.add", "path": clone}), None)
        .await;
    assert_eq!(status, 200, "{body}");
    let repo: Repo = serde_json::from_value(body["result"].clone()).unwrap();
    assert_eq!(repo.default_branch, "main");
    assert!(repo.remote_url.is_some());
    let (_, again) = h
        .command(
            json!({"type": "repo.add", "path": clone.join(".bantu")}),
            None,
        )
        .await;
    assert_eq!(again["result"]["id"], json!(repo.id));

    // Create, retried with the same Idempotency-Key, yields one workspace.
    let create = json!({"type": "workspace.create", "repo_id": repo.id});
    let (status, first) = h.command(create.clone(), Some("k1")).await;
    assert_eq!(status, 200, "{first}");
    let (_, retry) = h.command(create, Some("k1")).await;
    assert_eq!(first, retry);
    let ws: Workspace = serde_json::from_value(first["result"].clone()).unwrap();
    assert_eq!(ws.base_ref, "origin/main");
    assert_eq!(ws.branch, format!("bantu/{}", ws.name));

    let ws = h.wait_for(&ws.id, WorkspaceState::Ready).await;
    let path = Path::new(&ws.path);
    assert!(path.join("README.md").exists());
    assert_eq!(
        std::fs::read_to_string(path.join(".env")).unwrap(),
        "SECRET=1\n"
    );
    assert_eq!(
        std::fs::read_to_string(path.join("setup-ran.txt"))
            .unwrap()
            .trim(),
        format!("port={}", ws.port_base)
    );
    assert_eq!(h.state().await.workspaces.len(), 1);

    // A second workspace gets a different name and port block.
    let (_, second) = h
        .command(
            json!({"type": "workspace.create", "repo_id": repo.id}),
            None,
        )
        .await;
    let second: Workspace = serde_json::from_value(second["result"].clone()).unwrap();
    assert_ne!(second.name, ws.name);
    assert_eq!(second.port_base, ws.port_base + 10);
    h.wait_for(&second.id, WorkspaceState::Ready).await;

    // The event log replays gap-free and includes setup output.
    let last_seq = h.state().await.last_seq;
    let events = h.events(0, last_seq as usize).await;
    let seqs: Vec<i64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=last_seq).collect::<Vec<_>>());
    assert!(events.iter().any(|e| matches!(&e.data,
        EventData::WorkspaceLog { line, .. } if line == "hello-from-setup")));
    // Resuming from the middle starts right after `since`.
    assert_eq!(h.events(3, 1).await[0].seq, 4);

    // Archive keeps the branch and saves uncommitted tracked changes.
    std::fs::write(path.join("README.md"), "edited\n").unwrap();
    let (status, body) = h
        .command(
            json!({"type": "workspace.archive", "workspace_id": ws.id}),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let archived = h.wait_for(&ws.id, WorkspaceState::Archived).await;
    assert!(!path.exists());
    assert!(archived.archive_commit.is_some());
    sh(
        &clone,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{}", ws.branch),
        ],
    );
    sh(
        &clone,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/bantu/archive/{}", ws.id),
        ],
    );

    // Repos with live workspaces can't be removed.
    let (status, _) = h
        .command(json!({"type": "repo.remove", "repo_id": repo.id}), None)
        .await;
    assert_eq!(status, 409);

    // Bad input is a 4xx, not a crash.
    let (status, _) = h
        .command(
            json!({"type": "repo.add", "path": "/definitely/not/here"}),
            None,
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = h
        .command(
            json!({"type": "workspace.create", "repo_id": repo.id, "name": "Bad Name"}),
            None,
        )
        .await;
    assert_eq!(status, 400);
}

fn chat(events: &[Event], session_id: &str) -> Vec<ChatItem> {
    events
        .iter()
        .filter_map(|e| match &e.data {
            EventData::SessionItem {
                session_id: s,
                item,
                ..
            } if s == session_id => Some(item.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn agent_sessions() {
    let h = Harness::start().await;
    let (_fixture, clone) = fixture();
    let (_, body) = h
        .command(json!({"type": "repo.add", "path": clone}), None)
        .await;
    let repo_id = body["result"]["id"].as_str().unwrap().to_string();
    let (_, body) = h
        .command(
            json!({"type": "workspace.create", "repo_id": repo_id}),
            None,
        )
        .await;
    let ws: Workspace = serde_json::from_value(body["result"].clone()).unwrap();
    h.wait_for(&ws.id, WorkspaceState::Ready).await;

    // A ready workspace comes with one idle chat.
    let mut session = None;
    for _ in 0..100 {
        session = h
            .state()
            .await
            .sessions
            .into_iter()
            .find(|s| s.workspace_id == ws.id);
        if session.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let session = session.expect("auto-created session");
    assert_eq!(session.title, "New chat");
    let send = |text: &str| json!({"type": "session.send", "session_id": session.id, "text": text});

    // First turn starts a new Claude conversation and records the transcript.
    let (status, body) = h.command(send("hello there"), None).await;
    assert_eq!(status, 200, "{body}");
    let s = h
        .wait_session(&session.id, |s| {
            s.status == SessionStatus::Idle && s.turns == 1
        })
        .await;
    assert_eq!(s.title, "hello there");
    assert!((s.total_cost_usd - 0.25).abs() < 1e-9);
    let items = chat(&h.history(&ws.id).await, &session.id);
    assert_eq!(
        items[0],
        ChatItem::User {
            text: "hello there".into()
        }
    );
    assert!(matches!(&items[1], ChatItem::ToolUse { name, .. } if name == "Bash"));
    assert!(matches!(&items[2], ChatItem::ToolResult { content, .. } if content == "README.md"));
    assert_eq!(
        items[3],
        ChatItem::Assistant {
            text: "echo: hello there".into()
        }
    );
    assert!(matches!(
        items[4],
        ChatItem::TurnEnd {
            is_error: false,
            ..
        }
    ));

    // Later turns resume the same conversation.
    h.command(send("second"), None).await;
    h.wait_session(&session.id, |s| {
        s.status == SessionStatus::Idle && s.turns == 2
    })
    .await;
    let calls = std::fs::read_to_string(&h.claude_calls).unwrap();
    let calls: Vec<&str> = calls.lines().collect();
    assert!(calls[0].contains("--session-id"), "{}", calls[0]);
    assert!(calls[1].contains("--resume"), "{}", calls[1]);
    assert!(calls[1].contains("--permission-mode auto"), "{}", calls[1]);

    // Messages sent mid-turn are queued and run afterwards, in order.
    h.command(send("slow one"), None).await;
    let (_, body) = h.command(send("after the slow one"), None).await;
    assert_eq!(body["result"]["queued"], 1);
    let s = h
        .wait_session(&session.id, |s| {
            s.status == SessionStatus::Idle && s.turns == 4
        })
        .await;
    assert_eq!(s.queued, 0);
    let items = chat(&h.history(&ws.id).await, &session.id);
    let users: Vec<_> = items
        .iter()
        .filter_map(|i| match i {
            ChatItem::User { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        users,
        ["hello there", "second", "slow one", "after the slow one"]
    );

    // Stopping kills the running turn and drops the queue.
    h.command(send("slow again"), None).await;
    h.command(send("never runs"), None).await;
    h.wait_session(&session.id, |s| s.status == SessionStatus::Running)
        .await;
    let (status, _) = h
        .command(
            json!({"type": "session.cancel", "session_id": session.id}),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let s = h
        .wait_session(&session.id, |s| s.status == SessionStatus::Idle)
        .await;
    assert_eq!(s.turns, 5);
    let items = chat(&h.history(&ws.id).await, &session.id);
    assert_eq!(
        items.last(),
        Some(&ChatItem::Error {
            message: "Stopped".into()
        })
    );

    // Empty messages are rejected; settings changes stick.
    let (status, _) = h.command(send("   "), None).await;
    assert_eq!(status, 400);
    let (status, body) = h
        .command(
            json!({"type": "session.update", "session_id": session.id, "model": "haiku", "permission_mode": "plan"}),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["result"]["permission_mode"], "plan");
    h.command(send("planned"), None).await;
    h.wait_session(&session.id, |s| {
        s.status == SessionStatus::Idle && s.turns == 6
    })
    .await;
    let calls = std::fs::read_to_string(&h.claude_calls).unwrap();
    let last = calls.lines().last().unwrap();
    assert!(
        last.contains("--model haiku") && last.contains("--permission-mode plan"),
        "{last}"
    );
}
