# Bantu architecture

Bantu runs coding agents (Claude Code, Codex, OpenCode, Cursor, any CLI) in parallel,
each in its own git worktree, and lets you review, test and ship their work. It is free
and open source, and it is designed so the project itself costs almost nothing to run.

This document covers the five principles behind the design. The wire protocol is in
[PROTOCOL.md](PROTOCOL.md) and the build order in [ROADMAP.md](ROADMAP.md).

## 1. Daemon first; everything else is a client

```
Clients (thin, static, no backend of ours)
  Desktop (Tauri 2) · Web/PWA (static; served by bantud or any static host) · CLI `bantu` · agents via MCP
            \                     |                          |                 /
             ─────────────  Bantu protocol v1 (versioned; Rust types → TS)  ─────────────
               HTTP POST commands (Idempotency-Key)  +  SSE event stream (seq, resumable)
               + WebSocket only for raw PTY bytes and port forwarding
                                        |
bantud: ONE Rust binary, the same on macOS, Linux, EC2, a sandbox (Windows later)
  ├─ workspace manager  git worktrees, 10-port ranges, .worktreeinclude + .env* copy,
  │                     setup/run/archive scripts from .bantu/settings.toml, branch naming
  ├─ agent sessions     normalized ChatItems; message queue; cancel; permission requests
  │     claude      (the user's `claude` CLI, stream-json, one process per turn)   ← today
  │     codex       (`codex exec --json`, one process per turn)
  │     acp         (OpenCode, Gemini, Cursor: JSON-RPC over stdio, one process per turn)
  │     terminal    (any CLI in a pty)   ← M2
  ├─ permissions        Claude PermissionRequest HTTP hook → approval in any client
  ├─ review             diff vs merge-base (throwaway index), line comments → agent, PR status via gh
  ├─ pty manager        portable-pty, 512 KB scrollback replayed to every viewer, process-group kill
  ├─ checkpoints        temp GIT_INDEX_FILE → write-tree/commit-tree → refs/bantu/cp/<session>/<turn>
  ├─ git intelligence   busy-check today; conflict radar (git merge-tree --write-tree) later
  ├─ MCP server         (M2) diff, comments, terminal, notify/ask, spawn_workspace, send, wait_report
  ├─ scheduler          (M4) cron loops, GitHub triggers (polled via gh; no webhook server), idle auto-stop
  └─ SQLite (WAL)       append-only event log + projections
```

Why:
- **Agents keep running when the window closes.** The daemon runs as a launchd/systemd service.
- **Local and remote share one code path.** Every client works against any daemon, on your laptop or in your cloud.
- **HTTP + SSE passes through every free front door**: CloudFront, Tailscale Serve, Cloudflare Tunnel, plain localhost. It also resumes cleanly on flaky phone networks.
- **Idempotency keys make retries safe**, and the event log makes reconnects lossless (`?since=<seq>`).
- **Agents use the user's PATH.** At startup the daemon adopts its login shell's `PATH`. A daemon launched from the Dock therefore finds `claude`, `bun` and `node` just like a terminal does.

**State model.**
- Every change is an `Event` appended to SQLite with a gap-free `seq`.
- Tables like `repos` and `workspaces` are projections, updated in the same transaction as the event.
- Clients load a snapshot (`/v1/state`), then follow events.
- This also gives us an audit log and lets a session be replayed.
- Some state is deliberately *not* in the log:
  - Live text deltas go out as SSE `event: delta` frames with no `seq`, so the log doesn't fill with fragments.
  - Terminals and pending permission requests live in memory and end with the process. On startup the daemon records any terminal the log still thinks is open as closed.

## 2. Structured by default, terminal when you want it

Every agent session is structured chat by default: tool-call cards, plan approval, a
message queue, rewind, a context-usage dial. One click switches the **same conversation**
to a raw terminal (`claude --resume <id>`, `codex resume`) and back.

- **Structured** drivers speak each agent's own machine interface: Claude Code's `stream-json` output, Codex `app-server`, or ACP. Their output is normalized into one schema (`ChatItem`).
- **The terminal driver** runs any CLI in a pty. It's the escape hatch for agents with no structured interface, and for people who just want the TUI.

Conductor has no terminal mode for agents, and Pane's chat mode is experimental. Bantu treats both as first class.

### Why the Claude driver uses the CLI, not a Bun sidecar

Conductor embeds the Claude Agent SDK in a roughly 100 MB Bun sidecar. Bantu instead runs the user's own `claude` CLI once per turn:
- `claude -p --output-format stream-json --include-partial-messages`;
- the first turn uses `--session-id <uuid>`, and later turns `--resume <uuid>`.

| | CLI per turn (Bantu) | SDK sidecar |
|---|---|---|
| Size | Nothing extra to ship | ~100 MB runtime |
| Login | Whatever the user's Claude Code uses (subscription or API key) | Usually an API key for third-party apps |
| Restarts | Stateless: the next turn resumes by id | A long-lived process to supervise |
| Permission prompts | `PermissionRequest` HTTP hook via `--settings` | In-process `canUseTool` callback |
| Rewind | Files only (checkpoints); the agent is told | Same |
| Cost | ~1–3 s of CLI startup per turn; no in-process SDK hooks | — |

The rest of the turn plumbing:
- **Hook token.** The permission hook's token travels in the environment (`BANTU_HOOK_TOKEN`), never in argv, so other users can't read it from `ps`.
- **Undoing a turn.** Each turn starts with a checkpoint of the worktree (`refs/bantu/cp/<session>/<turn>`). `session.rewind` restores it and tells the agent on its next message.

## 3. The intelligence layer

What makes the product feel smart is mostly structure plus good built-in prompts.

**Parity with Conductor:**
- Per-repo prompts: review, create PR, fix CI, resolve conflicts, rename branch.
- The branch is named after the task from the first message (`bantu/hyderabad` → `bantu/fix-login-redirect`).
- A derived workspace status, and a checks view (git status, `gh pr checks`, Actions logs, deploy status).
- Failing checks and PR comments are forwarded to the agent; diff comments go to the agent.
- Plan mode, effort and fast mode, model loadouts, several chats per workspace, forking a chat.

**Beyond both** (details in [competitive/ideas.md](competitive/ideas.md)):
1. **Orchestrator mode.** One prompt is split into N workspaces, which are dispatched, watched (typed reports, named locks) and integrated into a single branch or stack.
2. **Conflict radar.** `git merge-tree --write-tree` between active branches warns you that "A and B will conflict in `auth.ts`" before either PR exists. It runs locally, so it's free.
3. **Definition of done.** A repo's `done` checks (tests, lint, screenshot) must pass before a workspace is marked Ready; the agent retries automatically.
4. **Cross-agent review.** Claude writes and Codex reviews, or the other way round.
5. **Universal Notify inbox.** Permission prompts, questions and "done" reach desktop, web and phone. Phone alerts use Web Push sent from the daemon itself (VAPID keys per daemon); no server of ours.
6. **Import from Conductor and Pane.**
7. **One API everywhere.** The same REST/MCP/CLI works locally and remotely. Cost and context dashboard. No telemetry by default.

## 4. Bring-your-own cloud, ranked from cheapest

Bantu runs no cloud of its own. If you want agents running while your laptop sleeps, you
run `bantud` somewhere you control:

| Tier | What | Your monthly cost |
|---|---|---|
| T0 Local | `bantud` on your Mac or Linux machine | $0 |
| T1 Own box | `curl … \| sh` on a Mac mini, Linux box or VPS; reach it with Tailscale (free personal plan) or Cloudflare Tunnel (free) | $0 extra |
| T2 AWS one-click | CloudFormation "Launch Stack": EC2 t4g.medium (Graviton) + 30 GB gp3 + Elastic IP. A CloudFront distribution (free tier: 1 TB, 10M requests) gives HTTPS + SSE/WebSocket with no load balancer. The security group only allows CloudFront's origin prefix list. The daemon stops its own instance when idle, and a Lambda Function URL "wake" endpoint (free tier) starts it from any client. | ≈ $6 idle floor (IPv4 $3.65 + EBS $2.40) + ≈ $0.034 per running hour → **≈ $10/mo at 4 h/day** |
| T2b AWS + Tailscale | The same, without the Elastic IP or CloudFront; the daemon joins your tailnet | ≈ $5/mo at 4 h/day |
| T3 Vercel Sandbox bursts | Per-task Firecracker microVMs in your Vercel account: create from a prebuilt repo snapshot → a headless daemon runs the agent → events stream back → snapshot + stop | Hobby: 5 CPU-h free, 45-min sessions; Pro: ≈ $0.13/CPU-h + $0.02/GB-h |

What each provider is for:
- **CloudFront** is a front door and CDN, not compute.
- **Vercel** can host the static web/PWA client, and Vercel Sandbox is a burst runner.
- **Vercel Functions cannot host the daemon**: they're capped at 300–800 s, with no persistent disk or long-lived terminals.
- **Spot instances** are offered only for unattended loops, because an interruption kills live terminals.

Prices were checked in November 2026; see the sources in [competitive/README.md](competitive/README.md).

## 5. The project's own running cost ≈ $99 a year

| Need | How | Cost |
|---|---|---|
| Code, CI (including macOS runners), binaries, updater manifest | Public GitHub repo, Actions, Releases | $0 |
| Website and web client | GitHub Pages or Vercel Hobby | $0 |
| macOS signing and notarization | Apple Developer Program | $99/yr |
| Windows signing (later) | SignPath Foundation (free for OSS) | $0 |
| Accounts, relay, telemetry backend, database | None: pairing is a QR code with daemon URL + token (the daemon stores only a hash) | $0 |

Hard rule: **no feature may require a server run by the project.**

## Tech stack

| Part | Choice |
|---|---|
| Daemon | Rust: tokio, axum (HTTP, SSE, WebSocket), sqlx (SQLite, WAL), portable-pty, the `git` CLI (not libgit2, so behavior matches the user's git), `ts-rs` for TS bindings |
| Agents | The user's own CLIs: `claude` today; `codex app-server` and ACP agents next |
| UI | React + Vite + Tailwind; react-markdown; xterm.js; a small unified-diff viewer; bun for tooling |
| Desktop | Tauri 2 (updater, `bantu://` deep links) |
| Mobile | PWA first; a native wrapper later |
| Deploy | CloudFormation YAML (Launch Stack URL), `install.sh`, `@vercel/sandbox` runner |

## Repository layout

```
crates/protocol    wire types (source of truth) → packages/protocol (generated TS)
crates/git         thin async wrapper around the git CLI
crates/daemon      bantud
apps/web           React web UI (also served by bantud)
apps/desktop       Tauri 2 shell
docs/              architecture, protocol, roadmap, competitive research
scripts/           changelog watcher, fake-claude.sh (offline Claude stand-in for UI work and tests)
deploy/            (M3) install.sh, AWS CloudFormation, Vercel Sandbox runner
```

## Known risks

- **Claude login.** Bantu never handles Claude credentials. It runs the user's own `claude` CLI, which uses whatever the user logged in with (a subscription or an API key). We still keep an eye on Anthropic's terms for third-party tools.
- **Clean-room only.** Conductor is proprietary and Pane is AGPL-3.0. We study public docs and changelogs and never copy code or assets.
- **CLI output format drift.** We parse Claude Code's `stream-json` and hook schemas. A parser unit test, an integration test with `scripts/fake-claude.sh`, and occasional real-CLI smoke runs catch changes.
