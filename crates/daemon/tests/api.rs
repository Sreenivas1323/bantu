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

/// Stands in for Claude Code (see the script for what each prompt does).
const FAKE_CLAUDE: &str = include_str!("../../../scripts/fake-claude.sh");
const FAKE_CODEX: &str = include_str!("../../../scripts/fake-codex.sh");

struct Harness {
    base: String,
    token: String,
    /// Token the agent-facing endpoints (permission hook, MCP) accept.
    hook_token: String,
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
            FAKE_CLAUDE.replace(
                "${BANTU_FAKE_CLAUDE_LOG:-/dev/null}",
                claude_calls.to_str().unwrap(),
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let codex_bin = home.path().join("fake-codex");
        std::fs::write(
            &codex_bin,
            FAKE_CODEX.replace(
                "${BANTU_FAKE_CODEX_LOG:-/dev/null}",
                home.path().join("codex-calls.log").to_str().unwrap(),
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&codex_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let config = bantud::Config {
            home: home.path().to_path_buf(),
            workspaces_dir: workspaces.path().to_path_buf(),
            repos_dir: home.path().join("repos"),
            web_dir: None,
            branch_prefix: "bantu/".into(),
            claude_bin,
            codex_bin,
            allowed_hosts: vec!["box.tail1234.ts.net".into()],
        };
        let started = bantud::start(config, false).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        started.daemon.set_listen_addr(addr);
        let hook_token = started.daemon.hook_token().to_string();
        tokio::spawn(async move { axum::serve(listener, started.router).await.unwrap() });
        Self {
            base: format!("http://{addr}"),
            token: started.token,
            hook_token,
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

    fn codex_calls(&self) -> Vec<String> {
        let log = self.claude_calls.with_file_name("codex-calls.log");
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
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
                // Live text deltas aren't log events; `deltas()` covers them.
                if frame.lines().any(|l| l == "event: delta") {
                    continue;
                }
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

    // Hosts the user allowed (e.g. a Tailscale name) are accepted.
    let res = h
        .http
        .get(format!("{}/v1/health", h.base))
        .header("Host", "BOX.tail1234.ts.net")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
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

    // A remote URL is cloned into the repos dir and added like a local path.
    let origin = clone.parent().unwrap().join("origin.git");
    let (status, body) = h
        .command(
            json!({"type": "repo.add", "path": format!("file://{}", origin.display())}),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["result"]["name"], "origin");
    assert!(Path::new(body["result"]["root_path"].as_str().unwrap())
        .join("README.md")
        .exists());
    let (_, again) = h
        .command(
            json!({"type": "repo.add", "path": format!("file://{}", origin.display())}),
            None,
        )
        .await;
    assert_eq!(
        again["result"]["id"], body["result"]["id"],
        "re-adding reuses the clone"
    );

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
    // The placeholder branch is renamed after the task.
    let renamed = h
        .state()
        .await
        .workspaces
        .into_iter()
        .find(|w| w.id == ws.id)
        .unwrap();
    assert_eq!(renamed.branch, "bantu/hello-there");
    sh(
        Path::new(&renamed.path),
        &[
            "rev-parse",
            "--verify",
            "-q",
            "refs/heads/bantu/hello-there",
        ],
    );
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

#[tokio::test]
async fn review_flow() {
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
    let ws = h.wait_for(&ws.id, WorkspaceState::Ready).await;
    let path = Path::new(&ws.path);

    // One committed change, one uncommitted edit, one untracked file.
    std::fs::write(path.join("committed.txt"), "c\n").unwrap();
    sh(path, &["add", "committed.txt"]);
    sh(
        path,
        &[
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "c",
        ],
    );
    std::fs::write(path.join("README.md"), "hi\nmore\n").unwrap();
    std::fs::write(path.join("new.txt"), "n\n").unwrap();

    let diff: Value = h
        .http
        .get(format!("{}/v1/workspaces/{}/diff", h.base, ws.id))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(diff["ahead"], 1);
    assert_eq!(diff["uncommitted"], 2);
    let files: Vec<(String, String)> = diff["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().into(),
                f["status"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        files,
        [
            ("README.md".into(), "modified".into()),
            ("committed.txt".into(), "added".into()),
            ("new.txt".into(), "added".into())
        ]
    );
    // Computing the diff must not stage anything in the user's index.
    let staged = Proc::new("git")
        .arg("-C")
        .arg(path)
        .args(["diff", "--cached", "--name-only"])
        .output()
        .unwrap();
    assert!(staged.stdout.is_empty());

    // Draft comments are bundled into one agent message, then marked sent.
    let (status, _) = h
        .command(json!({"type": "comment.add", "workspace_id": ws.id, "path": "README.md", "line": 2, "body": "say more"}), None)
        .await;
    assert_eq!(status, 200);
    let (_, c2) = h
        .command(json!({"type": "comment.add", "workspace_id": ws.id, "path": "new.txt", "line": 1, "body": "drop this"}), None)
        .await;
    assert_eq!(h.state().await.comments.len(), 2);
    let (status, _) = h
        .command(
            json!({"type": "comment.delete", "comment_id": c2["result"]["id"]}),
            None,
        )
        .await;
    assert_eq!(status, 200);

    let session = loop {
        if let Some(s) = h
            .state()
            .await
            .sessions
            .into_iter()
            .find(|s| s.workspace_id == ws.id)
        {
            break s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let (status, body) = h
        .command(
            json!({"type": "comments.send", "workspace_id": ws.id, "session_id": session.id}),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    h.wait_session(&session.id, |s| {
        s.status == SessionStatus::Idle && s.turns == 1
    })
    .await;
    let comments = h.state().await.comments;
    assert_eq!(comments.len(), 1);
    assert!(comments[0].sent_at.is_some());
    let items = chat(&h.history(&ws.id).await, &session.id);
    assert!(
        matches!(&items[0], ChatItem::User { text } if text.contains("- `README.md:2` — say more"))
    );
    let (status, _) = h
        .command(
            json!({"type": "comments.send", "workspace_id": ws.id, "session_id": session.id}),
            None,
        )
        .await;
    assert_eq!(status, 400, "nothing left to send");

    // Create PR hands the agent a prompt naming the branch and target.
    let (status, _) = h
        .command(
            json!({"type": "workspace.create_pr", "workspace_id": ws.id, "session_id": session.id}),
            None,
        )
        .await;
    assert_eq!(status, 200);
    h.wait_session(&session.id, |s| {
        s.status == SessionStatus::Idle && s.turns == 2
    })
    .await;
    let items = chat(&h.history(&ws.id).await, &session.id);
    let prompt = items.iter().rev().find_map(|i| match i {
        ChatItem::User { text } => Some(text.clone()),
        _ => None,
    });
    let prompt = prompt.unwrap();
    let branch = h
        .state()
        .await
        .workspaces
        .into_iter()
        .find(|w| w.id == ws.id)
        .unwrap()
        .branch;
    assert!(branch.starts_with("bantu/address-these-review"), "{branch}");
    assert!(
        prompt.contains(&format!("--base main --head {branch}")),
        "{prompt}"
    );

    // A local bare "origin" has no GitHub PR.
    let pr: Value = h
        .http
        .get(format!("{}/v1/workspaces/{}/pr", h.base, ws.id))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(pr.is_null());
}

/// Reads terminal output until `needle` shows up.
async fn read_until<S>(ws: &mut S, needle: &str) -> String
where
    S: futures::Stream<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    let mut seen = String::new();
    while !seen.contains(needle) {
        let msg = tokio::time::timeout(Duration::from_secs(15), ws.next())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {needle:?}; saw {seen:?}"))
            .expect("socket closed")
            .unwrap();
        if let tokio_tungstenite::tungstenite::Message::Binary(b) = msg {
            seen.push_str(&String::from_utf8_lossy(&b));
        }
    }
    seen
}

#[tokio::test]
async fn terminals() {
    use futures::SinkExt;
    use tokio_tungstenite::tungstenite::Message;

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
    let ws = h.wait_for(&ws.id, WorkspaceState::Ready).await;
    let ws_url = |id: &str| {
        format!(
            "{}/v1/terminals/{id}/ws?token={}",
            h.base.replace("http", "ws"),
            h.token
        )
    };

    // An interactive shell runs in the worktree and echoes input back.
    let (status, body) = h
        .command(
            json!({"type": "terminal.open", "workspace_id": ws.id, "cols": 80, "rows": 24}),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let term_id = body["result"]["id"].as_str().unwrap().to_string();
    let (mut sock, _) = tokio_tungstenite::connect_async(ws_url(&term_id))
        .await
        .unwrap();
    sock.send(Message::Text(
        r#"{"type":"resize","cols":100,"rows":30}"#.into(),
    ))
    .await
    .unwrap();
    sock.send(Message::Binary(
        b"echo bantu-$((40+2)) $BANTU_PORT $(basename \"$PWD\")\n"
            .to_vec()
            .into(),
    ))
    .await
    .unwrap();
    // One output line proves arithmetic, env and cwd; the echoed input can't
    // match because it contains the unexpanded `$((40+2))`.
    read_until(&mut sock, &format!("bantu-42 {} {}", ws.port_base, ws.name)).await;

    // A second viewer gets the scrollback, so reopening a tab loses nothing.
    let (mut second, _) = tokio_tungstenite::connect_async(ws_url(&term_id))
        .await
        .unwrap();
    read_until(&mut second, "bantu-42").await;

    // Typing `exit` ends the shell and marks the terminal exited.
    sock.send(Message::Binary(b"exit 3\n".to_vec().into()))
        .await
        .unwrap();
    for _ in 0..100 {
        let t = h
            .state()
            .await
            .terminals
            .into_iter()
            .find(|t| t.id == term_id)
            .unwrap();
        if t.exited {
            assert_eq!(t.exit_code, Some(3));
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Without a run script, Run explains how to add one.
    let (status, body) = h
        .command(
            json!({"type": "workspace.run", "workspace_id": ws.id}),
            None,
        )
        .await;
    assert_eq!(status, 400, "{body}");

    // With one, Run starts it once and returns the same terminal while it runs.
    std::fs::write(
        Path::new(&ws.path).join(".bantu/settings.toml"),
        "[scripts]\nrun = \"echo serving-on-$BANTU_PORT; sleep 30\"\n",
    )
    .unwrap();
    let (status, body) = h
        .command(
            json!({"type": "workspace.run", "workspace_id": ws.id}),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["result"]["kind"], "run");
    let run_id = body["result"]["id"].as_str().unwrap().to_string();
    let (_, again) = h
        .command(
            json!({"type": "workspace.run", "workspace_id": ws.id}),
            None,
        )
        .await;
    assert_eq!(again["result"]["id"], json!(run_id));
    let (mut run_sock, _) = tokio_tungstenite::connect_async(ws_url(&run_id))
        .await
        .unwrap();
    read_until(&mut run_sock, &format!("serving-on-{}", ws.port_base)).await;

    // Closing stops it and removes it from the snapshot.
    let (status, _) = h
        .command(
            json!({"type": "terminal.close", "terminal_id": run_id}),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert!(h.state().await.terminals.iter().all(|t| t.id != run_id));
    assert!(tokio_tungstenite::connect_async(ws_url(&run_id))
        .await
        .is_err());
}

#[tokio::test]
async fn rewind_restores_files() {
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
    let ws = h.wait_for(&ws.id, WorkspaceState::Ready).await;
    let path = Path::new(&ws.path);
    let session = loop {
        if let Some(s) = h
            .state()
            .await
            .sessions
            .into_iter()
            .find(|s| s.workspace_id == ws.id)
        {
            break s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let send = |text: &str| json!({"type": "session.send", "session_id": session.id, "text": text});

    std::fs::write(path.join("mine.txt"), "user's own untracked file\n").unwrap();
    h.command(send("please edit things"), None).await;
    h.wait_session(&session.id, |s| {
        s.status == SessionStatus::Idle && s.turns == 1
    })
    .await;
    assert!(path.join("agent-file.txt").exists());

    let (status, body) = h
        .command(
            json!({"type": "session.rewind", "session_id": session.id, "turn": 1}),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(!path.join("agent-file.txt").exists());
    assert_eq!(
        std::fs::read_to_string(path.join("README.md")).unwrap(),
        "hi\n"
    );
    assert!(
        path.join("mine.txt").exists(),
        "pre-turn untracked files come back too"
    );
    let items = chat(&h.history(&ws.id).await, &session.id);
    assert_eq!(items.last(), Some(&ChatItem::Rewound { to_turn: 1 }));

    // The agent hears about the rewind on its next turn.
    h.command(send("carry on"), None).await;
    h.wait_session(&session.id, |s| {
        s.status == SessionStatus::Idle && s.turns == 2
    })
    .await;
    let items = chat(&h.history(&ws.id).await, &session.id);
    let last_user = items.iter().rev().find_map(|i| match i {
        ChatItem::User { text } => Some(text.clone()),
        _ => None,
    });
    let last_user = last_user.unwrap();
    assert!(
        last_user.starts_with("[Bantu] The user restored"),
        "{last_user}"
    );
    assert!(last_user.ends_with("carry on"));

    let (status, _) = h
        .command(
            json!({"type": "session.rewind", "session_id": session.id, "turn": 9}),
            None,
        )
        .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn permission_prompts() {
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
    let session = loop {
        if let Some(s) = h
            .state()
            .await
            .sessions
            .into_iter()
            .find(|s| s.workspace_id == ws.id)
        {
            break s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let send = |text: &str| json!({"type": "session.send", "session_id": session.id, "text": text});
    let wait_request = || async {
        for _ in 0..200 {
            if let Some(r) = h.state().await.permissions.into_iter().next() {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("no permission request arrived");
    };

    // The agent's tool call waits for the user; allowing lets it proceed.
    h.command(send("needs permission"), None).await;
    let req = wait_request().await;
    assert_eq!(req.tool_name, "Bash");
    assert_eq!(req.tool_input["command"], "rm -rf build");
    assert_eq!(req.session_id, session.id);
    let (status, _) = h
        .command(json!({"type": "permission.respond", "request_id": req.id, "allow": true, "message": null}), None)
        .await;
    assert_eq!(status, 200);
    h.wait_session(&session.id, |s| {
        s.status == SessionStatus::Idle && s.turns == 1
    })
    .await;
    assert!(h.state().await.permissions.is_empty());
    let items = chat(&h.history(&ws.id).await, &session.id);
    assert!(items.contains(&ChatItem::Assistant {
        text: "echo: decision allow".into()
    }));

    // Answering twice is a clear 404, not a crash.
    let (status, _) = h
        .command(json!({"type": "permission.respond", "request_id": req.id, "allow": false, "message": null}), None)
        .await;
    assert_eq!(status, 404);

    // Stopping the agent denies whatever it was waiting on.
    h.command(send("permission again"), None).await;
    wait_request().await;
    h.command(
        json!({"type": "session.cancel", "session_id": session.id}),
        None,
    )
    .await;
    h.wait_session(&session.id, |s| s.status == SessionStatus::Idle)
        .await;
    assert!(h.state().await.permissions.is_empty());

    // The hook endpoint only accepts the hook token.
    let res = h
        .http
        .post(format!(
            "{}/v1/hooks/permission?session={}",
            h.base, session.id
        ))
        .bearer_auth(&h.token)
        .json(&json!({"tool_name": "Bash", "tool_input": {}}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn streams_text_deltas_live() {
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
    let session = loop {
        if let Some(s) = h
            .state()
            .await
            .sessions
            .into_iter()
            .find(|s| s.workspace_id == ws.id)
        {
            break s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };

    // Subscribe first: deltas are only delivered to connected clients.
    let since = h.state().await.last_seq;
    let res = h
        .http
        .get(format!(
            "{}/v1/events?since={since}&token={}",
            h.base, h.token
        ))
        .send()
        .await
        .unwrap();
    let mut stream = res.bytes_stream();
    h.command(
        json!({"type": "session.send", "session_id": session.id, "text": "stream it"}),
        None,
    )
    .await;

    let mut buf = String::new();
    let mut deltas = Vec::new();
    while deltas.len() < 2 {
        let chunk = tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .expect("timed out waiting for deltas")
            .unwrap()
            .unwrap();
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buf.find("\n\n") {
            let frame: String = buf.drain(..end + 2).collect();
            if frame.lines().any(|l| l == "event: delta") {
                let data = frame.lines().find_map(|l| l.strip_prefix("data:")).unwrap();
                let d: Value = serde_json::from_str(data.trim()).unwrap();
                assert_eq!(d["session_id"], json!(session.id));
                assert!(
                    frame.lines().all(|l| !l.starts_with("id:")),
                    "deltas carry no seq"
                );
                deltas.push(d["text"].as_str().unwrap().to_string());
            }
        }
    }
    assert_eq!(deltas.concat(), "echo: ");

    // Deltas never enter the log; the full text arrives as a normal item.
    h.wait_session(&session.id, |s| {
        s.status == SessionStatus::Idle && s.turns == 1
    })
    .await;
    let items = chat(&h.history(&ws.id).await, &session.id);
    assert!(items.contains(&ChatItem::Assistant {
        text: "echo: stream it".into()
    }));
}

#[tokio::test]
async fn codex_sessions() {
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

    let (status, body) = h
        .command(
            json!({"type": "session.create", "workspace_id": ws.id, "agent": "codex", "model": null, "permission_mode": "plan"}),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["result"]["agent"], "codex");
    let id = body["result"]["id"].as_str().unwrap().to_string();
    let send = |text: &str| json!({"type": "session.send", "session_id": id, "text": text});

    h.command(send("first task"), None).await;
    h.wait_session(&id, |s| s.status == SessionStatus::Idle && s.turns == 1)
        .await;
    let items = chat(&h.history(&ws.id).await, &id);
    assert!(matches!(&items[1], ChatItem::ToolUse { name, .. } if name == "Bash"));
    assert!(matches!(&items[2], ChatItem::ToolResult { content, .. } if content == "README.md"));
    assert_eq!(
        items[3],
        ChatItem::Assistant {
            text: "codex: first task".into()
        }
    );
    assert!(matches!(
        items[4],
        ChatItem::TurnEnd {
            is_error: false,
            input_tokens: Some(10),
            ..
        }
    ));

    // The second turn resumes the thread Codex reported.
    h.command(send("second task"), None).await;
    h.wait_session(&id, |s| s.status == SessionStatus::Idle && s.turns == 2)
        .await;
    let calls = h.codex_calls();
    assert!(
        calls[0].starts_with("exec --json --sandbox read-only -"),
        "{}",
        calls[0]
    );
    assert!(
        calls[0].contains("| You are running inside Bantu"),
        "first turn is briefed: {}",
        calls[0]
    );
    assert!(
        calls[1].starts_with("exec resume th_fake --json"),
        "{}",
        calls[1]
    );
    assert!(calls[1].ends_with("| second task"), "{}", calls[1]);
}

#[tokio::test]
async fn workspaces_from_existing_branches() {
    let h = Harness::start().await;
    let (_fixture, clone) = fixture();
    // A branch that only exists on origin, and one that only exists locally.
    sh(&clone, &["checkout", "-q", "-b", "feature/remote"]);
    std::fs::write(clone.join("remote.txt"), "from origin\n").unwrap();
    sh(&clone, &["add", "remote.txt"]);
    sh(&clone, &["commit", "-qm", "remote work"]);
    sh(&clone, &["push", "-q", "origin", "feature/remote"]);
    sh(&clone, &["checkout", "-q", "main"]);
    sh(&clone, &["branch", "-q", "-D", "feature/remote"]);
    sh(&clone, &["branch", "-q", "local-only"]);

    let (_, body) = h
        .command(json!({"type": "repo.add", "path": clone}), None)
        .await;
    let repo_id = body["result"]["id"].as_str().unwrap().to_string();
    let create =
        |from: Value| json!({"type": "workspace.create", "repo_id": repo_id, "from": from});

    let (status, body) = h
        .command(
            create(json!({"kind": "branch", "branch": "feature/remote"})),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let ws = h
        .wait_for(
            body["result"]["id"].as_str().unwrap(),
            WorkspaceState::Ready,
        )
        .await;
    assert_eq!(ws.branch, "feature/remote");
    assert!(Path::new(&ws.path).join("remote.txt").exists());
    let upstream = Proc::new("git")
        .arg("-C")
        .arg(&ws.path)
        .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&upstream.stdout).trim(),
        "origin/feature/remote"
    );

    let (status, body) = h
        .command(
            create(json!({"kind": "branch", "branch": "local-only"})),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let ws2 = h
        .wait_for(
            body["result"]["id"].as_str().unwrap(),
            WorkspaceState::Ready,
        )
        .await;
    assert_eq!(ws2.branch, "local-only");

    // A branch can only be checked out once; main is in the original clone.
    let (status, body) = h
        .command(create(json!({"kind": "branch", "branch": "main"})), None)
        .await;
    assert_eq!(status, 409, "{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("already checked out"));
    let (status, _) = h
        .command(
            create(json!({"kind": "branch", "branch": "bad..name"})),
            None,
        )
        .await;
    assert_eq!(status, 400);

    // The first message must not rename a branch the user chose.
    let session = loop {
        if let Some(s) = h
            .state()
            .await
            .sessions
            .into_iter()
            .find(|s| s.workspace_id == ws2.id)
        {
            break s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    h.command(
        json!({"type": "session.send", "session_id": session.id, "text": "rename me please"}),
        None,
    )
    .await;
    h.wait_session(&session.id, |s| {
        s.status == SessionStatus::Idle && s.turns == 1
    })
    .await;
    let after = h
        .state()
        .await
        .workspaces
        .into_iter()
        .find(|w| w.id == ws2.id)
        .unwrap();
    assert_eq!(after.branch, "local-only");
}

#[tokio::test]
async fn conflict_radar() {
    let h = Harness::start().await;
    let (_fixture, clone) = fixture();
    let (_, body) = h
        .command(json!({"type": "repo.add", "path": clone}), None)
        .await;
    let repo_id = body["result"]["id"].as_str().unwrap().to_string();
    let mut ws = Vec::new();
    for _ in 0..2 {
        let (_, body) = h
            .command(
                json!({"type": "workspace.create", "repo_id": repo_id}),
                None,
            )
            .await;
        ws.push(
            h.wait_for(
                body["result"]["id"].as_str().unwrap(),
                WorkspaceState::Ready,
            )
            .await,
        );
    }
    let scan = || async {
        h.http
            .post(format!("{}/v1/repos/{repo_id}/conflicts", h.base))
            .bearer_auth(&h.token)
            .send()
            .await
            .unwrap()
            .json::<Vec<Value>>()
            .await
            .unwrap()
    };
    assert!(scan().await.is_empty());

    // Uncommitted edits to the same line in two workspaces collide.
    std::fs::write(Path::new(&ws[0].path).join("README.md"), "hello from one\n").unwrap();
    std::fs::write(Path::new(&ws[1].path).join("README.md"), "hello from two\n").unwrap();
    std::fs::write(Path::new(&ws[1].path).join("only-two.txt"), "fine\n").unwrap();
    let found = scan().await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0]["workspace_id"], json!(ws[0].id));
    assert_eq!(found[0]["other_workspace_id"], json!(ws[1].id));
    assert_eq!(found[0]["files"], json!(["README.md"]));
    assert_eq!(h.state().await.conflicts.len(), 1);

    // Resolving it clears the radar and says so on the event stream.
    std::fs::write(Path::new(&ws[1].path).join("README.md"), "hi\n").unwrap();
    assert!(scan().await.is_empty());
    assert!(h.state().await.conflicts.is_empty());
    let updates: Vec<usize> = h
        .events(0, h.state().await.last_seq as usize)
        .await
        .into_iter()
        .filter_map(|e| match e.data {
            EventData::ConflictsUpdated { conflicts, .. } => Some(conflicts.len()),
            _ => None,
        })
        .collect();
    assert_eq!(updates.last(), Some(&0));
    assert!(updates.contains(&1));
}

#[tokio::test]
async fn activity_tracks_clients() {
    let h = Harness::start().await;
    let activity = || async {
        h.http
            .get(format!("{}/v1/activity", h.base))
            .bearer_auth(&h.token)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()
    };
    let a = activity().await;
    assert_eq!(a["busy"], false);
    assert_eq!(a["connected_clients"], 0);

    // An open event stream (a browser tab) keeps the daemon busy.
    let stream = h
        .http
        .get(format!("{}/v1/events?since=0&token={}", h.base, h.token))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let a = activity().await;
    assert_eq!(a["busy"], true);
    assert_eq!(a["connected_clients"], 1);
    assert_eq!(a["idle_seconds"], 0);

    // Closing it is noticed without waiting for another event.
    drop(stream);
    for _ in 0..50 {
        if activity().await["connected_clients"] == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let a = activity().await;
    assert_eq!(a["connected_clients"], 0);
    assert_eq!(a["busy"], false);
}

#[tokio::test]
async fn mcp_orchestration_tools() {
    let h = Harness::start().await;
    let (_fixture, clone) = fixture();
    let (_, body) = h
        .command(json!({"type": "repo.add", "path": clone}), None)
        .await;
    let repo_id = body["result"]["id"].as_str().unwrap().to_string();
    let (_, body) = h
        .command(
            json!({"type": "workspace.create", "repo_id": repo_id, "name": "lead"}),
            None,
        )
        .await;
    let lead = h
        .wait_for(
            body["result"]["id"].as_str().unwrap(),
            WorkspaceState::Ready,
        )
        .await;
    let session = loop {
        if let Some(s) = h
            .state()
            .await
            .sessions
            .into_iter()
            .find(|s| s.workspace_id == lead.id)
        {
            break s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    h.command(
        json!({"type": "session.update", "session_id": session.id, "model": "haiku", "permission_mode": "accept_edits"}),
        None,
    )
    .await;

    let mut next_id = 0;
    let mut rpc = |method: &str, params: Value| {
        next_id += 1;
        let body = json!({"jsonrpc": "2.0", "id": next_id, "method": method, "params": params});
        let req = h
            .http
            .post(format!("{}/v1/mcp?session={}", h.base, session.id))
            .bearer_auth(&h.hook_token)
            .json(&body);
        async move { req.send().await.unwrap().json::<Value>().await.unwrap() }
    };
    let call = |r: Value| {
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };

    let init = rpc("initialize", json!({"protocolVersion": "2025-06-18"})).await;
    assert_eq!(init["result"]["serverInfo"]["name"], "bantu");
    let tools = rpc("tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "list_workspaces",
            "create_workspace",
            "workspace_status",
            "wait_for_workspace",
            "send_message",
            "get_diff"
        ]
    );

    // The lead agent fans work out to a new workspace and waits for it.
    let created = call(
        rpc(
            "tools/call",
            json!({"name": "create_workspace", "arguments": {"name": "helper", "task": "edit the readme"}}),
        )
        .await,
    );
    assert!(created.starts_with("Started workspace helper"), "{created}");
    let report = call(
        rpc(
            "tools/call",
            json!({"name": "wait_for_workspace", "arguments": {"workspace": "helper"}}),
        )
        .await,
    );
    assert!(report.contains("agent idle"), "{report}");
    assert!(
        report.contains("echo: edit the readme"),
        "the child's reply is reported: {report}"
    );
    assert!(report.contains("Changes: 2 file(s)"), "{report}");

    // The child inherited the lead's model and permission mode.
    let child = h
        .state()
        .await
        .sessions
        .into_iter()
        .find(|s| s.workspace_id != lead.id)
        .unwrap();
    assert_eq!(child.model.as_deref(), Some("haiku"));
    assert_eq!(child.permission_mode.as_str(), "accept_edits");

    let listed = call(
        rpc(
            "tools/call",
            json!({"name": "list_workspaces", "arguments": {}}),
        )
        .await,
    );
    assert!(
        listed.contains("- lead (this one)") && listed.contains("- helper"),
        "{listed}"
    );
    let diff = call(
        rpc(
            "tools/call",
            json!({"name": "get_diff", "arguments": {"workspace": "helper"}}),
        )
        .await,
    );
    assert!(diff.contains("agent-was-here"), "{diff}");

    // Mistakes come back as tool errors the agent can read, not crashes.
    let bad = rpc(
        "tools/call",
        json!({"name": "workspace_status", "arguments": {"workspace": "nope"}}),
    )
    .await;
    assert_eq!(bad["result"]["isError"], true);

    // Only the hook token opens the MCP endpoint.
    let res = h
        .http
        .post(format!("{}/v1/mcp?session={}", h.base, session.id))
        .bearer_auth(&h.token)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}
