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
  - [ ] token-by-token streaming (`--include-partial-messages`)
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

## M2 — Conductor parity (weeks 4–5)

- [x] Per-turn checkpoints in private refs; rewind code (the agent is told; Claude has no partial-conversation rewind)
- [ ] Codex driver (`codex app-server`) and generic ACP driver (OpenCode, Cursor, Gemini)
- [ ] Terminal driver; switch chat ⇄ terminal in the same conversation
- [ ] Checks view: git status, `gh pr checks`, Actions logs; forward failures to the agent
- [ ] Per-repo prompts: review, create PR, fix errors, resolve conflicts, rename branch
- [ ] Bantu MCP server injected into every session (diff, comments, terminal, notify)
- [ ] Model picker (done) and loadouts; effort/fast mode; several chats per workspace (done); fork a chat
- [ ] Spotlight: sync a workspace into the main checkout for testing
- [ ] Create a workspace from a PR, a branch or a GitHub issue

## M3 — Anywhere (weeks 6–7)

- [ ] Linux packages + `install.sh`; launchd/systemd service
- [ ] Pairing by QR code (daemon URL + token; the daemon stores only a hash)
- [ ] Web/PWA client deployable as a static site; phone layout
- [ ] Web Push from the daemon (VAPID) for the Notify inbox
- [ ] AWS Launch Stack: EC2 t4g + CloudFront + idle auto-stop + Lambda wake URL
- [ ] Guides: Tailscale Serve, Cloudflare Tunnel
- [ ] Port forwarding for dev-server previews on remote daemons

## M4 — Beyond (weeks 8–10)

- [ ] Orchestrator mode: split a task into N workspaces, dispatch, watch, integrate
- [ ] Conflict radar between active branches
- [ ] Definition-of-done loop before "Ready"
- [ ] Cross-agent review (Claude writes, Codex reviews, or the reverse)
- [ ] Loops / routines: cron, GitHub triggers
- [ ] Vercel Sandbox burst runners
- [ ] Import sessions from Conductor and Pane
- [ ] Windows support
