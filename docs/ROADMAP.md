# Roadmap

Each milestone ends with something usable. Week estimates assume one focused maintainer
plus agents. Priorities: macOS and Linux first, Windows later.

Feature-by-feature status against Conductor and Pane lives in
[competitive/parity.md](competitive/parity.md).

## M0 — Foundations (week 1)

- [x] Public repo, Apache-2.0, README, contributing guide, code of conduct
- [x] Protocol v1: Rust wire types with generated TypeScript bindings ([PROTOCOL.md](PROTOCOL.md))
- [x] `bantud`:
  - [x] loopback HTTP, local token auth
  - [x] SQLite event log with projections
  - [x] SSE events that resume from `seq`
  - [x] idempotent commands
- [x] Workspaces:
  - [x] add a repo
  - [x] create a worktree (city names, 10-port ranges, `.worktreeinclude` + `.env*` copy, setup script)
  - [x] archive (archive script, save changes to a ref, remove the worktree)
- [x] Web UI: repos, workspaces, new/archive buttons, live event feed
- [x] Tauri desktop shell that starts or connects to `bantud`
- [x] CI on macOS and Ubuntu (fmt, clippy, tests, web build)
- [x] Changelog watcher (GitHub Action) + weekly Claude idea routine
- [x] Parity matrix and idea backlog seeded from research

## M1 — Daily driver (weeks 2–3)

- [x] Normalized agent events in the protocol (`ChatItem`)
- [x] Claude sessions, driving the user's own `claude` CLI (stream-json, one process per turn, resumed by id — no sidecar needed):
  - [x] tool-call cards paired with results; Markdown answers; thinking
  - [x] token-by-token streaming (`--include-partial-messages` → SSE `event: delta`, never stored)
  - [x] message queue
  - [x] plan mode and other permission modes
  - [x] permission prompts answered from the UI (Claude `PermissionRequest` HTTP hook → approval card; new "Ask first" mode)
  - [x] cancel
- [x] Session persistence and resume after a daemon restart
- [x] Diff viewer (workspace vs merge-base, incl. uncommitted + untracked), with line comments sent to the agent
- [x] Create PR via the agent + `gh`; PR status and checks summary
- [x] Terminal panel (PTY over WebSocket), scrollback replayed after a reload
- [x] Run scripts (`run = "…"` or `[scripts.run.<id>]`); stop hangs up the process group
- [x] Placeholder branch renamed from the first message (`bantu/hyderabad` → `bantu/fix-login-redirect`) while unpushed
- [x] Desktop notifications for approvals and finished turns (browser Notification API); pending count in the tab title. A cross-device inbox comes with M3.
- [x] Login-shell PATH for Dock-launched daemons
- [x] Lean context by default: turns skip the user's global plugins, hooks, skills and MCP servers (about a third of the input tokens); one switch in Accounts brings them back

## M2 — Conductor parity (weeks 4–5)

- [x] Per-turn checkpoints in private refs; rewind code (the agent is told; Claude has no partial-conversation rewind)
- [x] Codex driver (`codex exec --json`, resumed by thread id; sandbox per permission mode) — not yet verified against the live service
- [x] Generic ACP driver (OpenCode via `opencode acp`, Gemini via `gemini --acp`, Cursor via `agent acp`): streaming, tool cards, permission prompts, resume with `session/load` — tested with a fake agent; the handshake is verified against real Gemini CLI, but no full turn has run against a signed-in agent yet
- [x] Terminal driver: any CLI in a PTY, and a Claude or Codex chat continued in its own terminal UI. Claude's transcript is read back into the chat when the terminal ends (Codex resumes correctly, but its terminal history isn't imported)
- [x] Fix failing checks: `gh pr checks` + failed-step logs from GitHub Actions (cleaned, last 120 lines) sent to the agent in one click
- [ ] Full checks tab (deployments, PR comments, todos)
- [ ] Per-repo prompts: review, create PR, fix errors, resolve conflicts, rename branch
- [x] Bantu MCP server injected into every Claude session (`list_workspaces`, `create_workspace`, `workspace_status`, `wait_for_workspace`, `send_message`, `get_diff`)
- [ ] More MCP tools: terminal output, diff comments, notify/ask the user
- [ ] Model picker (done) and loadouts; effort/fast mode; several chats per workspace (done); fork a chat
- [ ] Spotlight: sync a workspace into the main checkout for testing
- [x] Create a workspace from a branch (local or origin), a PR (incl. forks via pull/N/head) or a GitHub issue (the agent starts on it)

## M3 — Anywhere (weeks 6–7)

- [x] Release builds (macOS arm64/x64, Linux x64/arm64) + `install.sh` with checksums; launchd/systemd service
- [x] Pairing by QR code (`bantud url --qr`, with `BANTU_PUBLIC_URL`)
- [ ] Web/PWA client deployable as a static site; phone layout
- [ ] Web Push from the daemon (VAPID) for the Notify inbox
- [x] AWS template (Tailscale variant): EC2 t4g, no open ports, idle auto-stop via `/v1/activity`, Lambda wake link, Session Manager shell ([AWS.md](AWS.md)) — lint-checked, not yet deployed end to end
- [ ] CloudFront variant (needs your own domain + certificate on the instance)
- [ ] Launch Stack button (needs a public S3 copy of the template)
- [x] Guides: Tailscale Serve, Cloudflare Tunnel, SSH ([REMOTE.md](REMOTE.md)); `--allowed-host` instead of all-or-nothing
- [ ] Port forwarding for dev-server previews on remote daemons

## M4 — Beyond (weeks 8–10)

- [x] Orchestration: any chat can split work into new workspaces with their own agents, wait for them and review their diffs (verified with real Claude); children inherit model and permission mode; max 10 active workspaces per repo
- [ ] Orchestrator UI: a lead chat with a live board of its child workspaces; integrate results into one branch
- [x] Conflict radar: snapshots every ready workspace (uncommitted work included) and test-merges each pair and each against its base with `git merge-tree`; reruns after turns; ⚠ in the sidebar + banner
- [x] Definition of done: `[scripts] check` runs after every agent turn that changed files (not in plan mode, not when the user queued a message); failures go back to the agent up to 2 times in a row
- [x] Review: a fresh read-only (plan mode) chat reviews the workspace diff; Shift-click to review with Codex
- [ ] Automatic cross-agent review before "Ready", findings sent back to the author chat
- [ ] Loops / routines: cron, GitHub triggers
- [ ] Vercel Sandbox burst runners
- [ ] Import sessions from Conductor and Pane
- [ ] Windows support
