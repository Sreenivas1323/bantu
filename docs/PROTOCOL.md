# Bantu protocol v1

Every Bantu client talks to `bantud` through this protocol: the desktop app, the web/PWA, the CLI, and agents calling over MCP. It is the same locally and remotely.

- **Commands** go in as `POST /v1/commands`.
- **State** comes out as an append-only, resumable event stream: `GET /v1/events` (Server-Sent Events).
- `GET /v1/state` returns a snapshot so a client can render at once, then follow events from `last_seq`.
- A few read-only endpoints compute things on demand (history, diff, PR status), and terminals use a WebSocket.

Wire types are defined in Rust in [`crates/protocol/src/lib.rs`](../crates/protocol/src/lib.rs).
`cargo test -p bantu-protocol` generates TypeScript bindings into `packages/protocol/src/generated/`. If this document and the Rust types disagree, the Rust types win; please fix this document.

`PROTOCOL_VERSION` is `1`. Breaking changes bump it. Additive changes (new optional fields, new command or event types) don't, so clients must ignore unknown event types and unknown fields.

## Transport and auth

| | |
|---|---|
| Default address | `127.0.0.1:47800` (`--host`, `--port`). Non-loopback hosts are refused unless `--allow-remote` is passed; expose a daemon remotely through Tailscale Serve, Cloudflare Tunnel or CloudFront instead. Requests whose `Host` isn't loopback are rejected too (DNS-rebinding protection). |
| Data dir | `~/.bantu` (override with `BANTU_HOME`): `bantu.db` (SQLite) and `local-token` (mode 0600); `org.token` and `org-claude-env.json` (mode 0600) while in an [organization](#organizations-bantu-connect) |
| Auth | `Authorization: Bearer <token>`, or `?token=<token>` on the query string (needed for `EventSource` and browser WebSockets, which can't set headers) |
| Token | `bantud --print-token` prints it |
| Web app | The daemon serves the built web app at `/` (`--web-dir` or `BANTU_WEB_DIR`) |

All bodies are JSON (`content-type: application/json`). Errors are JSON too: `{ "ok": false, "result": null, "error": "…" }`.

## Endpoints

| Method and path | Auth | Returns |
|---|---|---|
| `GET /v1/health` | none | `Health` |
| `GET /v1/state` | token | `StateSnapshot` |
| `POST /v1/commands` | token | `CommandResponse` |
| `GET /v1/events?since=<seq>` | token | SSE stream of `Event` (plus `delta` frames) |
| `GET /v1/workspaces/{id}/history?limit=<n>` | token | `Event[]`, oldest first |
| `GET /v1/workspaces/{id}/diff` | token | `WorkspaceDiff` |
| `GET /v1/workspaces/{id}/pr` | token | `PullRequest` or `null` |
| `GET /v1/workspaces/{id}/checks` | token | `WorkspaceChecks` |
| `GET /v1/workspaces/{id}/ports` | token | `ForwardedPort[]`, the servers listening on the workspace's ports |
| `GET /v1/workspaces/{id}/children` | token | `ChildWorkspace[]`, the workspaces its agent started |
| `GET /v1/tasks` | token | `Task[]`, open tasks from every connected tracker, newest first |
| `GET /v1/agents` | token | `AgentStatus[]`, every agent Bantu knows, in `AgentKind` order; see [Agents](#agents) |
| `GET /v1/push/key` | token | `{ public_key }`, the VAPID key browsers subscribe with |
| `GET /v1/terminals/{id}/ws` | token | WebSocket of raw PTY bytes |
| `POST /v1/hooks/permission?session=<id>` | hook token | Claude Code hook response (internal) |

### `GET /v1/health`
```json
{ "ok": true, "protocol_version": 1, "daemon_version": "0.0.1" }
```

### `GET /v1/state` → `StateSnapshot`
```json
{
  "protocol_version": 1,
  "daemon_version": "0.0.1",
  "last_seq": 42,
  "repos": [Repo, ...],
  "workspaces": [Workspace, ...],
  "sessions": [Session, ...],
  "comments": [DiffComment, ...],
  "terminals": [Terminal, ...],
  "permissions": [PermissionRequest, ...],
  "inbox": [InboxItem, ...],
  "trackers": [Tracker, ...],
  "org": Org | null,
  "org_ended": string | null
}
```
Render this, then open `/v1/events?since=42`. The snapshot carries no logs or transcripts; fetch those per workspace from `/history`. `terminals`, `permissions` and `inbox` live in the daemon's memory and are empty after a restart.

### `POST /v1/commands` → `CommandResponse`
The body is one `Command`. An optional `Idempotency-Key` header makes retries safe. The daemon stores the response for 24 hours and replays it for the same key without running the command again. Server errors (5xx) aren't stored, so they can be retried. Clients on flaky networks (phones, CloudFront) should always send one, e.g. a UUID per user action.

```json
{ "ok": true, "result": { ... }, "error": null }
{ "ok": false, "result": null, "error": "no repository r_123" }
```
Status codes:

| Code | Meaning |
|---|---|
| 200 | Success |
| 400 | Bad input |
| 404 | Unknown id |
| 409 | Wrong state, e.g. workspace not ready or agent running |
| 500 | Daemon error |

`result` is command-specific (see below). Long-running work, such as creating a worktree or running an agent turn, finishes asynchronously: the command returns right away and progress arrives as events.

### `GET /v1/events?since=<seq>` → SSE
- Replays every stored event with `seq > since` from the database, then streams live events. `Last-Event-ID` is honored too.
- Each stored event is an SSE message with `id: <seq>` and `data: <Event JSON>`.
- `seq` is monotonic and gap-free per daemon. To resume after a disconnect, reconnect with `since=<last seq you processed>`; a gap means you missed something, so resync from `/v1/state`.
- A comment line is sent every 15 seconds as a keepalive.
- `since` defaults to `0` (full replay).

```
id: 43
data: {"seq":43,"ts":"2026-11-01T10:00:00Z","data":{"type":"workspace.updated","workspace":{...}}}
```

**Live text deltas.** While an agent writes, the stream also carries frames named `delta` with a `StreamDelta` payload and **no `id`**:
```
event: delta
data: {"session_id":"s1","workspace_id":"w1","turn":3,"text":"Riv"}
```
Deltas are display-only. They are never stored or replayed and don't advance `seq`. The complete text always arrives afterwards as a `session.item` with an `assistant` item, which replaces the preview. With `EventSource`, listen for them with `addEventListener("delta", …)`; `onmessage` only receives stored events.

### `GET /v1/workspaces/{id}/history?limit=<n>`
The most recent `n` events for one workspace (default 2000, max 10000), oldest first. That covers its logs, chat items, session, comment, terminal and permission events. Clients call this when opening a workspace, merge by `seq`, and rely on the live stream after that.

### `GET /v1/workspaces/{id}/diff` → `WorkspaceDiff`
Everything the branch changed relative to its merge-base with `base_ref`: commits, uncommitted edits and untracked (non-ignored) files. It's computed with a throwaway index, so the user's staging area is never touched.

### `GET /v1/workspaces/{id}/pr` → `PullRequest | null`
The branch's pull request and a summary of its checks, from `gh pr view`. It is `null` when there's no PR, no `origin` remote, or `gh` isn't installed.

### `GET /v1/workspaces/{id}/checks` → `WorkspaceChecks`
`{ workspace_id, pull_request, checks: CheckRun[], deployments: Deployment[], comments: PrComment[], todos: TodoItem[] }`, for the Checks tab. GitHub data comes from `gh` and is empty when `gh` is missing or the branch has no PR. `todos` always includes the agent's latest todo list.

- `CheckRun`: `{ name, workflow, bucket, link, description }`, where `bucket` is `pass`, `fail`, `pending`, `skipping` or `cancel`.
- `Deployment`: `{ environment, state, url, created_at }`, the newest deployment per environment (up to 5).
- `PrComment`: `{ author, body, path, line, url, created_at }`, for conversation comments, review summaries and line comments.
- `TodoItem`: `{ text, done, source }`, where `source` is `agent` or `pull_request` (task list items in the PR description).

### `GET /v1/terminals/{id}/ws`
A WebSocket attached to a PTY.
- **Binary frames** carry raw bytes both ways: keystrokes in, terminal output out.
- **Text frames** carry JSON control messages: `{"type":"resize","cols":120,"rows":40}`, or `{"type":"input","data":"ls\n"}` as an alternative to binary input.
- On connect the server first sends the terminal's scrollback (up to 512 KB), so reopening a tab loses nothing.
- Any number of viewers can attach. Closing the socket doesn't stop the process; `terminal.close` does.

### `POST /v1/hooks/permission?session=<id>` (internal)
This is Claude Code's `PermissionRequest` HTTP hook (see [Agent sessions](#agent-sessions)). It is authenticated with a per-process **hook token**, not the client token. It blocks until the user answers and returns Claude's hook response. Clients never call it.

## Types

### `Repo`
| Field | Type | Notes |
|---|---|---|
| `id` | string | |
| `name` | string | Directory name of the repo root |
| `root_path` | string | Absolute path to the main checkout |
| `default_branch` | string | e.g. `main` |
| `remote_url` | string \| null | `origin` URL, if any |
| `github_account` | string \| null | The signed-in `gh` login this repo uses when the active one can't see it; see [GitHub logins](#github-logins) |
| `created_at` | string | RFC 3339 |

### `Workspace`
| Field | Type | Notes |
|---|---|---|
| `id` | string | |
| `repo_id` | string | |
| `name` | string | Human name; by default an unused city (`lisbon`) |
| `branch` | string | `bantu/<name>` at first; renamed after the task on the first message (see below) |
| `base_ref` | string | What the branch was created from, and the PR target, e.g. `origin/main` |
| `path` | string | `~/bantu/workspaces/<repo>/<name>` (canonical, e.g. `/private/tmp/…` on macOS) |
| `port_base` | number | First of 10 reserved ports (`BANTU_PORT` .. `BANTU_PORT+9`), allocated from 55000 in steps of 10 |
| `state` | `WorkspaceState` | `"creating" \| "ready" \| "failed" \| "archiving" \| "archived"` |
| `error` | string \| null | Set when `state` is `failed` |
| `archive_commit` | string \| null | Commit holding uncommitted tracked changes at archive time |
| `created_at` | string | RFC 3339 |
| `parent_id` | string \| null | The workspace whose agent started this one |
| `loop_id` | string \| null | The loop this workspace is a run of |
| `external` | boolean | Another tool owns the worktree (it was imported), so archiving leaves the files alone |
| `review_state` | `"reviewing" \| "changes_requested" \| "approved"` \| null | Where automatic review has got to |
| `task` | `TaskRef` \| null | `{ tracker_id, id, key, title, url }`: the tracker task it was started from |

### `Session`
One chat with an agent in a workspace. Every workspace gets one automatically when it becomes ready.

| Field | Type | Notes |
|---|---|---|
| `id` | string | |
| `workspace_id` | string | |
| `agent` | `AgentKind` | Fixed when the chat is created; see [Agents](#agents) |
| `title` | string | `"New chat"` until the first message, which becomes the title |
| `model` | string \| null | `null` uses the agent's default (e.g. `opus`, `sonnet`, `haiku`, or a full model id) |
| `permission_mode` | `PermissionMode` | See below |
| `effort` | string \| null | `low`, `medium`, `high`, `xhigh` or `max`; `null` uses the agent's default. Claude and Codex only (Codex treats `max` as `xhigh`) |
| `fast` | boolean | Claude's fast mode |
| `browser` | boolean | The agent gets a headless browser limited to the workspace's ports; see [Browser](#browser). New chats start with the repo's `[browser] enabled` |
| `forked_from` | string \| null | The chat this one was forked from |
| `status` | `"idle" \| "running"` | |
| `queued` | number | Messages waiting for the current turn to finish |
| `turns` | number | Turns started so far |
| `total_cost_usd` | number | Sum of the agent's reported turn costs |
| `created_at` | string | RFC 3339 |

### `PermissionMode`
| Value | Claude mode | Meaning |
|---|---|---|
| `auto` (default) | `auto` | A classifier approves safe actions; anything it won't approve becomes a `PermissionRequest` |
| `ask` | `default` | Ask in Bantu before edits, commands and other risky actions |
| `accept_edits` | `acceptEdits` | File edits are allowed; other risky actions ask |
| `plan` | `plan` | Read-only planning; the agent proposes instead of changing files |
| `bypass_permissions` | `bypassPermissions` | Everything is allowed. Only for trusted repos. |

### `ChatItem`
One transcript entry, normalized across agents and tagged by `kind`:

| `kind` | Fields | Notes |
|---|---|---|
| `user` | `text` | What was sent to the agent (may include a Bantu note after a rewind) |
| `assistant` | `text` | Markdown |
| `thinking` | `text` | Only when the model's thinking is visible |
| `tool_use` | `id`, `name`, `input` | e.g. `Bash {command}`, `Edit {file_path, …}` |
| `tool_result` | `tool_use_id`, `content`, `is_error` | Text output, truncated to 20,000 characters. Images (such as screenshots) become `[image]`; the agent still sees them |
| `turn_end` | `is_error`, `duration_ms`, `cost_usd`, `input_tokens`, `output_tokens` | `input_tokens` includes cache reads and writes |
| `rewound` | `to_turn` | Files were restored to before `to_turn` |
| `terminal_session` | `imported` | The chat continued in a terminal; `imported` entries read back from the agent's transcript come just before this one |
| `error` | `message` | Bantu-side problems: the agent couldn't start, crashed, or was stopped (`"Stopped"`) |
| `notice` | `message` | Something to know that didn't stop the turn, e.g. why a chat with the browser on has none |

### `StreamDelta`
`{ session_id, workspace_id, turn, text }`: a fragment of assistant text, only on the live stream (see above).

### `PermissionRequest`
`{ id, session_id, workspace_id, tool_name, tool_input, created_at }`: an agent waiting for the user to allow or deny a tool call.

### `WorkspaceDiff` and `FileDiff`
`WorkspaceDiff`:

| Field | Type | Notes |
|---|---|---|
| `workspace_id`, `base_ref`, `merge_base` | string | |
| `ahead` | number | Commits on the branch that aren't on `base_ref` |
| `behind` | number | Commits on `base_ref` that the branch doesn't have |
| `uncommitted` | number | Files with uncommitted changes, tracked or untracked |
| `files` | `FileDiff[]` | |

`FileDiff`:

| Field | Type | Notes |
|---|---|---|
| `path` | string | |
| `old_path` | string \| null | Set for renames |
| `status` | `"added" \| "modified" \| "deleted" \| "renamed"` | |
| `additions`, `deletions` | number | |
| `binary` | boolean | |
| `patch` | string | Unified diff hunks from the first `@@` |
| `truncated` | boolean | The patch was cut at about 200 KB |

### `DiffComment`
`{ id, workspace_id, path, line, body, created_at, sent_at }`
- `line` is a line number in the new version of the file.
- `sent_at` is `null` while the comment is a draft.

### `PullRequest`
`{ number, url, title, state, is_draft, review_decision, checks_passing, checks_failing, checks_pending }`
- `state` is `OPEN`, `CLOSED` or `MERGED`.
- `review_decision` is `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED` or `null`.

### `Terminal`
`{ id, workspace_id, kind, title, command, session_id, exited, exit_code, created_at }`
- `kind` is `"shell"` (an interactive login shell), `"run"` (the repo's run script, with its `command`), `"login"` (an agent's sign-in flow) or `"agent"` (any other CLI, or a chat continued in a terminal).
- `session_id` is the chat this terminal continues, if any.

### `InboxItem`
`{ id, workspace_id, session_id, kind, title, body, options, created_at }`: a notice or question an agent sent with the `notify_user` or `ask_user` MCP tool.
- `kind` is `"notice"` or `"question"`. A question's agent is waiting for an answer; `options` are suggestions, and the user can type their own.
- Items live in the daemon's memory and are empty after a restart. Stopping a chat releases its questions.

### `Spotlight`
`{ repo_id, workspace_id, restore_to, commit, synced_at }`: a workspace shown in its repo's main checkout. `restore_to` is where the checkout returns afterwards. Listed in `StateSnapshot.spotlights`; changes arrive as `spotlight.updated`.

### `LogStream`
`"stdout" | "stderr" | "system"`

## Commands

Commands are tagged by `type`. Optional fields may be `null` or omitted.

| `type` | Fields | `result` | What it does |
|---|---|---|---|
| `repo.add` | `path` | `Repo` | Registers a local git repo. Any path inside it works; adding the same repo again returns the existing one. |
| `repo.remove` | `repo_id` | — | Forgets a repo. Fails with 409 while it has unarchived workspaces. Files on disk are untouched. |
| `workspace.create` | `repo_id`, `name?`, `base_ref?` | `Workspace` (`creating`) | Creates a worktree and branch; see below. |
| `workspace.archive` | `workspace_id` | `Workspace` (`archiving`) | Stops its agents and terminals, then archives; see below. |
| `workspace.run` | `workspace_id` | `Terminal` | Starts the repo's run script in a `run` terminal, or returns the one already running. 400 if there's no run script. |
| `repo.set_github_account` | `repo_id`, `account?` | `Repo` | Uses this signed-in `gh` login for the repo; see [GitHub logins](#github-logins). 400 if it isn't signed in. |
| `workspace.integrate` | `workspace_id`, `session_id?` | `IntegrationResult` | Merges finished child workspaces into this one's branch; see below. 409 if the lead is busy or has uncommitted changes. |
| `workspace.spotlight` | `workspace_id` | `Spotlight` | Shows the workspace's work in the repo's main checkout; see below. 409 if the main checkout has uncommitted changes. |
| `repo.spotlight_stop` | `repo_id` | — | Returns the main checkout to its branch. 409 if it has uncommitted changes. |
| `workspace.create_pr` | `workspace_id`, `session_id` | `Session` | Asks the agent to commit, push and `gh pr create` against `base_ref`. 400 without an `origin` remote. |
| `workspace.fix_checks` | `workspace_id`, `session_id` | `Session` | Sends failing checks and their logs to the agent. |
| `workspace.address_comments` | `workspace_id`, `session_id` | `Session` | Sends the PR's comments to the agent. 400 without a PR or comments. |
| `workspace.resolve_conflicts` | `workspace_id`, `session_id` | `Session` | Asks the agent to resolve what the conflict radar found. 400 if there's nothing to resolve. |
| `workspace.rename_branch` | `workspace_id`, `session_id` | `Session` | Asks the agent to rename the branch. The daemon picks up the new name when the turn ends. |
| `workspace.review` | `workspace_id`, `agent?` | `Session` | Starts a read-only chat that reviews the workspace's changes. Without `agent`, the repo's `[agents] reviewer` (else Claude). |
| `session.create` | `workspace_id`, `agent?`, `role?`, `model?`, `permission_mode?`, `effort?`, `fast?` | `Session` | Opens another chat in a ready workspace. Without `agent`, the repo's agent for `role` (`worker`, `orchestrator` or `reviewer`; see [Repo configuration](#repo-configuration-bantusettingstoml)) is used, else Claude. `role` defaults to `orchestrator` in a workspace that has started others, and `worker` otherwise. 400 for a terminal-only agent. |
| `session.fork` | `session_id` | `Session` | Starts a new chat that continues from where this one is now; see below. 409 while a turn is running. |
| `session.send` | `session_id`, `text` | `Session` | Sends a message. If a turn is running it's queued (`queued` goes up) and runs afterwards, in order. |
| `session.cancel` | `session_id` | `Session` | Stops the running turn (it ends with an `error` item `"Stopped"`), drops queued messages, and denies pending permission requests. |
| `session.update` | `session_id`, `title?`, `model?`, `permission_mode?`, `effort?`, `fast?`, `browser?` | `Session` | Changes settings; they apply from the next turn. An empty `effort` goes back to the default. |
| `session.rewind` | `session_id`, `turn` | `Session` | Restores the workspace files to the checkpoint taken just before `turn` started; see below. 409 while the agent is running. |
| `permission.respond` | `request_id`, `allow`, `message?` | — | Answers a `PermissionRequest`. `message` tells the agent why it was denied. 404 if already answered. |
| `inbox.respond` | `item_id`, `answer` | — | Answers a question in the inbox. 404 if already handled. |
| `inbox.dismiss` | `item_id` | — | Clears a notice, or declines to answer a question. |
| `push.subscribe` | `endpoint`, `p256dh`, `auth` | — | Sends this browser alerts through Web Push. 400 unless the endpoint uses HTTPS. |
| `push.unsubscribe` | `endpoint` | — | |
| `loop.create` | `repo_id`, `name`, `trigger`, `prompt`, `agent?`, `model?`, `permission_mode?` | `Loop` | Creates a loop, enabled; see below. |
| `loop.update` | `loop_id`, `enabled?`, `prompt?` | `Loop` | Pauses, resumes or changes a loop. |
| `loop.delete` | `loop_id` | — | Existing runs' workspaces are kept. |
| `loop.run` | `loop_id` | `Workspace` | Starts a run now. 409 if the last run is still working or the repo has 10 active workspaces. |
| `tracker.add` | `repo_id`, `kind`, `site?`, `scope?`, `account?`, `token?` | `Tracker` | Connects a repo to GitHub Issues, GitLab, Jira or ClickUp; see [Tasks](#tasks). 400 if signing in or reading tasks fails. |
| `tracker.remove` | `tracker_id` | — | Workspaces started from it are kept. 409 for a tracker from the member's organization. |
| `tasks.refresh` | `tracker_id?` | `Task[]` | Fetches now instead of waiting for the next sync. |
| `task.start` | `tracker_id`, `task_id` | `Workspace` | A workspace for the task, with the agent given the task once it's ready. |
| `import.scan` | `source`, `path?` | `ImportPreview` | Shows what an import from `conductor` or `pane` would bring across. `path` overrides where the tool's database normally is. |
| `import.run` | `source`, `path?` | `ImportPreview` | Imports in the background; see below. 404 if the database isn't there. |
| `comment.add` | `workspace_id`, `path`, `line`, `body` | `DiffComment` | Adds a draft review comment. |
| `comment.delete` | `comment_id` | — | |
| `comments.send` | `workspace_id`, `session_id` | `Session` | Sends every draft comment to the agent as one message (sorted by file and line), then marks them sent. 400 if there are none. |
| `terminal.open` | `workspace_id`, `cols?`, `rows?`, `command?`, `agent?` | `Terminal` | Opens an interactive login shell in the worktree, or runs `command` (any CLI) there. With `agent` and no `command`, it runs that agent's own terminal UI (titled with its name); this is how terminal-only agents are used. |
| `session.open_terminal` | `session_id`, `cols?`, `rows?` | `Terminal` | Continues a Claude or Codex chat in the agent's own terminal UI; see below. Returns the existing terminal if it's already open. 409 while a turn is running. |
| `terminal.close` | `terminal_id` | — | Hangs up the terminal's whole process group and forgets it. |
| `org.join` | `link` | `Org` | Joins an organization with a Bantu Connect join link; see [Organizations](#organizations-bantu-connect). 400 for a bad, unknown or expired link, 409 if already in one. |
| `org.sync` | — | `Org` \| `null` | Fetches the org's settings now. `null` if there's no org, or the connector removed this machine. |
| `org.leave` | — | — | Leaves the org and removes its trackers and Claude settings. 400 if there's no org. |
| `preferences.update` | `claude_user_setup?`, `loadouts?` | `Preferences` | Changes daemon-wide preferences (also in `StateSnapshot.preferences`; changes arrive as `preferences.updated`). |

```json
{ "type": "session.send", "session_id": "s_123", "text": "Fix the flaky login test" }
```

### `workspace.create`
1. Fetches `origin`, if there is one.
2. Picks the name: by default an unused city name.
3. Picks the base: `base_ref` defaults to `origin/<default_branch>`, or the local default branch if there's no remote.
4. Runs `git worktree add -b bantu/<name> <path> <base_ref>`.
5. Copies gitignored files into the new worktree:
   - those matching the globs in the repo's `.worktreeinclude`;
   - `.env*` files, by default.
6. Runs the repo's setup script, if any. Its output streams as `workspace.log` events.
7. Ends in state `ready` with one idle `Session`, or in `failed` with `error` set.

### Loops

A `Loop` runs an agent on its own: `{ id, repo_id, name, trigger, prompt, agent, model, permission_mode, enabled, last_run_at, next_run_at, last_workspace_id, last_result, created_at }`. Listed in `StateSnapshot.loops`; changes arrive as `loop.updated` and `loop.deleted`.

`trigger` is tagged by `kind`:

| `kind` | Fields | When |
|---|---|---|
| `cron` | `schedule` | Five cron fields (`*`, `5`, `1-5`, `*/15`, lists) in the daemon's time zone, or `@hourly`, `@daily`, `@weekly`, `@monthly` |
| `github_issue` | `label` | A new open issue with that label, polled about once a minute with `gh` |
| `github_pull_request` | `label` | A new open PR with that label, not from a fork |

- **Each run gets a fresh workspace**, with `loop_id` set. A run that changes nothing is archived automatically. A failed one is kept, so you can see why.
- **One at a time.** A run is skipped if the last one is still working or the repo already has 10 active workspaces, and `last_result` says so.
- **Missed runs aren't caught up on.** If the daemon or machine was off at the scheduled time, the next run is the next one due.
- **GitHub triggers need a label**, because labels take triage rights while anyone can open an issue, and an issue's text goes into the agent's prompt. They refuse `bypass_permissions` and don't check out forks' PRs. The first poll records what's already open, so turning a loop on doesn't start a run for every old issue.
- **Alerts.** A finished loop run sends no push alert, but one that asks for approval still does.

### GitHub logins

Bantu runs `gh` for pull requests, checks and GitHub Issues. When a repo is added (and at startup, for repos added before), Bantu asks each signed-in `gh` login in turn, starting with the active one, whether it can see the repo's GitHub remote. If only another login can, it records that login as `Repo.github_account`. From then on, `gh` in the repo's checkout and workspaces runs with that login's token in `GH_TOKEN`, and the repo's agents, terminals and scripts get it too, so `gh pr create` and `git push` (through `gh auth setup-git`) act as that login. The active login is never changed. `repo.set_github_account { repo_id, account }` picks one by hand; `account: null` goes back to the active login. `repo.updated` reports the change.

### Pull requests and the board

`StateSnapshot.pull_requests` holds a `WorkspacePr` for each ready workspace whose branch has a pull request: `{ workspace_id, pull_request, mergeable, head_sha, updated_at, follow_ups, needs_you }`. The daemon fetches every repo's PRs with one `gh pr list` every 90 seconds, and sends `pr.updated` and `pr.removed` when they change. Without `gh`, or for a repo with no GitHub remote, there are none.

The app's board places each workspace in a lane from these facts, in this order: **Needs you** (setup failed, an approval or question waiting, or PR follow-up gave up), **Working** (an agent running or the workspace being set up), **In review** (an open PR that's a draft, conflicts, has CI running or failing, or waits for a review), **Ready to merge** (an open PR with CI passing and no review blocking it), and **Idle** (everything else, with the reason).

With this in `.bantu/settings.toml`, read from the main checkout like the other settings that make Bantu act on its own:

```toml
[pr]
follow_up = true
```

the daemon also sends the PR's agent what needs doing:

- **Conflicts** with the base branch: once per commit, first.
- **Failed CI**, with the failing jobs' logs, once the checks have finished: once per commit.
- **New review comments**, from anyone but the PR's author: once per batch.

It only does this while that workspace has no agent working and nothing waiting on you, and sends it to the newest chat that can edit files. After 3 automatic follow-ups without a message from you, it stops, sends an inbox notice, and the board shows the workspace under *Needs you*. Your next message in the workspace starts the count again. What's been handled is kept across restarts.

### Tasks

A repo can be connected to task trackers. `StateSnapshot.trackers` lists them (`Tracker`: `{ id, kind, repo_id, name, site, scope, account, created_at, last_synced_at, last_error, org_id }`), and `GET /v1/tasks` returns their open tasks. `org_id` is set on trackers that come from the member's [organization](#organizations-bantu-connect); members can't remove those.

| `kind` | Needs | `scope` |
|---|---|---|
| `github` | `gh`, signed in | A label, or empty for all open issues |
| `gitlab` | `token` (personal access token with the `api` scope); `site` only for self-managed GitLab (default `https://gitlab.com`) | The project's path (`group/project`, `group/sub/project`) or numeric id, optionally followed by a label: `acme/web ~agent`, or `acme/web ~"needs agent"` for a name with spaces. The `~` is optional; commas separate labels an issue must all have. |
| `jira` | `site` (`https://<you>.atlassian.net`), `account` (your email), `token` (API token) | A project key, or a JQL query |
| `clickup` | `token` (personal API token) | A list id |

GitLab support uses REST API v4 (`GET /user`, `GET /projects/:id/issues?state=opened`, `POST /projects/:id/issues/:iid/notes`, `GET /projects/:id/merge_requests?source_branch=…&state=opened`), with the token in the `PRIVATE-TOKEN` header. A `read_api` token can list issues but can't post notes, so it needs `api`. Keys are `#<iid>`, and `mine` means you're one of the issue's assignees. So far it's tested only against a local fake of the GitLab API, not a real instance.

- **Sync.** Open tasks are fetched every 3 minutes, and on `tasks.refresh`. A `tasks.changed` event says when a tracker's tasks changed; fetch `GET /v1/tasks` again. `Task.mine` is true when it's assigned to the person whose credentials Bantu uses.
- **Starting.** `task.start` creates a workspace on a branch named after the task (`bantu/web-12-fix-login`), names its chat after the task, and sends the agent the task. The description is marked as information written in the tracker, not instructions. `Workspace.task` records the link.
- **Reporting back.** The agent can post notes with the `update_task` MCP tool. Once the workspace's branch has a pull request, its link is posted to the task, once. For GitLab trackers that's the branch's open merge request, found through GitLab's API (other trackers find the pull request with `gh`), and the agent is asked to put `Closes #N` in the MR description; for GitHub it's `Fixes #N`.
- **Tokens** are stored in `$BANTU_HOME/trackers/<id>.token`, readable only by the user, and are never in the event log, the snapshot or any response. HTTP goes through `curl` with its arguments on stdin, so tokens aren't in the process list. `site` must be HTTPS (plain HTTP only to `127.0.0.1`, for testing).

### Organizations (Bantu Connect)

A daemon can belong to one organization through its **Bantu Connect** service, and then follows the org's settings. `StateSnapshot.org` is the `Org`, or `null`; `org.updated` (`{ org, ended }`) says when it changes.

`Org`:

| Field | Type | Notes |
|---|---|---|
| `id`, `name`, `slug` | string | |
| `member` | `{ login, role }` | Your login, and `owner`, `admin` or `member` |
| `connector` | string | The Connect service's address, from the join link |
| `joined_at` | string | RFC 3339 |
| `last_synced_at` | string \| null | The last good sync |
| `last_error` | string \| null | Why the last sync failed. The last known settings stay in place |
| `claude` | `{ mode, plan, env_keys }` | `mode` (`ClaudeAccessMode`) is `own`, `team`, `api_key`, `bedrock` or `vertex`; `plan` names the plan in `team` mode; `env_keys` are the names (never the values) of the variables Bantu sets on Claude Code |

`StateSnapshot.org_ended` (and `ended` in `org.updated`) says why the last membership ended when it wasn't the member's choice, e.g. after a 401. Joining again clears it.

- **Joining.** `org.join { link }` takes a join link, `https://<connector>/c/<code>`. The connector can sit under a path (`https://example.com/connect/c/<code>`). Links must be HTTPS; plain HTTP is accepted only to `127.0.0.1`, for testing. The daemon posts `{ code, name }` (`name` is the machine's hostname) to `<connector>/api/daemon/join`. A 400 or 410 (an unknown, expired or used code) is a 400 here with a plain message, and nothing is stored. 409 if the daemon is already in an org.
- **The daemon token** goes in `$BANTU_HOME/org.token` (mode 0600). It's sent only as `Authorization: Bearer` to the connector, through `curl` with its arguments on stdin, and is never in the event log, the snapshot or any response.
- **Sync.** `GET <connector>/api/daemon/sync` right after joining, every 5 minutes, on `org.sync`, and soon after `repo.add`. `org.sync` returns the `Org`, or `null` if the connector says this machine isn't a member any more.
- **Network errors and other failures** (anything but 2xx and 401) keep the last known trackers and Claude settings, and set `last_error`.
- **A 401** means the token was revoked or the member was removed. Bantu cleans up as for leaving and sets `org_ended`. It doesn't call `leave`.
- **Leaving.** `org.leave` posts to `<connector>/api/daemon/leave` (a failure there is logged, and the daemon leaves anyway), deletes the token, and removes the org's trackers (with their tokens and tasks) and Claude settings. Workspaces started from the org's tasks are kept. 400 if there's no org.

**Connections become trackers.** Each of the org's connections (`jira`, `clickup` or `gitlab`) becomes a `Tracker` with `org_id` set to the org's id, id `org-<connection id>-<repo id>`, and its token in the usual `$BANTU_HOME/trackers/<id>.token`. A connection attaches to the local repos its scope obviously points at:

- a repo whose folder name is the connection's name, a Jira project key (`WEB` → `web`), or a GitLab project's last segment (`acme/web` → `web`), ignoring case;
- for GitLab, a repo whose `origin` is that project (`git@gitlab.com:acme/web.git`).

With no match, it attaches to every repo, so nothing an admin connects is hidden. Repos added later are picked up by the next sync. `tracker.remove` refuses org trackers with 409 ("managed by your organization"); they go when an admin drops them from Connect (the next sync removes them) or the member leaves. A connection with a non-HTTPS `site` or an unusable id is skipped.

**Claude access.** In any mode but `own`, the org's `claude.env` is set on every Claude Code process Bantu starts: chat turns (including reviews and automatic review, which are chats) and every PTY terminal (shells, agent terminals and chats continued in a terminal, so `claude` run there gets it too). Only variables that configure Claude Code and its providers pass: names made of `A-Z`, `0-9` and `_` starting with `ANTHROPIC_`, `CLAUDE_CODE_`, `AWS_`, `CLOUD_ML_`, `GOOGLE_`, `VERTEX_REGION_`, `DISABLE_` or `OTEL_`. Others (such as `PATH`) are dropped, with a warning in the daemon log. The values are kept in `$BANTU_HOME/org-claude-env.json` (mode 0600) so they survive a restart. In `own` mode nothing is set. In `team` mode the app shows the plan's name and asks the member to sign in to Claude with their work account in the accounts dialog.

**Team totals.** After a good sync, if yesterday (UTC) hasn't been sent, the daemon posts `{ day: "YYYY-MM-DD", counts }` to `<connector>/api/daemon/totals`. That happens once a day, and on startup when a day was missed. `counts` are numbers only, computed from the local store:

| Count | From |
|---|---|
| `workspaces_started` | Workspaces created that day, not counting imported ones |
| `prs_opened` | Pull requests the board first saw that day (`pr.updated`) |
| `prs_merged` | Pull requests first seen merged that day |
| `checks_passed`, `checks_failed` | `check` chat items |
| `agent_turns` | `turn_end` chat items |
| `cost_usd` | The sum of their `cost_usd` |

No names, repos, branches or text are sent. Only the last day is sent; earlier missed days aren't caught up on.

So far this is tested only against a fake connector in the daemon's API tests, not the real Connect service.

### Importing from Conductor and Pane

`import.run` brings across a tool's active workspaces and chats, with `import.updated` events carrying an `ImportStatus` (`{ source, running, workspaces, chats, items, skipped, error }`).

- **The source isn't changed.** Its database is opened read-only (`~/Library/Application Support/com.conductor.app/conductor.db`, `~/.pane/sessions.db`), in `immutable` mode unless it has pending WAL data, so no sidecar files are created. These are private formats that can change between versions, so an unknown layout is reported as an error and nothing else happens.
- **Worktrees are adopted in place.** They get `external: true`, so archiving only forgets them: no scripts run and no files are removed. Each still gets its own reserved ports, and its branch stays as it is.
- **What comes across.** Active workspaces whose folders still exist and belong to the expected repo, on a branch (a detached HEAD is skipped). Archived ones don't. Each chat keeps its last 1,000 messages. Claude and Codex chats keep their conversation ids, so they can carry on. Chats with other agents, or with no conversation id, don't come across.
- **Importing again is safe.** A worktree that's already a Bantu workspace is skipped.

### `workspace.integrate`
A workspace created through the `create_workspace` MCP tool records its lead in `parent_id`. This command merges the lead's children with `git merge --no-edit`, in the lead's worktree, one at a time.

- A child is skipped, with a reason, if its agent is working, it has uncommitted files, it isn't ready, or it has nothing new.
- On the first conflict that merge is aborted, so the branch is never left half merged. With a `session_id`, that chat's agent is asked to merge the conflicting branch and the rest, and `sent_to_agent` is true.
- Merges are local. Nothing is pushed.

`IntegrationResult` is `{ merged, skipped: [{ workspace, reason }], conflict, sent_to_agent }`.

### `workspace.spotlight`
Tests a workspace in the repo's main checkout, where the user's own dev servers, `.env` files and tools already live.

1. Takes a snapshot of the workspace (commits, uncommitted and untracked files) and keeps it at `refs/bantu/spotlight/<repo id>`.
2. Runs `git checkout --detach <snapshot>` in the main checkout. Detaching means no branch moves, and the workspace's branch stays usable in its worktree.
3. Repeats after each agent turn in that workspace.

`repo.spotlight_stop` checks out the original branch again, and archiving the workspace does the same. The state is kept in the database, because the checkout stays that way across a daemon restart.

The main checkout belongs to the user, so nothing here overwrites it. Tracked changes or a merge in progress refuse the command with 409. If its `HEAD` isn't where Spotlight left it, because the user switched branch or committed there, automatic updates stop and `repo.spotlight_stop` only forgets the spotlight, without moving anything. If that happens during an automatic update, the update is skipped and the reason goes in the workspace log. Gitignored files are left alone.

### `workspace.archive`
1. Stops running agent turns and closes terminals.
2. Runs the archive script, if any. A failing archive script is logged and doesn't block archiving.
3. Saves uncommitted tracked changes with `git stash create` to `refs/bantu/archive/<id>` and records the commit as `archive_commit`. Untracked files are not saved.
4. Removes the worktree and the session checkpoint refs.
5. Keeps the branch.
6. Ends in state `archived`.

## Port previews

Open a workspace's dev server from any device, through the daemon.

- **`GET /v1/workspaces/{id}/ports`** checks the workspace's 10 reserved ports and returns `{ port, url }` for each one that's listening. `url` carries a grant for that port, valid for 5 minutes. It's `null` when the client is remote and `BANTU_PREVIEW_URL` isn't set.
- **Its own listener.** Previews are served on a separate port (`--preview-port`, by default the API port + 1), and only previews. A different port is a different browser origin, so a previewed page can't read the daemon's token from `localStorage`. The daemon's token isn't accepted there.
- **`/p/{workspace}/{port}/{path}`** forwards HTTP (streaming both ways) and WebSockets to `127.0.0.1:{port}`. Opening a grant link sets an `HttpOnly` cookie scoped to that path and removes the grant from the URL. Only the workspace's own ports can be reached (403 otherwise). Grants and cookies stop working when the daemon restarts.
- **Previews share an origin with each other.** That origin is separate from the daemon's, but a page in one preview could fetch another preview on the same daemon, so only preview apps you trust alongside each other. Apps' cookies are rescoped to their own preview's path, so they don't collide.
- **What's rewritten.** Redirects to `/path` or `http://localhost:{port}/path` stay inside the preview. A root-absolute request such as `/assets/app.js` is redirected back into its preview using the `Referer`. Paths are forwarded exactly as sent. Bantu's cookie and `Authorization` header aren't forwarded, and `Host` is set to `localhost:{port}`, since dev servers often reject other names. A WebSocket opened at a root path can't be redirected, so hot reload clients may need their dev server's base path set to `$BANTU_PREVIEW_BASE` (set in workspace terminals to `/p/{workspace}/{BANTU_PORT}/`). URLs an app builds from its own host, and third-party sign-in callbacks, may still need the app configured for its preview address.

## Web Push

The daemon sends alerts itself, with no server of ours in the path. It sends one when an agent asks for approval, sends a notice or question, or finishes.

- **Key**: an ES256 VAPID key generated on first start and stored in `<home>/vapid-key` (mode 0600).
- **Encryption**: RFC 8291 (`aes128gcm`), so the browser's push service only sees ciphertext. Payloads are JSON: `{ title, body, tag }`.
- **Sending**: a `POST` to the subscription's endpoint, using the system `curl`. Subscriptions that answer 404 or 410 are removed.
- **Subscribing**: fetch `GET /v1/push/key`, call `pushManager.subscribe`, then send `push.subscribe`. Web Push needs an installed PWA on iOS, and HTTPS everywhere except `localhost`.

## Agent sessions

Each turn runs the user's own `claude` CLI. The daemon doesn't use an SDK or its own API key: whatever Claude Code is logged in with is what the agent uses.

```
claude -p --output-format stream-json --verbose --include-partial-messages \
  --permission-prompts none --permission-mode <mode> \
  --session-id <uuid> | --resume <uuid> \
  --append-system-prompt <workspace context> [--model <m>] \
  [--setting-sources project,local --strict-mcp-config] \
  --settings <PermissionRequest hook JSON> --mcp-config <Bantu MCP server [+ browser]>
```

- **One process per turn.** The prompt goes in over stdin. The first turn uses `--session-id`; every later turn uses `--resume` with the same id, so the conversation continues. A daemon restart never strands a process: turns that were running are marked with an `error` item `"Interrupted by a daemon restart"`, and the next message simply resumes.
- **Output** is parsed line by line into `ChatItem`s (`session.item` events). Partial text becomes `delta` frames.
- **Workspace context** is appended to the system prompt: the workspace name, branch, base, and its reserved ports.
- **Lean context.** Unless `preferences.claude_user_setup` is on, turns skip the user's global Claude setup (`~/.claude/settings.json`, its plugins, hooks and skills, user MCP servers and `~/.claude/CLAUDE.md`). A trivial turn then sends about 17k input tokens instead of about 53k with a heavily customised setup. The repo's `CLAUDE.md`, `.claude/settings.json` and `.claude/settings.local.json` still load, and so do Bantu's hook and MCP server.
- **Environment**: `BANTU_PORT`, `BANTU_WORKSPACE_ID`, `BANTU_WORKSPACE_NAME`, `BANTU_WORKSPACE_PATH`, `BANTU_SESSION_ID`, plus the organization's Claude settings when its mode isn't `own` (see [Organizations](#organizations-bantu-connect)).
- **Permission prompts.** `--settings` installs a `PermissionRequest` HTTP hook pointing at `POST /v1/hooks/permission?session=<id>`.
  - The hook token travels in the `BANTU_HOOK_TOKEN` environment variable (referenced as `$BANTU_HOOK_TOKEN` in the hook's headers), never in argv.
  - The daemon publishes a `permission.requested` event and waits for `permission.respond`, a stop, or about 10 minutes, whichever comes first.
  - With nothing answering, Claude would deny the call; that's what `--permission-prompts none` would otherwise do.
- **Branch naming.** On the first message, a placeholder branch (`bantu/<city>`) that has never been pushed is renamed to a slug of the message, e.g. "Please fix the login redirect bug" → `bantu/fix-login-redirect-bug`. Duplicates get `-2`, `-3`, and so on.
- **Checkpoints.** Before each turn the daemon snapshots the worktree to `refs/bantu/cp/<session>/<turn>`: tracked and untracked (non-ignored) files, as a commit whose parent is `HEAD`. It uses a temporary index, so the user's index, branch and files are untouched. It's skipped while a merge or rebase is in progress.
- **Rewind.** `session.rewind` resets the branch to the checkpoint's parent, restores the snapshot's files (ignored files such as `node_modules` are left alone), and adds a `rewound` item. Claude can't rewind part of a conversation, so the next message is prefixed with a note telling the agent which turn the files went back to.

### Automatic review

With `[review] auto = true`, a chat that finishes work gets reviewed by a different agent, in a read-only chat titled "Automatic review". `Workspace.review_state` follows along.

1. **It starts** when a chat has no queued work left, isn't in plan mode, its last turn succeeded, the repo's `check` passed (if it has one), and the files differ from where the branch started. Identical work isn't reviewed twice.
2. **The reviewer** ends its reply with `VERDICT: APPROVED` or `VERDICT: CHANGES_REQUESTED`. Only the last line counts.
3. **Approved** sets `approved`, which means the work is ready.
4. **Changes requested** sets `changes_requested` and sends the findings to the author's chat. After 2 rounds it stops and sends an inbox notice instead, so two agents can't spend your quota arguing. A message from you starts the count again.
5. **No verdict**, for example because the reviewer's CLI isn't installed, clears the state and notes it in the workspace log.

Any new message to a chat clears `review_state`, because the review no longer describes the work.

### Forking a chat

`session.fork` creates a chat with a copy of the parent's transcript, settings and checkpoints, so rewinding to an earlier turn still works. The parent is untouched.

- **Claude** forks the real conversation: the fork's first turn runs `claude --resume <parent> --fork-session --session-id <fork>`.
- **Other agents** can't fork a conversation, so the fork starts a new one and its first message includes the last 20,000 characters of the transcript.

### Loadouts

`preferences.loadouts` is a saved list of `Loadout`: `{ name, agent, model, effort, fast, permission_mode }`. Clients offer these when starting a chat and pass the values to `session.create`. `preferences.update` replaces the whole list.

### Bantu's MCP tools

Every turn gets Bantu's MCP server (`POST /v1/mcp?session=<id>`, authenticated with the hook token). Tools act only on the caller's repository.

| Tool | What it does |
|---|---|
| `list_workspaces`, `workspace_status`, `wait_for_workspace` | See and wait for this repo's workspaces |
| `create_workspace`, `send_message`, `get_diff` | Start, message and review other workspaces' agents |
| `integrate_workspaces` | Merge the workspaces this one started into its branch |
| `update_task` | Post a progress note on the tracker task this workspace was started from |
| `list_terminals`, `get_terminal_output` | Read a workspace's terminals, such as dev server logs (ANSI codes stripped, last 2,000 lines at most) |
| `list_diff_comments`, `add_diff_comment` | Read review comments, and add draft ones for the user to send |
| `notify_user` | Send a notice to the inbox without waiting |
| `ask_user` | Ask a question and block until it's answered, dismissed or stopped, or the timeout passes (10–1800 s, default 600) |

A chat in plan mode can't use `create_workspace`, `send_message`, `integrate_workspaces` or `update_task`. A chat that doesn't bypass permissions can't `send_message` to one that does.

### Browser

A chat with `browser` on gives its agent a headless browser, so it can open its own app, click through it and take screenshots. It's off by default, because it adds about 25 tools to every turn and needs Node.js. Turn it on for one chat with the **Browser** button next to the model and effort pickers (`session.update` with `browser`), or for every new chat in a repo with `[browser] enabled = true` in `.bantu/settings.toml`. The repo setting only decides where new chats start; each chat keeps its own switch after that.

- **The server** is [Playwright MCP](https://github.com/microsoft/playwright-mcp), which the agent's CLI starts over stdio as an MCP server named `browser`:
  ```
  npx -y @playwright/mcp@latest --headless --isolated \
    --allowed-origins "http://localhost:<p>;https://localhost:<p>;http://127.0.0.1:<p>;…" \
    --proxy-server http://127.0.0.1:9 --proxy-bypass "localhost:<p>,127.0.0.1:<p>,…" \
    --output-dir <temp dir>/bantu-browser/<workspace id>
  ```
  `<p>` runs from `BANTU_PORT` to `BANTU_PORT+9`. Claude gets it in `--mcp-config` (with `mcp__browser` in `--allowedTools`, except in plan mode), and so does a chat continued in the terminal. ACP agents get it in `session/new` and `session/load`.
- **Only the workspace's own ports.** `--headless --isolated` means no window and a profile kept in memory, so none of the user's cookies or logins. Playwright's `--allowed-origins` blocks other requests from the page, but its docs say it isn't a security boundary, and it doesn't cover redirects. So Bantu also points the browser at a proxy that isn't there (`127.0.0.1:9`), and exempts only the workspace's ports. A redirect to the internet or to another workspace then fails too (`ERR_PROXY_CONNECTION_FAILED`). The browser is a convenience, not a sandbox: the agent's own shell and fetch tools still reach the network as before, and so does Playwright MCP's `browser_run_code_unsafe` tool, which runs code in the server's own Node process.
- **The agent is told** where its app runs, in one line of its briefing: run the dev server on `http://localhost:$BANTU_PORT` and open that.
- **Screenshots** go to the agent as images, and to a temporary folder, not the worktree. Bantu's chat shows the tool's text and `[image]` in place of the picture.
- **Without `npx`** (no Node.js on the daemon's `PATH`), the turn runs without a browser and the chat gets one `notice` saying why. Set `BANTU_NPX_BIN` to use another `npx`.
- **The first turn** downloads Playwright MCP through `npx`, which can take longer than the agent waits for an MCP server; the next turn then has it. The browser is the installed Google Chrome, Playwright MCP's default. That's the only setup it has been tried with.

### Chat and terminal on one conversation

`session.open_terminal` runs `claude --resume <id>` (or `--session-id` before the first turn) or `codex resume <id>` in a PTY, with the same workspace context, model and permission mode as the chat. Bantu's MCP server stays available; the permission hook doesn't apply, because the agent's terminal UI asks for itself.

While that terminal runs, `session.send` returns 409, because two processes must not write to one conversation. When it ends, the chat gets a new turn:

- **Claude**: new entries in its transcript (`<CLAUDE_CONFIG_DIR>/projects/*/<id>.jsonl`) are added as chat items, then a `terminal_session` item. Subagent work and injected context are skipped, and the transcript carries no per-turn cost.
- **Codex**: only the `terminal_session` item is added, but the conversation resumes normally, because Codex keeps it.
- **ACP agents** can't reopen a conversation in a terminal yet (400). Open their own UI with `terminal.open` and `agent`; that's a separate conversation.

### Agents

`AgentKind` is one of these ids. The daemon's catalogue (`crates/daemon/src/catalogue.rs`) holds each one's binary, how it starts, its install command and how to tell it's signed in. `GET /v1/agents` returns an `AgentStatus` for each: `{ agent, name, mode, install_hint, docs_url, can_sign_in, installed, version, logged_in, auth_method, account, organization, plan }`, where `mode` (`AgentMode`) is `native`, `acp` or `terminal`.

| `AgentKind` | Agent | Mode | Command Bantu runs |
|---|---|---|---|
| `claude` | Claude Code | native | `claude -p --output-format stream-json …` |
| `codex` | Codex CLI | native | `codex exec --json …` |
| `opencode` | OpenCode | acp | `opencode acp` |
| `gemini` | Gemini CLI | acp | `gemini --acp` |
| `cursor` | Cursor CLI | acp | `agent acp` |
| `goose` | Goose | acp | `goose acp` |
| `qwen` | Qwen Code | acp | `qwen --acp` |
| `copilot` | GitHub Copilot CLI | acp | `copilot --acp` |
| `auggie` | Auggie (Augment Code) | acp | `auggie --acp` |
| `kimi` | Kimi Code CLI | acp | `kimi acp` |
| `kiro` | Kiro CLI | acp | `kiro-cli acp` |
| `cline` | Cline CLI | acp | `cline --acp` |
| `kilo` | Kilo Code CLI | acp | `kilo acp` |
| `vibe` | Mistral Vibe | acp | `vibe-acp` (installed next to `vibe`) |
| `aider` | Aider | terminal | `aider` |
| `amp` | Amp | terminal | `amp` |
| `crush` | Crush | terminal | `crush` |
| `droid` | Factory Droid | terminal | `droid` |

- **ACP** is listed only where the agent's own current docs show an ACP mode. Amp, Aider and Crush have no first-party ACP mode (only third-party adapters or unreleased branches), and Factory's CLI reference doesn't document one for Droid, so they run in a terminal. Binaries, install commands and sign-in checks were cross-checked against [Agent Orchestrator](https://github.com/OrchestratorInc/agent-orchestrator)'s adapters (Apache-2.0).
- **Tested with fakes only.** The automated tests drive Claude, Codex and the ACP agents through stand-in scripts (`scripts/fake-*.sh`). Goose, Qwen Code, Copilot, Auggie, Kimi, Kiro, Cline, Kilo Code, Mistral Vibe, Aider, Amp, Crush and Droid have not been run against their real CLIs yet.
- **Binaries.** Each agent's binary can be overridden with `BANTU_<ID>_BIN` (e.g. `BANTU_KIRO_BIN`) or `--agent-bin <id>=<path>`. `--claude-bin`, `--codex-bin`, `--opencode-bin`, `--gemini-bin` and `--cursor-bin` still work.
- **Installed** means `<bin> --version` succeeds. **Signed in** comes from `claude auth status`, `codex login status` and `kiro-cli whoami`; Goose, Auggie and Cline count as signed in when their credential file exists or their API-key variable is set; the other agents count as signed in once installed, until a turn says otherwise.
- **Terminal-only agents** can't hold a chat (`session.create` returns 400). `terminal.open` with `agent` runs their own UI in the worktree.
- **Sign-in.** `agent.login` opens the agent's own flow: a subcommand where it has one (`goose configure`, `kiro-cli login`, `auggie auth login`, `cline auth`, …), or else its UI, where it signs in with a command such as `/login`. Aider has none (`can_sign_in` is false): it reads provider API keys from the environment.

### ACP agents

These agents speak the [Agent Client Protocol](https://agentclientprotocol.com): JSON-RPC 2.0 over stdio. Each turn is one process running `initialize`, then `session/new` (or `session/load` to resume), then `session/prompt`.

- **Commands**: the `acp` rows in [Agents](#agents).
- **Client capabilities**: Bantu advertises neither `fs` nor `terminal`, so agents use their own file and shell tools inside the worktree.
- **Bantu's MCP server** is passed to agents that advertise `mcpCapabilities.http`. The token travels over stdin, never in argv. The [browser](#browser), when it's on, is passed as a stdio server, which every ACP agent supports.
- **Workspace context** is prepended to the first prompt, because ACP has no system prompt.
- **Permission modes** are enforced in the client on `session/request_permission`: `bypass_permissions` allows everything; `plan` allows only `read`, `search`, `fetch` and `think` tools; `accept_edits` allows `edit` tools and asks about the rest; `auto` and `ask` always ask. If the agent advertises a matching mode (such as `plan`, `autoEdit` or `yolo`), it's selected with `session/set_mode` too. An agent decides for itself which tools need permission, so this can only restrict what it asks about.
- **Models** are selected with `session/set_model` when the agent advertises `models`.
- **Resuming**: an agent that can't load sessions, or has lost one, gets a new session whose first prompt includes the last 20,000 characters of the transcript.
- **Handshake timeout**: a turn fails after 60 seconds if the agent hasn't reached `session/prompt`, showing whatever it printed to stderr.

## Events

`Event = { seq: number, ts: string, data: EventData }`, where `data` is tagged by `type`:

| `type` | Fields | When |
|---|---|---|
| `repo.added` | `repo` | A repo was registered |
| `repo.removed` | `repo_id` | A repo was forgotten |
| `workspace.updated` | `workspace` | A workspace was created or changed. It always carries the full object, so clients just replace their copy. |
| `workspace.log` | `workspace_id`, `stream`, `line` | One line of setup or archive script output, or a daemon progress message (`system`) |
| `session.updated` | `session` | A session was created or changed (full object) |
| `session.item` | `session_id`, `workspace_id`, `turn`, `item: ChatItem` | One transcript entry; `turn` starts at 1 |
| `comment.updated` | `comment` | A draft was added or a comment was sent (full object) |
| `comment.deleted` | `comment_id`, `workspace_id` | |
| `permission.requested` | `request` | An agent is waiting for approval |
| `permission.resolved` | `request_id`, `session_id`, `workspace_id`, `allowed` | Answered, stopped or timed out |
| `repo.updated` | `repo` | A repo's details changed, such as its GitHub login |
| `pr.updated` | `pr` | A workspace's pull request changed (full `WorkspacePr`) |
| `pr.removed` | `workspace_id` | Its branch no longer has a pull request |
| `tracker.updated` | `tracker` | A tracker was connected, or synced (full object) |
| `tracker.removed` | `tracker_id` | |
| `org.updated` | `org`, `ended` | Joined, synced, left or removed from an organization. `org` is `null` without one; `ended` says why it was removed |
| `tasks.changed` | `tracker_id` | Its tasks changed; fetch `GET /v1/tasks` |
| `loop.updated` | `loop` | A loop was created or changed, including after a run (full object) |
| `loop.deleted` | `loop_id` | |
| `spotlight.updated` | `repo_id`, `spotlight` | A workspace entered or updated the main checkout; `spotlight` is `null` when it returned to its branch |
| `inbox.added` | `item` | An agent sent a notice or question |
| `inbox.removed` | `item_id`, `workspace_id` | It was answered, dismissed, timed out or released by a stop |
| `terminal.updated` | `terminal` | A terminal opened or its process exited (full object) |
| `terminal.closed` | `terminal_id`, `workspace_id` | Closed by the user, by archiving, or by a daemon restart |

## Repo configuration: `.bantu/settings.toml`

```toml
[scripts]
setup = "bun install"
archive = "docker compose down"
run = "bun dev --port $BANTU_PORT"

# or Conductor-style named run actions (the first one is used for now):
# [scripts.run.dev]
# command = "bun dev --port $BANTU_PORT"
```

Repo-specific instructions can be added to Bantu's built-in prompts. Each one is appended to the prompt for that action, after the context Bantu fills in:

```toml
[prompts]
review = "We care most about migrations. Ignore generated files."
create_pr = "Use our template in .github/pull_request_template.md."
fix_checks = "CI runs on Node 22; run `bun test` locally first."
address_comments = "Reply on the PR when you disagree."
resolve_conflicts = "Prefer rebasing over merge commits."
rename_branch = "Branches look like team/short-description."
```

A headless browser for agents is off by default. This turns it on for new chats (see [Browser](#browser)):

```toml
[browser]
enabled = true
```

Automatic review by another agent is off by default, because it spends quota. It's described under [Automatic review](#automatic-review).

```toml
[review]
auto = true
agent = "codex"   # defaults to Codex for Claude's work, and Claude for everyone else's
```

Which agent the repo uses for each role. Any `AgentKind` that can hold a chat works; terminal-only and unknown ids are ignored, with a warning in the daemon log.

```toml
[agents]
worker = "codex"         # new chats, including each workspace's first chat and create_workspace children
orchestrator = "claude"  # a lead chat: session.create with role "orchestrator", or any new chat
                         # in a workspace that has started others
reviewer = "gemini"      # workspace.review without an agent, and automatic review
```

Each defaults to Claude, except that automatic review without a reviewer still picks a different agent from the author's. `[review] agent` is an alias for `reviewer`; `reviewer` wins when both are set. An explicit `agent` in a command always wins.

Bantu looks for `setup` and `run` first in the worktree (the branch's committed config), then in the main checkout. Settings that make the daemon act on its own (`check`, `archive`, `[prompts]`, `[review]`, `[pr]`, `[browser]` and `[agents]`) come only from the main checkout, because an agent allowed to edit files could otherwise run any command without a prompt, or turn off its own review. A branch's changes to them apply once they're merged. While Spotlight is active, those settings come from the base branch instead. In each place it tries `.bantu/settings.toml`, then `.conductor/settings.toml` with the same keys, so existing Conductor setups work unchanged. Unknown keys are ignored.

Scripts run in the user's login shell (`$SHELL -lc`) in the workspace directory, with these environment variables:

| Variable | Value |
|---|---|
| `BANTU_PORT` | `port_base` (10 ports reserved: `BANTU_PORT` .. `BANTU_PORT+9`) |
| `BANTU_WORKSPACE_ID` | Workspace id |
| `BANTU_WORKSPACE_NAME` | Workspace name |
| `BANTU_WORKSPACE_PATH` | Worktree path |
| `BANTU_PREVIEW_BASE` | `/p/{workspace}/{BANTU_PORT}/`, the preview's base path (terminals and the run script) |
| `BANTU_CF_ACCESS_TEAM`, `BANTU_CF_ACCESS_AUD` | Daemon settings, not script variables: with both, requests for a public hostname need a valid Cloudflare Access token as well as the daemon's ([CLOUDFLARE.md](CLOUDFLARE.md)) |
| `BANTU_ROOT_PATH` | Main checkout path (setup and archive scripts) |
| `BANTU_BASE_REF` | e.g. `origin/main` (setup and archive scripts) |

On Windows, scripts run in `$SHELL` if it's set (Git Bash and MSYS set it), and otherwise in PowerShell, where variables are written `$env:BANTU_PORT`.

When `bantud` starts, it adopts the `PATH` of the user's login shell. A daemon started from the Dock therefore finds `claude`, `bun`, `node` and friends just like a terminal would. Set `BANTU_SKIP_SHELL_ENV=1` to skip this.

## Example session

```sh
TOKEN=$(bantud --print-token)
H="Authorization: Bearer $TOKEN"
J='content-type: application/json'

curl -s 127.0.0.1:47800/v1/health
curl -s -H "$H" -H "$J" -d '{"type":"repo.add","path":"/Users/me/code/app"}' 127.0.0.1:47800/v1/commands
curl -s -H "$H" -H "$J" -H "Idempotency-Key: $(uuidgen)" \
  -d '{"type":"workspace.create","repo_id":"<repo id>"}' 127.0.0.1:47800/v1/commands
curl -s -H "$H" 127.0.0.1:47800/v1/state                       # find the session id
curl -s -H "$H" -H "$J" \
  -d '{"type":"session.send","session_id":"<session id>","text":"Add a health check endpoint"}' \
  127.0.0.1:47800/v1/commands
curl -N -H "$H" '127.0.0.1:47800/v1/events?since=0'
```

## Planned


See [ROADMAP.md](ROADMAP.md).
