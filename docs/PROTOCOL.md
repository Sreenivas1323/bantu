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
| Data dir | `~/.bantu` (override with `BANTU_HOME`): `bantu.db` (SQLite) and `local-token` (mode 0600) |
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
  "inbox": [InboxItem, ...]
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

### `Session`
One chat with an agent in a workspace. Every workspace gets one automatically when it becomes ready.

| Field | Type | Notes |
|---|---|---|
| `id` | string | |
| `workspace_id` | string | |
| `agent` | `"claude" \| "codex" \| "opencode" \| "gemini" \| "cursor"` | Fixed when the chat is created |
| `title` | string | `"New chat"` until the first message, which becomes the title |
| `model` | string \| null | `null` uses the agent's default (e.g. `opus`, `sonnet`, `haiku`, or a full model id) |
| `permission_mode` | `PermissionMode` | See below |
| `effort` | string \| null | `low`, `medium`, `high`, `xhigh` or `max`; `null` uses the agent's default. Claude and Codex only (Codex treats `max` as `xhigh`) |
| `fast` | boolean | Claude's fast mode |
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
| `tool_result` | `tool_use_id`, `content`, `is_error` | Text output, truncated to 20,000 characters |
| `turn_end` | `is_error`, `duration_ms`, `cost_usd`, `input_tokens`, `output_tokens` | `input_tokens` includes cache reads and writes |
| `rewound` | `to_turn` | Files were restored to before `to_turn` |
| `terminal_session` | `imported` | The chat continued in a terminal; `imported` entries read back from the agent's transcript come just before this one |
| `error` | `message` | Bantu-side problems: the agent couldn't start, crashed, or was stopped (`"Stopped"`) |

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
| `workspace.integrate` | `workspace_id`, `session_id?` | `IntegrationResult` | Merges finished child workspaces into this one's branch; see below. 409 if the lead is busy or has uncommitted changes. |
| `workspace.spotlight` | `workspace_id` | `Spotlight` | Shows the workspace's work in the repo's main checkout; see below. 409 if the main checkout has uncommitted changes. |
| `repo.spotlight_stop` | `repo_id` | — | Returns the main checkout to its branch. 409 if it has uncommitted changes. |
| `workspace.create_pr` | `workspace_id`, `session_id` | `Session` | Asks the agent to commit, push and `gh pr create` against `base_ref`. 400 without an `origin` remote. |
| `workspace.fix_checks` | `workspace_id`, `session_id` | `Session` | Sends failing checks and their logs to the agent. |
| `workspace.address_comments` | `workspace_id`, `session_id` | `Session` | Sends the PR's comments to the agent. 400 without a PR or comments. |
| `workspace.resolve_conflicts` | `workspace_id`, `session_id` | `Session` | Asks the agent to resolve what the conflict radar found. 400 if there's nothing to resolve. |
| `workspace.rename_branch` | `workspace_id`, `session_id` | `Session` | Asks the agent to rename the branch. The daemon picks up the new name when the turn ends. |
| `workspace.review` | `workspace_id`, `agent?` | `Session` | Starts a read-only chat that reviews the workspace's changes. |
| `session.create` | `workspace_id`, `agent?`, `model?`, `permission_mode?`, `effort?`, `fast?` | `Session` | Opens another chat in a ready workspace. |
| `session.fork` | `session_id` | `Session` | Starts a new chat that continues from where this one is now; see below. 409 while a turn is running. |
| `session.send` | `session_id`, `text` | `Session` | Sends a message. If a turn is running it's queued (`queued` goes up) and runs afterwards, in order. |
| `session.cancel` | `session_id` | `Session` | Stops the running turn (it ends with an `error` item `"Stopped"`), drops queued messages, and denies pending permission requests. |
| `session.update` | `session_id`, `title?`, `model?`, `permission_mode?`, `effort?`, `fast?` | `Session` | Changes settings; they apply from the next turn. An empty `effort` goes back to the default. |
| `session.rewind` | `session_id`, `turn` | `Session` | Restores the workspace files to the checkpoint taken just before `turn` started; see below. 409 while the agent is running. |
| `permission.respond` | `request_id`, `allow`, `message?` | — | Answers a `PermissionRequest`. `message` tells the agent why it was denied. 404 if already answered. |
| `inbox.respond` | `item_id`, `answer` | — | Answers a question in the inbox. 404 if already handled. |
| `inbox.dismiss` | `item_id` | — | Clears a notice, or declines to answer a question. |
| `push.subscribe` | `endpoint`, `p256dh`, `auth` | — | Sends this browser alerts through Web Push. 400 unless the endpoint uses HTTPS. |
| `push.unsubscribe` | `endpoint` | — | |
| `comment.add` | `workspace_id`, `path`, `line`, `body` | `DiffComment` | Adds a draft review comment. |
| `comment.delete` | `comment_id` | — | |
| `comments.send` | `workspace_id`, `session_id` | `Session` | Sends every draft comment to the agent as one message (sorted by file and line), then marks them sent. 400 if there are none. |
| `terminal.open` | `workspace_id`, `cols?`, `rows?`, `command?` | `Terminal` | Opens an interactive login shell in the worktree, or runs `command` (any CLI) there. |
| `session.open_terminal` | `session_id`, `cols?`, `rows?` | `Terminal` | Continues a Claude or Codex chat in the agent's own terminal UI; see below. Returns the existing terminal if it's already open. 409 while a turn is running. |
| `terminal.close` | `terminal_id` | — | Hangs up the terminal's whole process group and forgets it. |
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

The main checkout belongs to the user, so nothing here overwrites it. Tracked changes or a merge in progress refuse the command with 409. If that happens during an automatic update, the update is skipped and the reason goes in the workspace log. Gitignored files are left alone.

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
- **What's rewritten.** Redirects to `/path` or `http://localhost:{port}/path` stay inside the preview. A root-absolute request such as `/assets/app.js` is redirected back into its preview using the `Referer`. Bantu's cookie and `Authorization` header aren't forwarded, and `Host` is set to `localhost:{port}`, since dev servers often reject other names. A WebSocket opened at a root path can't be redirected, so hot reload clients may need their dev server's base path set to `$BANTU_PREVIEW_BASE` (set in workspace terminals to `/p/{workspace}/{BANTU_PORT}/`). URLs an app builds from its own host, and third-party sign-in callbacks, may still need the app configured for its preview address.

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
  --settings <PermissionRequest hook JSON> --mcp-config <Bantu MCP server>
```

- **One process per turn.** The prompt goes in over stdin. The first turn uses `--session-id`; every later turn uses `--resume` with the same id, so the conversation continues. A daemon restart never strands a process: turns that were running are marked with an `error` item `"Interrupted by a daemon restart"`, and the next message simply resumes.
- **Output** is parsed line by line into `ChatItem`s (`session.item` events). Partial text becomes `delta` frames.
- **Workspace context** is appended to the system prompt: the workspace name, branch, base, and its reserved ports.
- **Lean context.** Unless `preferences.claude_user_setup` is on, turns skip the user's global Claude setup (`~/.claude/settings.json`, its plugins, hooks and skills, user MCP servers and `~/.claude/CLAUDE.md`). A trivial turn then sends about 17k input tokens instead of about 53k with a heavily customised setup. The repo's `CLAUDE.md`, `.claude/settings.json` and `.claude/settings.local.json` still load, and so do Bantu's hook and MCP server.
- **Environment**: `BANTU_PORT`, `BANTU_WORKSPACE_ID`, `BANTU_WORKSPACE_NAME`, `BANTU_WORKSPACE_PATH`, `BANTU_SESSION_ID`.
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
| `list_terminals`, `get_terminal_output` | Read a workspace's terminals, such as dev server logs (ANSI codes stripped, last 2,000 lines at most) |
| `list_diff_comments`, `add_diff_comment` | Read review comments, and add draft ones for the user to send |
| `notify_user` | Send a notice to the inbox without waiting |
| `ask_user` | Ask a question and block until it's answered, dismissed or stopped, or the timeout passes (10–1800 s, default 600) |

### Chat and terminal on one conversation

`session.open_terminal` runs `claude --resume <id>` (or `--session-id` before the first turn) or `codex resume <id>` in a PTY, with the same workspace context, model and permission mode as the chat. Bantu's MCP server stays available; the permission hook doesn't apply, because the agent's terminal UI asks for itself.

While that terminal runs, `session.send` returns 409, because two processes must not write to one conversation. When it ends, the chat gets a new turn:

- **Claude**: new entries in its transcript (`<CLAUDE_CONFIG_DIR>/projects/*/<id>.jsonl`) are added as chat items, then a `terminal_session` item. Subagent work and injected context are skipped, and the transcript carries no per-turn cost.
- **Codex**: only the `terminal_session` item is added, but the conversation resumes normally, because Codex keeps it.
- **ACP agents** can't reopen a conversation in a terminal yet (400). Run their CLI with `terminal.open` and a `command`; that's a separate conversation.

### ACP agents (OpenCode, Gemini CLI, Cursor)

These agents speak the [Agent Client Protocol](https://agentclientprotocol.com): JSON-RPC 2.0 over stdio. Each turn is one process running `initialize`, then `session/new` (or `session/load` to resume), then `session/prompt`.

- **Commands**: `opencode acp`, `gemini --acp` and `agent acp`. Override the binaries with `BANTU_OPENCODE_BIN`, `BANTU_GEMINI_BIN` and `BANTU_CURSOR_BIN`.
- **Client capabilities**: Bantu advertises neither `fs` nor `terminal`, so agents use their own file and shell tools inside the worktree.
- **Bantu's MCP server** is passed to agents that advertise `mcpCapabilities.http`. The token travels over stdin, never in argv.
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

Automatic review by another agent is off by default, because it spends quota. It's described under [Automatic review](#automatic-review).

```toml
[review]
auto = true
agent = "codex"   # defaults to Codex for Claude's work, and Claude for everyone else's
```

Bantu looks for settings first in the worktree (the branch's committed config), then in the main checkout. In each place it tries `.bantu/settings.toml`, then `.conductor/settings.toml` with the same keys, so existing Conductor setups work unchanged. Unknown keys are ignored.

Scripts run in the user's login shell (`$SHELL -lc`) in the workspace directory, with these environment variables:

| Variable | Value |
|---|---|
| `BANTU_PORT` | `port_base` (10 ports reserved: `BANTU_PORT` .. `BANTU_PORT+9`) |
| `BANTU_WORKSPACE_ID` | Workspace id |
| `BANTU_WORKSPACE_NAME` | Workspace name |
| `BANTU_WORKSPACE_PATH` | Worktree path |
| `BANTU_PREVIEW_BASE` | `/p/{workspace}/{BANTU_PORT}/`, the preview's base path (terminals and the run script) |
| `BANTU_ROOT_PATH` | Main checkout path (setup and archive scripts) |
| `BANTU_BASE_REF` | e.g. `origin/main` (setup and archive scripts) |

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
