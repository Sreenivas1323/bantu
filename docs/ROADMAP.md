# Roadmap

Each milestone ends with something usable. Week estimates assume one focused maintainer
plus agents. Priorities: macOS and Linux first, Windows later.

Feature-by-feature status against Conductor and Pane lives in
[competitive/parity.md](competitive/parity.md).

## M0 — Foundations (week 1)

- [ ] Public repo, Apache-2.0, README, contributing guide, code of conduct
- [ ] Protocol v1: Rust wire types with generated TypeScript bindings ([PROTOCOL.md](PROTOCOL.md))
- [ ] `bantud`:
  - [ ] loopback HTTP, local token auth
  - [ ] SQLite event log with projections
  - [ ] SSE events that resume from `seq`
  - [ ] idempotent commands
- [ ] Workspaces:
  - [ ] add a repo
  - [ ] create a worktree (city names, 10-port ranges, `.worktreeinclude` + `.env*` copy, setup script)
  - [ ] archive (archive script, save changes to a ref, remove the worktree)
- [ ] Web UI: repos, workspaces, new/archive buttons, live event feed
- [ ] Tauri desktop shell that starts or connects to `bantud`
- [ ] CI on macOS and Ubuntu (fmt, clippy, tests, web build)
- [ ] Changelog watcher (GitHub Action) + weekly Claude idea routine
- [ ] Parity matrix and idea backlog seeded from research

## M1 — Daily driver (weeks 2–3)

- [ ] Agent driver trait and normalized agent events in the protocol
- [ ] Claude structured sessions (Bun sidecar + Agent SDK):
  - [ ] streaming text and tool-call cards
  - [ ] message queue
  - [ ] plan mode
  - [ ] permission prompts
  - [ ] cancel
- [ ] Session persistence and resume after a daemon restart
- [ ] Diff viewer (workspace vs base), with comments sent to the agent
- [ ] Create PR via `gh`, with an agent-drafted title and body
- [ ] Terminal panel (pty over WebSocket), restored after a reload
- [ ] Run scripts (`[scripts.run.<id>]`), stop = SIGTERM to the process group, then SIGKILL
- [ ] Agent names the branch from the first message
- [ ] Notify inbox (desktop): questions, permission requests, "done"

## M2 — Conductor parity (weeks 4–5)

- [ ] Per-turn checkpoints in private refs; rewind code + chat
- [ ] Codex driver (`codex app-server`) and generic ACP driver (OpenCode, Cursor, Gemini)
- [ ] Terminal driver; switch chat ⇄ terminal in the same conversation
- [ ] Checks view: git status, `gh pr checks`, Actions logs; forward failures to the agent
- [ ] Per-repo prompts: review, create PR, fix errors, resolve conflicts, rename branch
- [ ] Bantu MCP server injected into every session (diff, comments, terminal, notify)
- [ ] Model picker and loadouts; effort/fast mode; several chats per workspace; fork a chat
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
