# Bantu for teams

The plan for taking Bantu from one person's agents to a team's: tasks synced with the trackers
teams already use, a shared way of working, a light org layer, and agents that test, watch
deployments and report production problems. Market research behind it:
[competitive/teams.md](competitive/teams.md).

## What it is

- **The same app on a laptop and in the cloud.** A daemon on someone's Mac and a daemon on the
  org's cloud machine are the same program. People use their own; shared agents run on the
  org's.
- **Tasks live where the team already tracks them.** GitHub Issues, Jira and ClickUp first. Bantu
  reads tasks, works on them, and writes progress, status and the pull request back, so nobody
  has to look at Bantu to know what happened.
- **One way of working, shared.** An org's skills, `CLAUDE.md`, commands, MCP servers, checks and
  allowed models come from one git repo, and every member's Bantu stays in step with it.
- **Team totals, never per-person numbers.** Admins see how the team uses agents, not how any one
  person does.
- **Agents for the work around the code.** Testing, watching deployments and live versions, and a
  support agent that notices production problems and files tickets worth reading. Individual users
  get these too.
- **Free, self-hosted or hosted.** Each org can run all of it in its own cloud account. The project
  also hosts a free connector for teams that don't want to run one: it signs people in and keeps
  tasks and the board in sync, while agents, code and compute stay in the team's own cloud
  accounts. Both run the same open-source code, and nothing self-hosted depends on the hosted one.

## Where Bantu stands out

From the research, as of November 2026:

- **No one offers totals-only analytics.** Copilot, Cursor, Claude, Codex, Devin, Warp and Amp all
  show per-person usage.
- **No coding-agent product lists ClickUp,** and only Factory confirms it moves a ticket's status.
- **"Self-hosted" elsewhere usually means only the sandbox.** The agent loop stays with the vendor
  at Cursor, Devin, Warp and Amp. Bantu runs entirely in your account.
- **Production to a good ticket is open ground.** Most tools jump from an alert straight to a PR.
  Few file a deduplicated ticket a person can triage.

## The plan

Each step is useful on its own, and each step works for one person before it works for a team.

### 1. Tasks, both ways

- [x] Connect a repo to GitHub Issues, GitLab, Jira or ClickUp; open tasks sync every 3 minutes
  ([PROTOCOL.md](PROTOCOL.md#tasks)). GitLab works with gitlab.com and self-managed instances, but
  so far it's tested only against a local fake of its API.
- [x] Start a task: a workspace on a branch named after it, and the agent given the task.
- [x] Progress back to the task: notes through the `update_task` MCP tool, and the pull
  request (or GitLab merge request) link once it exists.
- [ ] Status back to the task: In Progress when work starts, In Review when there's a pull request,
  using each tracker's own transitions.
- [ ] Assign to an agent: a label or assignee (e.g. `bantu`) puts a task in the agent queue, like
  loops do for GitHub labels today. Linear after these three.

### 2. A shared team pack

- [ ] An org playbook repo: skills, `CLAUDE.md`, commands, MCP servers, `check` and review
  settings, prompts and allowed models. Each item is marked *enforced* or *default*.
- [ ] Every member's daemon syncs it, applies it to Claude Code and the other agents, and shows
  what changed ("New skill: release-notes").
- [ ] Works with no org service: a git repo is enough.

### 3. The org service

The first piece that needs a server, so it's built to run anywhere: Cloudflare (Workers and D1)
or a container on the org's cloud. The hosted version is this same service.

- [x] The daemon's side of Bantu Connect ([PROTOCOL.md](PROTOCOL.md#organizations-bantu-connect)):
  join with a link, sync every 5 minutes, leave, and clean up when removed (a 401). The org's
  Jira, ClickUp and GitLab connections become trackers members can't remove, attached to the repo
  their scope points at or else every repo. Its Claude access (`team`, `api_key`, `bedrock`,
  `vertex`) is applied to Claude Code in chats, reviews and terminals. Yesterday's team totals are
  posted once a day, as counts only. So far it's tested only against a fake connector in the
  daemon's API tests, not the Connect service itself.
- [x] The app's Organization section in the accounts dialog: join, the org's trackers and Claude
  access with plain guidance, sync status, and leaving.
- [ ] Sign-in with GitHub or Google, or Cloudflare Access; owner, admin and member roles.
- [ ] A team board across trackers: assign a task to a person or to the agent queue.
- [ ] Analytics in team totals: tasks done, cycle time, check pass rate, review verdicts, agent
  hours and spend. Daemons send counts, not events, and a group smaller than 3 people is never
  shown on its own, so a total can't stand in for a person.
- [ ] Claude Code's own telemetry (OpenTelemetry, set through managed settings) goes to a
  collector that drops the person before storing anything.

One caveat to state plainly: the Claude and Codex admin consoles still show their vendor's
per-person usage. Bantu can only promise it adds no per-person data of its own.

### 4. Shared agents on the org's cloud

- [ ] Cloud daemons take tasks from the agent queue, with the same workspaces as a laptop.
- [ ] A test agent: runs the repo's `check`, writes missing tests, and won't call work done
  until they pass.
- [ ] Deployments and live versions: watch the CI/CD the team already has (GitHub deployments,
  Vercel, Cloudflare), show what's live in each environment and from which branch, and post
  preview links to the task. Bantu watches deploys rather than running them.

### 5. Support and production

- [ ] A support agent that watches error tracking (Sentry first), logs and uptime checks.
- [ ] It groups duplicates and files a ticket in the team's tracker with evidence: the error, how
  often, since which deploy, and the likely code. Only one ticket per problem.
- [ ] A person decides whether an agent should take it on; then it goes to the agent queue.

## Decisions made

- **Trackers:** GitHub Issues, GitLab, Jira and ClickUp first. Wherever the org or the person chooses.
- **Hosting:** each org can run Bantu in its own accounts. The project also hosts a connector for
  free: sign-in, the team board and keeping everyone in sync. Agents, code and compute always run
  in the team's own cloud accounts and agent subscriptions, never on the project's machines.
- **Analytics:** team totals only.
- **Not forking Pane or Conductor.** Pane is AGPL-3.0 with a required attribution, and both ship
  almost daily. Bantu works alongside them; it already imports their workspaces and chats.

## Still to decide

- **Multi-user daemons.** Shared cloud daemons need more than one person's sign-in on one daemon.
  That's the biggest change in the plan, and it lands in step 4.
