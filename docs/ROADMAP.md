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
- [x] Full checks tab: CI runs, deployments, PR comments (sent to the agent in one click) and todos from the agent and the PR description
- [x] Per-repo prompts: `[prompts]` in `.bantu/settings.toml` adds instructions to review, create PR, fix checks, address comments, resolve conflicts and rename branch
- [x] Bantu MCP server injected into every Claude session (`list_workspaces`, `create_workspace`, `workspace_status`, `wait_for_workspace`, `send_message`, `get_diff`)
- [x] More MCP tools: `list_terminals`, `get_terminal_output`, `list_diff_comments`, `add_diff_comment`, `notify_user`, `ask_user`, plus an inbox in the UI
- [x] Model picker, loadouts, effort and fast mode, several chats per workspace, and forking a chat (Claude forks the real conversation; other agents get the transcript)
- [x] Spotlight: show a workspace's work in the main checkout as a detached snapshot that follows each agent turn; refuses to touch uncommitted changes there
- [x] Create a workspace from a branch (local or origin), a PR (incl. forks via pull/N/head) or a GitHub issue (the agent starts on it)

## M3 — Anywhere (weeks 6–7)

- [x] Release builds (macOS arm64/x64, Linux x64/arm64) + `install.sh` with checksums; launchd/systemd service
- [x] Pairing by QR code (`bantud url --qr`, with `BANTU_PUBLIC_URL`)
- [x] Web/PWA client: installable, deployable as a static site (`#daemon=…&token=…` pairing with `BANTU_APP_URL`), with a phone layout
- [x] Web Push from the daemon (VAPID) for approvals, inbox items and finished turns — encryption matches the RFC 8291 test vector and delivery is tested with a local push server, but not yet against a real browser push service
- [x] AWS template (Tailscale variant): EC2 t4g, no open ports, idle auto-stop via `/v1/activity`, Lambda wake link, Session Manager shell ([AWS.md](AWS.md)) — lint-checked, not yet deployed end to end
- [x] CloudFront variant: your own domain, Caddy for the origin certificate, CloudFront-only access with a secret header, and a separate distribution for previews ([AWS.md](AWS.md)) — lint-checked and its routing tested with the real Caddy, not yet deployed end to end
- [x] Launch Stack links for both AWS templates ([AWS.md](AWS.md#launch-stack)), from a public S3 bucket; CloudFormation validates both from there
- [x] Guides: Tailscale Serve, Cloudflare Tunnel, SSH ([REMOTE.md](REMOTE.md)); `--allowed-host` instead of all-or-nothing
- [x] Port forwarding for dev-server previews: HTTP and WebSockets through a separate origin, with short-lived per-port grants

## M4 — Beyond (weeks 8–10)

- [x] Orchestration: any chat can split work into new workspaces with their own agents, wait for them and review their diffs (verified with real Claude); children inherit model and permission mode; max 10 active workspaces per repo
- [x] Orchestrator UI: a live board of a lead's child workspaces, nested in the sidebar, and one-click local merging into its branch (the agent takes over on a conflict); also an `integrate_workspaces` MCP tool
- [x] Conflict radar: snapshots every ready workspace (uncommitted work included) and test-merges each pair and each against its base with `git merge-tree`; reruns after turns; ⚠ in the sidebar + banner
- [x] Definition of done: `[scripts] check` runs after every agent turn that changed files (not in plan mode, not when the user queued a message); failures go back to the agent up to 2 times in a row
- [x] Review: a fresh read-only (plan mode) chat reviews the workspace diff; Shift-click to review with Codex
- [x] Automatic cross-agent review (`[review] auto = true`): a different agent reviews finished work, findings go back to the author for up to 2 rounds, and approval marks the workspace ready
- [x] Loops: cron schedules and GitHub triggers (issues and PRs by label, polled with `gh`), each run in a fresh workspace, archived automatically if it changes nothing — GitHub polling is unit-tested, not yet run against a real repo
- [x] Cloudflare: a setup script for Tunnel and Access, and the daemon verifies Access sign-ins itself ([CLOUDFLARE.md](CLOUDFLARE.md)); burst runs on Cloudflare Containers with the API key held in the Worker ([CLOUDFLARE-BURST.md](CLOUDFLARE-BURST.md)) — neither has run against a real Cloudflare account yet
- [x] Vercel Sandbox burst runner ([VERCEL.md](VERCEL.md)): one microVM per task, with the API key held at the firewall and work returned as local branches — tested with a local stand-in for the VM, not yet run against Vercel
- [x] Import from Conductor and Pane: active workspaces adopted in place (archiving only forgets them), chats with their recent history, read-only on the source
- [ ] Windows support: the daemon compiles for Windows, uses PowerShell or `$SHELL`, finds npm-installed agents, and CI builds it on `windows-latest` without blocking. Still needed: running it on a real Windows machine, a background service, releases and the desktop app
