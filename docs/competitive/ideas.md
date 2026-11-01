# Idea backlog

Each card covers:
- **Problem**: what hurts today.
- **Better version**: what we'll do better than Conductor and Pane.
- **Milestone** and **Effort**: S (≤ 2 days), M (≤ 1 week), L (more).

New cards come from triaging `competitor-update` issues (see [README.md](README.md)).
Cards are ranked by impact ÷ effort within each section.

## Beyond both

### Chat ⇄ terminal on the same conversation
- **Problem:** Conductor only offers structured chat. Pane is terminal-first, and its chat mode is experimental. People want rich review *and* the raw TUI.
- **Better version:**
  - Every session has a structured view and a terminal view, backed by the same agent session id (`claude --resume`, `codex resume`).
  - Switching is one keystroke.
  - Events from either view feed the same event log, so diffs, checkpoints and notifications keep working.
- **Milestone:** M2 · **Effort:** M

### Conflict radar
- **Problem:** parallel agents edit the same files, and you only find out at PR time.
- **Better version:**
  - After each turn, run `git merge-tree --write-tree` between every pair of active branches, and against the base.
  - Badge the workspaces: "conflicts with *lisbon* in `src/auth.ts`".
  - One click asks an agent to rebase or reconcile.
  - It's local and runs in milliseconds, so it costs nothing.
- **Milestone:** M4 (could move to M2: it's small) · **Effort:** S

### Definition of done
- **Problem:** agents say "done" when tests fail or the UI is broken.
- **Better version:**
  - A repo declares `[done]` checks in `.bantu/settings.toml` (test, lint, typecheck, optional screenshot of a route).
  - A workspace becomes **Ready** only when they pass; on failure the output is sent back to the agent automatically, up to N retries.
- **Milestone:** M4 · **Effort:** M

### Cross-agent review
- **Problem:** an agent reviewing its own work misses its own blind spots.
- **Better version:**
  - When a workspace turns Ready, a *different* agent reviews the diff, e.g. Claude writes and Codex reviews.
  - The review posts inline diff comments that the author agent can address in one click.
- **Milestone:** M4 · **Effort:** M

### Orchestrator mode
- **Problem:** splitting a big task across workspaces is manual.
- **Better version:**
  - An orchestrator session with MCP tools: `spawn_workspace`, `send`, `wait_report`, `lock`, `integrate`.
  - It plans, fans out N workspaces, watches typed reports and merges the results into one branch or a PR stack.
  - The conflict radar runs throughout.
- **Milestone:** M4 · **Effort:** L

### Universal Notify inbox without a relay
- **Problem:** agents block on questions and permission prompts while you're away. Conductor relays through its own servers; Pane needs Tailscale for the phone.
- **Better version:**
  - Every question, permission request and "done" goes to one inbox, shown on desktop, web and phone.
  - Phone alerts use **Web Push sent by the daemon itself** (per-daemon VAPID keys, through browsers' free push services). There's no server of ours.
  - The first answer on any device wins.
- **Milestone:** M1 (desktop) / M3 (phone) · **Effort:** M

### AWS one-click with sleep and wake
- **Problem:** "cloud" means paying a vendor's margin or running a box 24/7.
- **Better version:**
  - A CloudFormation Launch Stack creates EC2 t4g + CloudFront in the user's account.
  - The daemon stops its own instance when idle, and a Lambda Function URL wakes it from any client.
  - About $10/mo at 4 h/day, paid to AWS directly.
- **Milestone:** M3 · **Effort:** M

### Vercel Sandbox bursts
- **Problem:** sometimes you want 10 agents at once, briefly.
- **Better version:**
  - Fan tasks out to Firecracker microVMs in the user's Vercel account, created from a prebuilt repo snapshot.
  - Stream events back, snapshot, then stop.
  - Pay per CPU-second; the Hobby free tier covers experiments.
- **Milestone:** M4 · **Effort:** L

### Import from Conductor and Pane
- **Problem:** switching tools loses history.
- **Better version:**
  - Read Conductor's local SQLite (repos, workspaces, sessions, session_messages) and Pane's `sessions.db`.
  - Adopt their worktrees and import transcripts as read-only history.
  - Repo settings already migrate: Bantu reads `.conductor/settings.toml`.
- **Milestone:** M4 · **Effort:** M

### One API everywhere
- **Problem:** Conductor's API and MCP are cloud-only and paid.
- **Better version:**
  - The local daemon *is* the API: the same protocol, CLI and MCP whether `bantud` runs on your laptop or in your AWS account.
  - Generated TS types, idempotent commands, resumable events.
- **Milestone:** M0 (done) → M2 (MCP) · **Effort:** M

### Cost and context dashboard
- **Problem:** you can't see what parallel agents are spending.
- **Better version:** per-workspace and per-agent token use, cost and context fill, all from local data with no telemetry.
- **Milestone:** M2 · **Effort:** S

## Worth taking from Pane

### Answer permission prompts from anywhere
- **Problem:** a permission prompt stalls the agent until you're at the keyboard.
- **Better version:**
  - In terminal mode, generate a per-session Claude `--settings` file with a `PermissionRequest` hook pointing at the daemon.
  - In structured mode, use the SDK's permission callback.
  - Both route into the Notify inbox.
- **Milestone:** M1 · **Effort:** S

### Agent-operable control surface
- **Problem:** agents can't create workspaces or start sibling agents.
- **Better version:** MCP tools and a `bantu` CLI so agents can create workspaces, start agents, wait for typed reports and open a page beside the user.
- **Milestone:** M2 · **Effort:** M

### Loops that land in fresh worktrees
- **Problem:** scheduled agent jobs pollute your working branch.
- **Better version:**
  - Each run of a cron loop gets a new workspace, ready for review.
  - Runs with no changes are archived automatically.
- **Milestone:** M4 · **Effort:** S

### Handoff across machines
- **Problem:** work started on the laptop has to continue on the always-on box.
- **Better version:**
  - Push the branch, then start the same agent session on another daemon with a verified resumption brief.
  - Confirm the receiving agent actually continued.
- **Milestone:** M3 · **Effort:** M

### Spare-worktree pool
- **Problem:** creating a worktree plus running its setup takes seconds to minutes.
- **Better version:** keep one or two pre-created, pre-set-up worktrees per repo, and rename and re-branch one on demand.
- **Milestone:** M2 · **Effort:** S

### Pull another terminal into context
- **Problem:** "look at the error in my dev server" means copy and paste.
- **Better version:** `@terminal:<name>` inserts the last N lines of any terminal in the workspace.
- **Milestone:** M1 · **Effort:** S

### Keep the machine awake while agents work
- **Problem:** a laptop going to sleep kills long turns.
- **Better version:** hold a `caffeinate` / `systemd-inhibit` lock while any turn is running, and release it when all are idle.
- **Milestone:** M1 · **Effort:** S

### Push only for turns a human started
- **Problem:** notification spam from automated loops.
- **Better version:** send phone push only for turns a person started, grouped by session, with answer buttons on the alert.
- **Milestone:** M3 · **Effort:** S
