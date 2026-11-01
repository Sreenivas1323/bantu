# Bantu protocol v1

Every Bantu client — desktop app, web/PWA, CLI, an agent calling over MCP — talks to
`bantud` through this protocol. It is the same locally and remotely.

- **Commands** go in as `POST /v1/commands`.
- **State** comes out as an append-only, resumable event stream: `GET /v1/events` (Server-Sent Events).
- `GET /v1/state` returns a snapshot so a client can render at once, then follow events from `last_seq`.

Wire types are defined in Rust in [`crates/protocol/src/lib.rs`](../crates/protocol/src/lib.rs).
TypeScript bindings are generated into `packages/protocol/src/generated/` by
`cargo test -p bantu-protocol`. If this document and the Rust types disagree, the Rust
types win; please fix this document.

`PROTOCOL_VERSION` is `1`. Breaking changes bump it; additive changes (new optional
fields, new command or event types) don't, so clients must ignore unknown event types
and unknown fields.

## Transport and auth

| | |
|---|---|
| Default address | `127.0.0.1:47800` (`--host`, `--port`). Non-loopback hosts are refused unless `--allow-remote` is passed; expose a daemon remotely through Tailscale Serve, Cloudflare Tunnel or CloudFront instead. |
| Data dir | `~/.bantu` (override with `BANTU_HOME`): `bantu.db` (SQLite) and `local-token` (mode 0600) |
| Auth | `Authorization: Bearer <token>`, or `?token=<token>` on the query string (needed for `EventSource`, which can't set headers) |
| Token | `bantud --print-token` prints it |
| Web app | The daemon serves the built web app at `/` (`--web-dir` or `BANTU_WEB_DIR`) |

All bodies are JSON (`content-type: application/json`).

## Endpoints

### `GET /v1/health` (no auth)
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
  "workspaces": [Workspace, ...]
}
```
Render this, then open `/v1/events?since=42`.

### `POST /v1/commands` → `CommandResponse`
The body is one `Command`. An optional `Idempotency-Key` header makes retries safe:
the daemon stores the response for 24 hours and replays it for the same key without
running the command again. Clients on flaky networks (phones, CloudFront) should always
send one, e.g. a UUID per user action.

```json
{ "ok": true, "result": { ... }, "error": null }
{ "ok": false, "result": null, "error": "repository not found: r_123" }
```
`result` is command-specific (see below). Long-running work, such as creating a
worktree and running its setup script, completes asynchronously: the command returns
right away and progress arrives as events.

### `GET /v1/events?since=<seq>` → SSE stream of `Event`
- Replays every stored event with `seq > since` from the database, then streams live events.
- Each SSE message has `id: <seq>` and `data: <Event JSON>`.
- A comment line is sent every 15 seconds as a keepalive.
- `since` defaults to `0` (full replay).
- To resume after a disconnect, reconnect with `since=<last seq you processed>`.
- `seq` is monotonic and gap-free per daemon.

```
id: 43
data: {"seq":43,"ts":"2026-11-01T10:00:00Z","data":{"type":"workspace.updated","workspace":{...}}}
```

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
| `name` | string | Human name, by default an unused city (`lisbon`) |
| `branch` | string | `bantu/<name>` |
| `base_ref` | string | What the branch was created from, and the PR target, e.g. `origin/main` |
| `path` | string | `~/bantu/workspaces/<repo>/<name>` |
| `port_base` | number | First of 10 reserved ports (`BANTU_PORT` .. `BANTU_PORT+9`), allocated from 55000 in steps of 10 |
| `state` | `WorkspaceState` | |
| `error` | string \| null | Set when `state` is `failed` |
| `archive_commit` | string \| null | Commit holding uncommitted tracked changes at archive time |
| `created_at` | string | RFC 3339 |

### `WorkspaceState`
`"creating" | "ready" | "failed" | "archiving" | "archived"`

### `LogStream`
`"stdout" | "stderr" | "system"`

## Commands

Commands are tagged by `type`.

| `type` | Fields | `result` | What it does |
|---|---|---|---|
| `repo.add` | `path` | `Repo` | Registers a local git repo. Any path inside the repo works. |
| `repo.remove` | `repo_id` | — | Forgets a repo. Files on disk are untouched. |
| `workspace.create` | `repo_id`, `name?`, `base_ref?` | `Workspace` (state `creating`) | Creates a worktree and branch; see below. |
| `workspace.archive` | `workspace_id` | `Workspace` (state `archiving`) | Archives a workspace; see below. |

```json
{ "type": "workspace.create", "repo_id": "r_123", "name": null, "base_ref": null }
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
7. Ends in state `ready`, or `failed` with `error` set.

### `workspace.archive`
1. Runs the archive script, if any.
2. Saves uncommitted tracked changes with `git stash create` to `refs/bantu/archive/<id>` and records the commit as `archive_commit`. Untracked files are not saved.
3. Removes the worktree.
4. Keeps the branch.
5. Ends in state `archived`.

## Events

`Event = { seq: number, ts: string, data: EventData }`, where `data` is tagged by `type`:

| `type` | Fields | When |
|---|---|---|
| `repo.added` | `repo` | A repo was registered |
| `repo.removed` | `repo_id` | A repo was forgotten |
| `workspace.updated` | `workspace` | A workspace was created or changed. It always carries the full workspace, so clients just replace their copy. |
| `workspace.log` | `workspace_id`, `stream`, `line` | One line of setup or archive script output, or a daemon progress message (`system`) |

## Repo configuration: `.bantu/settings.toml`

```toml
[scripts]
setup = "bun install"
archive = "docker compose down"
```

If `.bantu/settings.toml` is missing, Bantu reads `.conductor/settings.toml` using the same
keys, so existing Conductor setups work unchanged.

Scripts run with `sh -c` in the workspace directory with these environment variables:

| Variable | Value |
|---|---|
| `BANTU_PORT` | `port_base` (10 ports reserved: `BANTU_PORT` .. `BANTU_PORT+9`) |
| `BANTU_WORKSPACE_ID` | Workspace id |
| `BANTU_WORKSPACE_NAME` | Workspace name |
| `BANTU_WORKSPACE_PATH` | Worktree path |
| `BANTU_ROOT_PATH` | Main checkout path |
| `BANTU_BASE_REF` | e.g. `origin/main` |

## Example session

```sh
TOKEN=$(bantud --print-token)
H="Authorization: Bearer $TOKEN"

curl -s 127.0.0.1:47800/v1/health
curl -s -H "$H" -H 'content-type: application/json' \
  -d '{"type":"repo.add","path":"/Users/me/code/app"}' 127.0.0.1:47800/v1/commands
curl -s -H "$H" -H 'content-type: application/json' -H "Idempotency-Key: $(uuidgen)" \
  -d '{"type":"workspace.create","repo_id":"<id>"}' 127.0.0.1:47800/v1/commands
curl -N -H "$H" '127.0.0.1:47800/v1/events?since=0'
```

## Planned (not in v1 yet)

- Agent sessions: `session.create`, `session.send`, `session.cancel`, plus normalized agent events (`turn.*`, `tool.*`, `permission.*`, `usage`) — M1
- `GET /v1/pty/<id>` WebSocket for raw terminal bytes — M1
- Checkpoints, diff comments and checks — M2
- Pairing codes for remote clients — M3

See [ROADMAP.md](ROADMAP.md).
