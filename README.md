# Bantu (బంటు)

**Run a team of coding agents in parallel, each in its own git worktree, on your laptop
or in your own cloud. Free and open source.**

*Bantu* is Telugu for the faithful helper who does the work while you direct it.

> Status: **alpha** (end of M1, early M2). You can run Claude Code and Codex agents in parallel
> workspaces, approve their actions, review diffs, fix CI and open PRs. Expect rough edges;
> see the [roadmap](docs/ROADMAP.md).

## Install (macOS and Linux)

```sh
curl -fsSL https://raw.githubusercontent.com/Sreenivas1323/bantu/main/deploy/install.sh | sh
```

This installs `bantud` and the web app to `~/.local`, checks the download against the
release's SHA256SUMS, starts it as a background service (launchd or systemd), and prints the
link to open. You need `git`, and [Claude Code](https://claude.com/claude-code) (or Codex)
signed in with your own account. Bantu never sees your credentials; it runs your CLI.

`bantud url --open` opens the app again later; `bantud service uninstall` removes the service.
To use it from your phone or another machine, see [docs/REMOTE.md](docs/REMOTE.md). To run it
on your own AWS account for ≈ $7/month with idle auto-stop, see [docs/AWS.md](docs/AWS.md).

## Why

- **Free for everyone.** No paid tier and no account. The project runs no servers, so there's nothing to charge for.
- **Open source.** Apache-2.0.
- **Daemon first.** One small Rust daemon, `bantud`, runs your agents. The desktop app, web app, phone and CLI are thin clients. Agents keep working when you close the window.
- **Bring your own cloud.** The same daemon runs on a spare Mac mini, a Linux box or your own AWS account, at cost price and paid to your provider directly.
- **Structured and terminal.** Rich chat with tool cards, plan approval and rewind, and one click to the raw agent terminal for the same conversation.
- **Any agent.** Claude Code, Codex, OpenCode, Cursor, or any CLI.

## Status

| Area | Status |
|---|---|
| Protocol v1 (HTTP commands + resumable SSE events, idempotent retries) | ✅ |
| Repos, worktree workspaces, setup/archive/run scripts, port ranges | ✅ |
| Web UI; one-line installer and background service for macOS/Linux | ✅ |
| Tauri desktop app | shell only (not packaged yet) |
| Claude Code chat: tool cards, live streaming, queue, stop, plan mode, model picker | ✅ |
| Codex chat (`codex exec --json`) | ✅ (not yet verified against the live service) |
| Workspaces from a branch, PR or GitHub issue; descriptive branch names | ✅ |
| Fix failing CI checks in one click (logs sent to the agent) | ✅ |
| Permission prompts answered in Bantu; desktop notifications | ✅ |
| Diff review with line comments sent to the agent; Create PR; PR checks summary | ✅ |
| Terminals and run scripts (PTY over WebSocket) | ✅ |
| Per-turn checkpoints and "restore files to before this turn" | ✅ |
| ACP agents (OpenCode, Cursor, Gemini), chat ⇄ terminal | M2 |
| Remote daemons, phone/PWA, pairing, AWS one-click | M3 |
| Orchestration: a chat starts, watches and reviews other workspaces via Bantu's MCP tools | ✅ |
| Conflict radar: warns when workspaces (or a workspace and its base) will conflict | ✅ |
| Definition-of-done loop, cross-agent review, orchestrator board | M4 |

## Your own cloud, at cost

| Where `bantud` runs | Your monthly cost |
|---|---|
| Your laptop | $0 |
| Your own machine or VPS, via Tailscale or Cloudflare Tunnel | $0 extra |
| Your AWS account: EC2 t4g + CloudFront, stops itself when idle (M3) | ≈ $10 at 4 h/day |
| Your Vercel account: Sandbox microVMs for bursts of parallel tasks (M4) | Hobby free tier, then ≈ $0.13/CPU-hour |

The project itself costs about $99 a year to run (Apple code signing). See
[ARCHITECTURE.md](docs/ARCHITECTURE.md#4-bring-your-own-cloud-ranked-from-cheapest).

## Architecture

```
  Desktop (Tauri)   Web / PWA   CLI   Agents (MCP)
          \            |         |        /
     HTTP commands + SSE events (protocol v1, resumable, idempotent)
                          |
   bantud — one Rust binary on macOS, Linux, EC2 or a sandbox
     ├─ workspaces   git worktrees, scripts, port ranges
     ├─ agents       your own `claude` CLI today · Codex app-server · ACP · any CLI
     ├─ checkpoints  per-turn snapshots in private git refs
     └─ SQLite       append-only event log
```

Details: [ARCHITECTURE.md](docs/ARCHITECTURE.md) · [PROTOCOL.md](docs/PROTOCOL.md) ·
[ROADMAP.md](docs/ROADMAP.md)

## Quickstart (development)

Requirements: [Rust](https://rustup.rs) stable, [Bun](https://bun.sh), git, and
[Claude Code](https://claude.com/claude-code) logged in. macOS or Linux.

```sh
git clone https://github.com/Sreenivas1323/bantu && cd bantu

bun install && bun run build:web   # the daemon serves the built web app
cargo run -p bantud                # prints http://127.0.0.1:47800/#token=… — open it
```

For UI work, `bun run dev:web` serves the app with hot reload; it proxies `/v1` to the daemon.

Agents run through **your own `claude` CLI and its login**. Bantu never sees your
credentials, and usage counts against your own Claude plan or API key.

To develop without spending tokens, point the daemon at the bundled stand-in:

```sh
BANTU_CLAUDE_BIN=$PWD/scripts/fake-claude.sh cargo run -p bantud
```

It speaks the same `stream-json` as Claude Code. Prompts containing "slow" take 3 s,
"edit" changes files, and "permission" asks for approval.

Configure a repo by adding `.bantu/settings.toml` (an existing `.conductor/settings.toml` also works):

```toml
[scripts]
setup = "bun install"                  # runs when a workspace is created
run = "bun dev --port $BANTU_PORT"      # the ▶ Run button
archive = "docker compose down"        # runs when a workspace is archived
```

Each workspace gets 10 ports from `$BANTU_PORT`. Gitignored `.env*` files, and anything
listed in `.worktreeinclude`, are copied into new workspaces.

## Keeping up with the field

A weekly GitHub Action watches the Conductor and Pane changelogs and opens a
`competitor-update` issue for every new release. We triage those into
[docs/competitive](docs/competitive/), where the parity matrix and idea backlog live.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Good first issues are labelled `good first issue`.

## License

[Apache License 2.0](LICENSE).

Bantu is an independent project. It is not affiliated with, endorsed by, or derived from
the code of Conductor (conductor.build) or Pane. Names belong to their owners.
