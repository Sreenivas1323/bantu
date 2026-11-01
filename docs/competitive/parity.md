# Parity matrix

Seeded on 2026-11-01 from Conductor's public docs and changelog (up to 0.92.0) and Pane's
README, docs and releases (up to v2.4.212). It is updated by the weekly triage routine;
see [README.md](README.md).

**How to read the columns:**
- Conductor and Pane show the version that shipped the feature when known, ✓ if the version is unknown, and — if absent.
- **Bantu** status is `have`, `planned` or `skip`; a milestone of `later` means not yet scheduled.
- Rows marked *(inferred)* come from inspecting the installed app, not from docs.

## Workspaces and git

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| One workspace = one git worktree + branch | ✓ | ✓ | have | M0 |
| Fetch origin and branch from `origin/<base>` | ✓ | ✓ | have | M0 |
| City-name default workspace names | ✓ | — | have | M0 |
| Agent renames branch from first message | ✓ (descriptive names 0.89.0) | — | planned | M1 |
| Workspace numbers | 0.89.0 | — | planned | M1 |
| Create workspace from branch | ✓ | ✓ | planned | M2 |
| Create workspace from PR | 0.15.2 | — | planned | M2 |
| Create workspace from GitHub issue | 0.32.0 / 0.66.0 | — | planned | M2 |
| Create workspace from Linear issue | 0.15.0 | — | planned | later |
| Fork a workspace | 0.25.6 | — | planned | M2 |
| Dispatcher create screen (⌘N, "create more") | 0.63.0 | — | planned | M1 |
| Adopt an existing worktree | — | 2.4.96 | planned | M2 |
| Spare-worktree pool for instant creation | — | ✓ | planned | M2 |
| Branch can only be checked out in one workspace | ✓ | ✓ (git rule) | have | M0 |
| Copy gitignored files via `.worktreeinclude` | ✓ | — | have | M0 |
| Copy `.env*` into new worktrees by default | ✓ | ✓ | have | M0 |
| Uncommitted `.context/` folder for notes/plans/attachments | ✓ | — | planned | M1 |
| Multiple repos per workspace | 0.25.6 | — | planned | later |
| Per-workspace port range | ✓ (10 ports, `CONDUCTOR_PORT`) | ✓ | have (10 ports, `BANTU_PORT`) | M0 |
| Setup script | ✓ | ✓ (`pane.json`) | have | M0 |
| Archive script | ✓ | ✓ | have | M0 |
| Archive keeps branch, restore from history | ✓ | — | have (archive) / planned (restore) | M0 / M1 |
| Save uncommitted changes on archive | ✓ (archive commit, *inferred*) | — | have (`refs/bantu/archive/<id>`) | M0 |
| Crash-safe archive cleanup | — | ✓ | planned | M1 |
| Archive on merge | ✓ | — | planned | M2 |
| Pin workspaces | 0.25.3 | — | planned | M1 |
| Unread indicator | 0.25.11 | ✓ (status dots) | planned | M1 |
| Workspace status (in progress / review / done) | 0.35.0 | ✓ (working / idle / blocked) | planned | M1 |
| Group by repo | 0.35.2 | ✓ | planned | M0 |
| Sidebar sections (emoji, agent-driven moves) | 0.85.0 / 0.86.0 | — | planned | later |
| Manual ordering | 0.92.0 | — | planned | later |
| Stacked PRs (`gh stack`, Graphite) | 0.80.0 / 0.32.0 | — | planned | later |
| GitHub Enterprise, `GH_TOKEN` | 0.22.4 / 0.25.5 | — | planned | M2 |
| Hierarchical changes tree | — | ✓ (Aug 29) | planned | M1 |
| Commit window, keyboard rebase/squash/merge | — | ✓ | planned | M2 |
| PR watches (base changed / deleted) | — | 2.4.183–187 | planned | M2 |
| Branch prefix, delete branch on archive | ✓ | — | planned | M1 |
| Git busy check (merge/rebase in progress) | ✓ (*inferred*) | — | planned | M1 |

## Agents and sessions

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| Claude Code | ✓ (Agent SDK, *inferred*) | ✓ (terminal; ACP chat experimental) | planned (SDK + terminal) | M1 |
| Codex | 0.18.0 (app-server, *inferred*) | ✓ (terminal; app-server chat 2.4.194+) | planned | M2 |
| Cursor | 0.63.0 | ✓ | planned (ACP) | M2 |
| OpenCode | 0.69.0 (2.0 in 0.87.0) | 2.4.170 | planned (ACP) | M2 |
| Any CLI as agent | — | ✓ | planned (terminal driver) | M2 |
| Bundled/pinned agent CLIs | ✓ | — | planned | M2 |
| Structured chat (tool cards, turns) | ✓ (core) | experimental | planned | M1 |
| Switch chat ⇄ terminal on same conversation | — | ✓ (2.4.194+) | planned | M2 |
| Multiple chats per workspace | 0.17.0 (10 tabs 0.57.0) | ✓ (tabs) | planned | M2 |
| Fork a chat mid-turn | 0.78.0 (experimental) | — | planned | M2 |
| Plan mode with approval and feedback | 0.7.3 / 0.21.0 / 0.25.5 | — | planned | M1 |
| Fast mode | 0.40.1 | — | planned | M2 |
| Effort levels | 0.48.5 | — | planned | M2 |
| Manual mode | 0.37.0 | — | planned | M2 |
| Tool approval / permission prompts | 0.41.0 | ✓ (via Notify, 2.4.212) | planned | M1 |
| Steering (message during a turn) | 0.50.0 | — | planned | M1 |
| Message queue | 0.1.0 / 0.54.0 | — | planned | M1 |
| Background-task UI | 0.77.0 | — | planned | M2 |
| Codex goals, personalities | 0.65.0 / 0.45.0 | — | planned | later |
| Model picker | 0.85.0 | — | planned | M2 |
| Saved and shareable loadouts | 0.85.0 / 0.86.0 | — | planned | M2 |
| Model search | 0.87.5 | — | planned | later |
| Bedrock / Vertex / custom providers | 0.13.6 | — | planned | later |
| Vercel AI Gateway keys | 0.79.0 | — | planned | later |
| Sign in with ChatGPT | 0.89.0 | — | planned (uses Codex CLI login) | M2 |
| Sync skills/commands/MCPs between Claude and Codex | 0.57.0 | — | planned | later |
| Todos / task list display | 0.28.4 / 0.33.0 | — | planned | M1 |
| Per-repo prompts (general, review, create PR, fix errors, conflicts, rename branch) | ✓ | — | planned | M2 |
| Context-usage dial, compaction state | ✓ | — | planned | M1 |
| Session resume after app restart | ✓ | ✓ | planned | M1 |
| Agent status detection (working / idle / blocked) | ✓ (structured) | ✓ (screen heuristics) | planned (structured) | M1 |
| Agent notes / memory | — | 2.4.169 (experimental) | planned | later |
| Usage and cost views | 0.91.0 (team heatmap) | ✓ (+ leaderboard) | planned | M2 |
| Keep machine awake while agents work | — | ✓ | planned | M1 |

## Chat UI

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| Attachments (drag and drop) | 0.0.20 | ✓ (up to 50 MB) | planned | M1 |
| Slash commands from `.claude/commands` | 0.1.1 / 0.25.0 | — | planned | M1 |
| @file mentions | 0.46.0 | — | planned | M1 |
| @terminal mentions (pull terminal scrollback into context) | 0.46.0 | ✓ (last 500 lines) | planned | M1 |
| Thinking display | 0.15.1 | — | planned | M1 |
| Chat search, ⌘F find in chat | 0.31.0 / 0.90.0 | ✓ (⌘⇧P search 2.4.189+) | planned | M2 |
| Chat summaries, chat outline | 0.34.2 / 0.92.0 | — | planned | later |
| Markdown, LaTeX, Mermaid rendering | ✓ | — | planned | M1 (md) / later |
| File / HTML / PDF / CSV / JSON previews | 0.45.0–0.87.0 | — | planned | later |
| Run button on bash snippets | 0.91.0 | — | planned | M2 |
| Voice input | 0.91.0 | ✓ (phone dictation) | planned | later |
| Sounds | 0.52.0 | — | planned | later |
| Command palette | 0.14.0 / 0.39.0 | ✓ | planned | M1 |
| Customizable shortcuts | 0.92.0 | — | planned | later |
| Themes, fonts, zoom, low-contrast dark | 0.83.0 | ✓ | planned | M1 (dark/light) |
| Clipboard snippets, popover on selection | — | ✓ | planned | later |
| `pane://`-style deep links to any workspace/chat | ✓ (`conductor://`) | ✓ | planned (`bantu://`) | M2 |

## Diff and review

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| Diff viewer vs base (file list, unified/split) | 0.3.0 → 0.36.0 | ✓ | planned | M1 |
| Per-commit filter, historical diffs | 0.22.0 | — | planned | M2 |
| Mark files viewed | 0.29.1 | — | planned | M1 |
| Inline and multiline comments sent to the agent | 0.38.3 | — | planned | M1 |
| Agent leaves comments on code | 0.29.0 | — | planned | M2 |
| Review action with a custom review model | 0.10.0 / 0.22.6 | — | planned (cross-agent review) | M2 / M4 |
| Edit files directly in the diff | 0.84.0 | ✓ (editor tab) | planned | M2 |
| Cross-file search ⌘⇧F | 0.87.0 | ✓ | planned | M2 |
| Revert a single file | ✓ | — | planned | M1 |
| File explorer tab | — | ✓ | planned | M2 |

## PRs, merge and CI checks

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| Create PR with agent-drafted title/body | ✓ (⌘⇧P) | ✓ (draft PR icon) | planned | M1 |
| Edit PR title/body before creating | 0.34.1 | — | planned | M1 |
| PR page | 0.73.0 / 0.89.0 | — | planned | M2 |
| Checks tab (git status, CI, deployments, comments, todos) | 0.31.1 | — | planned | M2 |
| Forward failing checks to the agent | 0.12.0 | — | planned | M2 |
| GitHub Actions logs | 0.33.2 | — | planned | M2 |
| Vercel deploy status | 0.25.7 / 0.29.2 | — | planned | M2 |
| Sync GitHub PR comments | 0.25.4 | — | planned | M2 |
| One-click approve | 0.83.0 | — | planned | later |
| Change target branch | 0.28.7 | — | planned | M1 |
| Automerge | 0.90.0 | — | planned | later |
| Merge from app + archive | ✓ | ✓ | planned | M2 |

## Terminal and scripts

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| Integrated terminal (pty) | ✓ | ✓ (core) | planned | M1 |
| Terminal restored after app restart | ✓ (*inferred*) | ✓ (headless xterm) | planned | M1 |
| Run scripts, several named actions | 0.70.0 / 0.92.0 | ✓ | planned | M1 |
| Concurrent / non-concurrent run mode | ✓ | — | planned | M1 |
| Stop = SIGTERM process group, SIGKILL after 5 s | ✓ | — | planned | M1 |
| Layered settings (user / project / local / managed) | ✓ (`settings.toml`) | ✓ (`pane.json`) | have (project) / planned (layers) | M0 / M2 |
| Read Conductor settings for migration | n/a | — | have (`.conductor/settings.toml` fallback) | M0 |
| Spotlight (sync a workspace into the main checkout) | 0.55.0 | — | planned | M2 |
| Big terminal mode | 0.48.0 | ✓ (terminal-first) | planned | M2 |
| Agent reads the terminal | 0.29.5 | ✓ (@-mention) | planned (MCP) | M2 |
| direnv | 0.38.4 | — | planned | later |
| Browser preview, named preview URLs | 0.62.0 / 0.63.0 | ✓ (2.4.206 live preview) | planned | M2 |
| Per-process resource monitor | — | ✓ | planned | later |
| Open in IDE / Finder | ✓ | ✓ | planned | M1 |

## Checkpoints

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| Per-turn checkpoint in private git refs | 0.19.0 | — | planned | M2 |
| Revert code + chat to a turn | 0.19.0 | — | planned | M2 |
| Reset chat only | 0.83.0 | — | planned | M2 |

## Orchestration and automation

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| Orchestrator chat that spawns and drives workers | — | 2.4.106 / 2.4.138 (Sessions) | planned (orchestrator mode) | M4 |
| Typed worker reports, `submit --wait` | — | 2.4.187 | planned | M4 |
| Named locks for shared resources | — | 2.4.187 | planned | M4 |
| Mission Control grid / attention inbox | — | 2.4.172 (experimental) | planned (Notify inbox) | M1 / M3 |
| Routines / loops (cron) | 0.85.0 | ✓ (at most hourly) | planned | M4 |
| GitHub and webhook triggers | 0.85.0 / 0.87.0 | — | planned (polled; no webhook server) | M4 |
| Handoff session to another machine | — | 2.4.166 / 2.4.186 | planned | M3 |
| Import sessions from other tools | — | 2.4.185 (incl. Conductor) | planned (Conductor + Pane) | M4 |

## Notifications

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| `notify` tool with typed cards (options / approve / ask / info) | — | 2.4.204 | planned | M1 |
| Overlay over any app, keyboard queue, undo, snooze | — | 2.4.204 | planned | M2 |
| Answer Claude permission prompts remotely | — | 2.4.212 | planned | M3 |
| Phone push with action buttons | ✓ (iOS app) | 2.4.205 | planned (Web Push from daemon) | M3 |
| Push only for turns a human started | — | ✓ | planned | M3 |

## Remote, cloud and team

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| Hosted cloud workspaces | 0.78.0 (Vercel Sandbox, paid) | removed 2.4.123 | skip (BYO cloud instead) | — |
| Self-hosted remote daemon | "in development" (Enterprise) | ✓ (Tailscale / SSH) | planned | M3 |
| Multi-machine read/write/exec | — | 2.4.164 | planned | M3 |
| "All machines" sidebar | — | 2.4.211 | planned | M3 |
| SSH hosts | ✓ (SSH into cloud) | 2.4.178 | planned | M3 |
| Port forwarding / dev-server previews on remote | 0.83.0 | 2.4.173 | planned | M3 |
| Remote desktop | 0.84.0 | — | skip | — |
| Cloud→Mac file sync | ✓ | — | planned | later |
| BYO cloud: AWS one-click with auto-stop | — | — | planned | M3 |
| BYO cloud: Vercel Sandbox bursts | — | — | planned | M4 |
| Mobile app | 0.90.0 (iOS) | ✓ (Capacitor + Expo, 2.4.151) | planned (PWA first) | M3 |
| Remote control of the desktop from a phone | 0.91.2 / 0.92.0 (E2E relay) | ✓ | planned (direct, no relay) | M3 |
| QR pairing | 0.92.0 | ✓ (pairing codes) | planned | M3 |
| Multiplayer (presence, shared chats, follow, reassign) | 0.77.0 | — | planned | later |
| Private workspaces | 0.85.0 | — | planned | later |
| SSO / SCIM | 0.81.0 | — | skip (no accounts) | — |
| Org env vars / shared Cloud Computer image | 0.80.0 | — | planned (repo snapshot) | M4 |
| Setup export/import | — | 2.4.206 | planned | later |

## API, CLI and MCP

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| REST API | 0.77.0 (cloud, paid) | ✓ (local daemon) | have (protocol v1, local + remote) | M0 |
| Idempotent commands | ✓ (`messageId`) | ✓ (request ids) | have (`Idempotency-Key`) | M0 |
| Resumable event stream | polling (`after=`) | SSE | have (SSE `since=`) | M0 |
| Read-only SQL over transcripts | ✓ | — | planned | later |
| CLI | ✓ (`conductor`) | ✓ (`runpane`, npm + PyPI) | planned (`bantu`) | M1 |
| MCP server for external agents | 0.82.0 (hosted, OAuth) | 2.4.131 | planned | M2 |
| MCP tools injected into each session (diff, comments, terminal) | ✓ (*inferred*) | ✓ | planned | M2 |
| Deep links (`prompt=`, `path=`, `branch=`) | ✓ | ✓ | planned | M2 |
| Generated TS types for clients | — | — | have | M0 |

## Integrations

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| GitHub (via `gh`) | ✓ (bundled `gh`, GitHub App) | ✓ | planned (`gh`) | M1 |
| Linear | 0.8.0 / 0.36.5 | — | planned | later |
| Slack | ✓ (cloud) | — | planned (via MCP) | later |
| MCP servers in sessions | 0.1.0 | ✓ | planned | M2 |
| Claude for Chrome | 0.30.0 | — | planned | later |

## Platform and distribution

| Feature | Conductor | Pane | Bantu | Milestone |
|---|---|---|---|---|
| macOS | ✓ | ✓ | planned | M0 |
| Linux | — | ✓ | planned | M0 (daemon) / M3 (desktop) |
| Windows / WSL | — | ✓ | planned | M4 |
| Auto-update | ✓ (while idle) | ✓ (signed) | planned | M3 |
| Homebrew / winget | — | ✓ | planned | M3 |
| Onboarding flow | ✓ | 2.4.189+ | planned | M1 |
| Product analytics | ✓ | on by default, opt-out | skip (no telemetry by default) | — |
| Price | Free local; Pro $50/mo; Teams $60/user | Free (AGPL) | Free (Apache-2.0) | — |
