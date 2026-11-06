# Bantu (బంటు)

**Run a team of coding agents in parallel, each in its own git worktree, on your laptop
or in your own cloud. Free and open source.**

*Bantu* is Telugu for the faithful helper who does the work while you direct it.
See it at **[sreenivas1323.github.io/bantu](https://sreenivas1323.github.io/bantu/)**.

> Status: **alpha**. You can run Claude Code and Codex agents in parallel workspaces, approve
> their actions, review diffs, fix CI and open PRs. Expect rough edges. Codex hasn't been tested
> against the live service yet, and the AWS template hasn't been deployed end to end. See the
> [roadmap](docs/ROADMAP.md).

## Install (macOS and Linux)

```sh
curl -fsSL https://raw.githubusercontent.com/Sreenivas1323/bantu/main/deploy/install.sh | sh
```

This installs `bantud` and the web app to `~/.local`, checks the download against the
release's SHA256SUMS, starts it as a background service (launchd or systemd), and prints the
link to open. You need `git`, and [Claude Code](https://claude.com/claude-code) (or Codex)
signed in with your own account. Bantu never sees your credentials; it runs your CLI.

**Mac app:** download `Bantu-macOS-apple-silicon.dmg` (or `-intel`) from
[Releases](https://github.com/Sreenivas1323/bantu/releases). It bundles the daemon and
starts it when you open the app. It isn't notarized yet, so after dragging it to
Applications run `xattr -dr com.apple.quarantine /Applications/Bantu.app` once.

`bantud url --open` opens the app again later; `bantud service uninstall` removes the service.
To use it from your phone or another machine, see [docs/REMOTE.md](docs/REMOTE.md). To run it
on your own AWS account for ≈ $7/month with idle auto-stop, see [docs/AWS.md](docs/AWS.md).

## Using Bantu

**1. Sign in to an agent.** Bantu runs the `claude` or `codex` CLI already on your machine, so
chats use that CLI's account and your plan's limits apply. Open the accounts dialog from the
status bar to see who's signed in, or to sign in with a subscription, SSO or an API key.

**2. Add a repository.** Click **+ Add repository** in the sidebar and enter a local path,
`owner/repo` or a git URL. Repos added by URL are cloned into `~/bantu/repos`.

**3. Create a workspace.** Each workspace is a git worktree on its own branch, in
`~/bantu/workspaces`, so agents can't step on each other or on your checkout. You can also start
one from an existing branch, a pull request or a GitHub issue. In the issue case, the agent starts
on it right away. The branch begins with a city name, and your first message renames it, so
`bantu/hyderabad` becomes something like `bantu/fix-login-redirect`.

**4. Chat with the agent.** Tool calls appear as cards as they run. Messages you send mid-turn
wait in a queue. Choose how much the agent can do without asking:

| Mode | What the agent does |
|---|---|
| Auto (default) | A classifier approves safe actions. Anything else asks you. |
| Ask first | Asks before edits, commands and other risky actions. |
| Accept edits | Edits files freely. Other risky actions are denied. |
| Plan | Reads and proposes, but changes nothing. |
| Bypass permissions | Does anything. Only for repos you trust. |

Approval requests appear in the chat with **Allow** and **Deny**. If you deny one, you can tell
the agent what to do instead. Turn on notifications in the status bar if you tend to walk away.

**5. Review the work.** The changes view shows the workspace's diff against its base. Comment on
any line, then send the comments to the agent as one message. **Review** opens a separate read-only
chat that checks the diff, and Shift-click runs that review with Codex for a second opinion. If a
turn went wrong, **Restore files to before this turn** puts the files back. The conversation stays,
and the agent is told on its next turn.

**6. Ship it.** **Create PR** asks the agent to commit, push and open the pull request. If CI
fails, **Fix checks** sends the failed checks and their logs to the agent. **Archive** removes the
worktree, keeps the branch and saves uncommitted changes to tracked files in a private ref.
Untracked files aren't saved.

### Running several agents at once

Create as many workspaces as you like. The conflict radar test-merges every ready workspace
against the others and against its base, uncommitted work included, and warns you in the sidebar
before two branches collide.

An agent can also split the work itself. Ask it to break a task into workspaces, and it uses
Bantu's MCP tools to create them, wait for them and read their diffs. An agent can't create more
once the repo has 10 active workspaces.

### Token use

Your global Claude setup (plugins, hooks, skills, MCP servers and `~/.claude/CLAUDE.md`) stays out
of Bantu chats by default, because it loads into every turn. On one heavily customised setup, a
one-line reply went from about 53k input tokens to about 17k. The repo's own `CLAUDE.md` and
`.claude/settings.json` still load. To bring your global setup back, turn on **Load my Claude setup
in Bantu chats** in the accounts dialog. This is on `main` and isn't in v0.1.0-alpha.3 yet.

## Why

- **Free for everyone.** No paid tier and no account. The project runs no servers, so there's nothing to charge for.
- **Open source.** Apache-2.0.
- **Daemon first.** One small Rust daemon, `bantud`, runs your agents. The desktop and web apps are thin clients. With `bantud` installed as a service, agents keep working when you close the window.
- **Bring your own cloud.** The same daemon runs on a spare Mac mini, a Linux box or your own AWS account, at cost price and paid to your provider directly.
- **Your own agents.** Claude Code, Codex, OpenCode, Gemini CLI and Cursor, with your own login.

## Status

| Area | Status |
|---|---|
| Protocol v1 (HTTP commands + resumable SSE events, idempotent retries) | ✅ |
| Repos, worktree workspaces, setup/archive/run scripts, port ranges | ✅ |
| Web UI; one-line installer and background service for macOS/Linux | ✅ |
| macOS desktop app (.dmg, bundles the daemon; not notarized yet) | ✅ |
| Claude Code chat: tool cards, live streaming, queue, stop, plan mode, model picker | ✅ |
| Codex chat (`codex exec --json`) | ✅ (not yet verified against the live service) |
| Workspaces from a branch, PR or GitHub issue; descriptive branch names | ✅ |
| Fix failing CI checks in one click (logs sent to the agent) | ✅ |
| Permission prompts answered in Bantu; desktop notifications | ✅ |
| Diff review with line comments sent to the agent; Create PR; PR checks summary | ✅ |
| Terminals and run scripts (PTY over WebSocket) | ✅ |
| Per-turn checkpoints and "restore files to before this turn" | ✅ |
| ACP agents (OpenCode, Cursor, Gemini) | ✅ (tested with a fake agent; no full turn against a real one yet) |
| Chat ⇄ terminal on one conversation; any CLI in a terminal | ✅ |
| Checks tab: CI, deployments, PR comments, todos | ✅ |
| Per-repo prompts, loadouts, effort and fast mode, forking a chat | ✅ |
| Agents can read terminals, leave comments, notify you and ask questions | ✅ |
| Spotlight: try a workspace in your main checkout | ✅ |
| Remote access via Tailscale, Cloudflare Tunnel or SSH; pairing by QR code | ✅ |
| AWS template with idle auto-stop (Tailscale) | ✅ (lint-checked, not yet deployed end to end) |
| Lean Claude turns: global setup skipped by default | ✅ (on `main`) |
| Installable web app with a phone layout, hostable on its own | ✅ |
| Port forwarding for dev-server previews | ✅ |
| Web Push alerts from the daemon | ✅ (tested with a local push server, not a real browser service yet) |
| AWS template with CloudFront and your own domain | ✅ (lint-checked, not yet deployed end to end) |
| Launch Stack links for both AWS templates ([AWS.md](docs/AWS.md#launch-stack)) | ✅ |
| Cloudflare Tunnel and Access set up by one script; the daemon checks Access sign-ins | ✅ (not yet run against a real account) |
| Orchestration: a chat starts, watches and reviews other workspaces via Bantu's MCP tools | ✅ |
| Conflict radar: warns when workspaces (or a workspace and its base) will conflict | ✅ |
| Definition of done: a repo `check` runs after agent edits; failures go back to the agent | ✅ |
| One-click review in a read-only chat (Claude or Codex) | ✅ |
| Orchestrator board, merging child workspaces into one branch | ✅ |
| Automatic cross-agent review before "Ready" (opt-in) | ✅ |
| Loops: agents on a schedule or from GitHub labels | ✅ (GitHub polling not yet run against a real repo) |
| Import workspaces and chats from Conductor and Pane | ✅ |
| Burst runs on Vercel Sandbox or Cloudflare Containers | ✅ (not yet run against either service) |
| Windows | Preview: builds in CI, not yet tried on a real machine |

## Your own cloud, at cost

| Where `bantud` runs | Your monthly cost |
|---|---|
| Your laptop | $0 |
| Your own machine or VPS, via Tailscale or Cloudflare Tunnel ([guide](docs/CLOUDFLARE.md)) | $0 extra |
| Your AWS account: EC2 t4g over Tailscale, stops itself when idle | ≈ $7 at 4 h/day |
| Your AWS account with CloudFront ([guide](docs/AWS.md)) | ≈ $10 at 4 h/day |
| Your Vercel account: Sandbox microVMs for bursts of parallel tasks ([guide](docs/VERCEL.md)) | Hobby free tier, then ≈ $0.13/CPU-hour |
| Your Cloudflare account: Containers for bursts of parallel tasks ([guide](docs/CLOUDFLARE-BURST.md)) | Workers Paid ($5), then a few cents a task |

The project itself costs about $99 a year to run (Apple code signing). See
[ARCHITECTURE.md](docs/ARCHITECTURE.md#4-bring-your-own-cloud-ranked-from-cheapest).

## Architecture

```
  Desktop (Tauri)     Web app     Agents (MCP)
          \              |            /
     HTTP commands + SSE events (protocol v1, resumable, idempotent)
                          |
   bantud: one Rust binary on macOS, Linux or EC2
     ├─ workspaces   git worktrees, scripts, port ranges
     ├─ agents       your own `claude`, `codex`, `opencode`, `gemini` and `agent` CLIs
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
cargo run -p bantud                # prints http://127.0.0.1:47800/#token=…, open it
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
check = "bun run typecheck && bun test" # after each agent turn that changed files;
                                        # failures go back to the agent (twice at most)
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
