# Bantu (బంటు)

**Run a team of coding agents in parallel, each in its own git worktree, on your laptop
or in your own cloud. Free and open source.**

*Bantu* is Telugu for the faithful helper who does the work while you direct it.

> Status: **early, milestone M0** (foundations). Not usable for real work yet. Watch or
> star the repo to follow along; see the [roadmap](docs/ROADMAP.md).

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
| Protocol v1 (HTTP commands + resumable SSE events) | M0 — in progress |
| Repos, worktree workspaces, setup/archive scripts, port ranges | M0 — in progress |
| Web UI + Tauri desktop shell | M0 — in progress |
| Agent chat (Claude, then Codex/ACP), diff review, PRs, terminal | M1 |
| Checkpoints/rewind, checks, MCP server, chat ⇄ terminal | M2 |
| Remote daemons, phone/PWA, pairing, AWS one-click | M3 |
| Orchestrator, conflict radar, definition-of-done, cross-agent review | M4 |

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
     ├─ agents       Claude SDK · Codex app-server · ACP · any CLI in a pty
     ├─ checkpoints  per-turn snapshots in private git refs
     └─ SQLite       append-only event log
```

Details: [ARCHITECTURE.md](docs/ARCHITECTURE.md) · [PROTOCOL.md](docs/PROTOCOL.md) ·
[ROADMAP.md](docs/ROADMAP.md)

## Quickstart (development)

Requirements: [Rust](https://rustup.rs) stable, [Bun](https://bun.sh), git. macOS or Linux.

```sh
git clone https://github.com/Sreenivas1323/bantu && cd bantu

cargo run -p bantud            # daemon on 127.0.0.1:47800
cargo run -p bantud -- --print-token

cd apps/web && bun install && bun dev
```

Configure a repo by adding `.bantu/settings.toml` (an existing `.conductor/settings.toml` also works):

```toml
[scripts]
setup = "bun install"
archive = "docker compose down"
```

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
