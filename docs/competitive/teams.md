# Bantu for teams: market research

Researched 2026-11-17. This looks at what the products teams compare Bantu with actually ship, where nobody
has done the job well, and what Bantu should build first for teams.

How to read the links: each claim about another product is followed by a link to its source.
The link text is the page's own date, or `n.d.` if the page has no date. Every page was read on 2026-11-17.
**Unconfirmed** means we couldn't find it in a primary source. *(summary)* means it appeared only in a
search-engine summary, not on a page we loaded.

## The short version

- **Two-way tracker sync is now standard for Jira and Linear, and missing for ClickUp.** Copilot, Cursor,
  Devin, Factory, Warp, Amp and Codex take a ticket and post progress back to it. None of them lists ClickUp.
- **Every vendor shows per-person analytics.** Leaderboards, per-user spend and per-user CSV exports are the
  default. None of them offers totals only.
- **"Self-hosted" usually means only the sandbox runs in your cloud.** The agent loop or planner stays with
  the vendor at Cursor, Devin, Warp and Amp. Fully self-hosted options (Factory, Coder, OpenHands Enterprise)
  are sold to enterprises.
- **Production-to-fix is crowded, but production-to-*good-ticket* is not.** Most tools go from an alert
  straight to a PR or a report. Few file a deduplicated ticket that a person can triage.
- **The market is unsettled.** Google Jules shuts down on 2026-12-31. Conductor announced it was joining Meta
  and then reversed the shutdown nine days later. Atlassian is retiring standalone Rovo Dev. Copilot, Sentry
  Seer, Linear and Rovo have all moved to usage credits this year.

## Comparison 1: where it runs, trackers, analytics, self-hosting

| Product | Runs | Trackers and sync direction | Org analytics granularity | Self-host |
|---|---|---|---|---|
| GitHub Copilot | Both. Cloud agent in a GitHub sandbox, an Actions runner or your own "remote" environment [n.d.](https://docs.github.com/en/copilot/concepts/agents/coding-agent/about-coding-agent). A desktop app has been in preview since June [2026-06-02](https://github.blog/2026-06-02-github-copilot-app-the-agent-native-desktop-experience/) | GitHub Issues. Jira and Linear are two-way: progress streams into the ticket and you steer by comment [2026-06-25](https://github.blog/changelog/2026-06-25-github-copilot-for-jira-is-now-generally-available/), [2026-07-23](https://github.blog/changelog/2026-07-23-copilot-cloud-agent-for-linear-is-now-generally-available/). Claude and Codex can be assigned too [2026-02-04](https://github.blog/changelog/2026-02-04-claude-and-codex-are-now-available-in-public-preview-on-github/) | Per user, daily [2026-02-27](https://github.blog/changelog/2026-02-27-copilot-metrics-is-now-generally-available); credits per user [2026-06-19](https://github.blog/changelog/2026-06-19-ai-credits-consumed-per-user-now-in-the-copilot-usage-metrics-api). Per team unconfirmed | Self-hosted runners, for Actions sessions only [n.d.](https://docs.github.com/en/copilot/concepts/agents/coding-agent/about-coding-agent) |
| Cursor | Both. IDE plus cloud agents [n.d.](https://cursor.com/pricing) | Linear: assign the issue; live status and follow-ups in Linear [n.d.](https://cursor.com/docs/integrations/linear). Jira (needs Rovo): posts the result and PR link back [2026-05-19](https://cursor.com/changelog/05-19-26). Slack and GitHub. Issue status changes unconfirmed | Per user, per directory group, per repo and org totals; leaderboard; API on Enterprise only [n.d.](https://cursor.com/docs/account/teams/analytics) | Agent machines on your laptop or in worker pools on AWS, Coder, Cloudflare, Modal, Vercel and others. Planning stays in Cursor's cloud [2026-09-02](https://cursor.com/changelog/self-hosted-machines) |
| Devin | Both. Devin cloud, plus Devin Desktop (Windsurf, renamed in June) [n.d.](https://docs.devin.ai/desktop/devin-desktop-faq) | Jira and Linear: assign, label or mention; comments and the PR link come back; reply to steer [n.d.](https://docs.devin.ai/integrations/jira), [n.d.](https://docs.devin.ai/integrations/linear). Status sync unconfirmed | Per org, per user and per session; budgets by team and user [n.d.](https://docs.devin.ai/admin/billing/enterprise.md) | Dev machines in your VPC, with the planner in Cognition's cloud [n.d.](https://docs.devin.ai/enterprise/vpc/overview); a single-tenant option [n.d.](https://docs.devin.ai/enterprise/deployment/overview) |
| Factory | Both. CLI, desktop app and hosted machines [n.d.](https://factory.com/pricing) | Linear two-way, and it moves the issue to "started" [n.d.](https://docs.factory.com/integrations/linear). Jira and Slack are listed; how they work is unconfirmed | Org, team and user through an API; OpenTelemetry export per user or as totals [n.d.](https://docs.factory.com/enterprise/usage-cost-and-analytics.md) | Cloud, hybrid or fully air-gapped [n.d.](https://docs.factory.ai/enterprise/network-and-deployment) |
| Claude Code (Team/Enterprise) | Both. CLI, desktop and IDE; cloud sessions on Anthropic VMs [n.d.](https://code.claude.com/docs/en/claude-code-on-the-web) | GitHub via `@claude` in Actions [n.d.](https://code.claude.com/docs/en/github-actions). Linear only through Linear's own coding sessions [n.d.](https://linear.app/docs/coding-sessions). Jira only through MCP; no first-party trigger | Org usage, a top-10 leaderboard, an all-user CSV and a per-user spend report; per-user API on Enterprise [n.d.](https://code.claude.com/docs/en/analytics) | Self-hosted environments, public beta [n.d.](https://code.claude.com/docs/en/self-hosted-environments) |
| OpenAI Codex | Both. Cloud tasks, plus a desktop app, CLI and IDE extension [n.d.](https://learn.chatgpt.com/docs/cloud) | GitHub, GitLab and Slack. Linear two-way: activity panel plus a closing summary [n.d.](https://developers.openai.com/codex/integrations/linear). Jira unconfirmed | Per user with leaderboards, Enterprise only *(summary)* [n.d.](https://developers.openai.com/codex/enterprise/governance) | Unconfirmed for Codex cloud tasks. The Agents API has self-hosted sandboxes [n.d.](https://developers.openai.com/api/docs/guides/agents-api/environments/self-hosted) |
| Warp | Both. Open-source terminal [2026-04-28](https://www.warp.dev/newsroom/2026/4/28/warp-open-sources-its-agentic-development-environment) and Oz cloud agents [2026-02-10](https://www.warp.dev/newsroom/2026/2/10/warp-launches-oz-the-orchestration-platform-for-cloud-coding-agents) | Linear or Jira (one at a time), Slack, Teams, GitHub, GitLab and Azure DevOps; results post back [n.d.](https://docs.warp.dev/factories/connect-your-factory/) | Team totals, per-user rollups and message events, Enterprise [n.d.](https://docs.warp.dev/enterprise/enterprise-features/analytics-api/) | Workers in your VPC (Enterprise); Warp orchestrates [n.d.](https://www.warp.dev/oz/self-hosting) |
| Amp | Both. Cloud "orbs" plus desktop, mobile and CLI [n.d.](https://ampcode.com/manual) | Linear, GitHub, GitLab and Slack two-way; no Jira [n.d.](https://ampcode.com/docs/integrations) | Per member, model and mode: spend, tokens, orb hours; CSV [n.d.](https://ampcode.com/docs/collaborate/workspaces) | Self-hosted orbs (Helm, Docker, E2B…). The agent loop and inference stay with Amp [n.d.](https://ampcode.com/docs/orbs/self-hosted) |
| OpenHands | Cloud, self-hosted Enterprise, and a free MIT local app [n.d.](https://docs.openhands.dev/enterprise/enterprise-vs-oss.md) | Jira Cloud, Jira Data Center (Enterprise) and Linear, by mention or label; write-back unconfirmed [n.d.](https://docs.openhands.dev/openhands/usage/cloud/project-management/overview.md) | Usage and budgets across all users (Enterprise); per user unconfirmed [n.d.](https://www.openhands.dev/blog/super-admin-and-smarter-automations-in-openhands-enterprise) | Yes, Helm on Kubernetes [n.d.](https://www.openhands.dev/blog/openhands-cloud-self-hosted-secure-convenient-deployment-of-ai-software-development-agents) |
| Conductor | Mac app; cloud workspaces on Vercel sandboxes, GitHub only [n.d.](https://www.conductor.build/docs/cloud/faq) | Linear and GitHub Issues, one-way into a workspace [n.d.](https://www.conductor.build/docs/guides/issue-to-pr) | None found; "experimental cloud cost estimates" [n.d.](https://www.conductor.build/changelog/0.77.0-early-access-multiplayer-api-background-tasks-performance) | "In development", through Enterprise sales [n.d.](https://www.conductor.build/docs/cloud/faq) |
| Pane | Desktop on macOS, Windows and Linux, with a browser/phone remote; your own machines only [n.d.](https://github.com/greenfield-inc/Pane) | None built in; through MCP or the CLI [n.d.](https://github.com/greenfield-inc/Pane) | Per user only; opt-in leaderboard [n.d.](https://github.com/greenfield-inc/Pane) | Yes, including the relay [n.d.](https://github.com/greenfield-inc/Pane) |
| Linear (as a hub) | Cloud; its own coding agent runs in a cloud environment [2026-09-24](https://linear.app/changelog/2026-09-24-new-controls-for-linear-coding-agent) | Hosts agents from Cursor, Copilot, Codex, Devin, Warp and Sentry, two-way through an agent-session API [n.d.](https://linear.app/enablement/guide/ai-agents) | Insights by assignee and agent-session status, Business plan [n.d.](https://linear.app/docs/insights); cost per session [n.d.](https://linear.app/docs/ai-credits) | Not found |
| Atlassian Rovo Dev | Both: CLI, and a Jira coding agent in a cloud sandbox *(summary)* | Jira items can be assigned to Claude Code, Cursor and Copilot *(summary)*; a Forge API connects outside agents [n.d.](https://developer.atlassian.com/platform/forge/remote-agents-in-jira/) | Per-user credit use, 90 days of history *(summary)* | Cloud only, according to a community answer (unconfirmed) |
| ClickUp | Cloud. Codegen agents read a task and return a PR in its thread [n.d.](https://clickup.com/agents/codegen) | ClickUp tasks via assign, mention or automation (help page blocked; unconfirmed) | Unconfirmed | No; data residency only [n.d.](https://clickup.com/pricing) |
| Sentry Seer | Cloud; MCP for local agents [n.d.](https://docs.sentry.io/product/ai-in-sentry/seer.md) | Hands fixes to Claude Code, Cursor or Copilot [n.d.](https://docs.sentry.io/product/ai-in-sentry/seer.md); two-way Jira issue sync [n.d.](https://docs.sentry.io/organization/integrations/issue-tracking/jira/) | Unconfirmed | No. AI features are closed source and missing from self-hosted Sentry [n.d.](https://develop.sentry.dev/self-hosted/) |

## Comparison 2: tests and previews, production, price, admin push

| Product | Tests, deploys, previews | Production → ticket or fix | Price (Nov 2026) | Admin push of rules/skills |
|---|---|---|---|---|
| GitHub Copilot | Runs tests and linters [n.d.](https://docs.github.com/en/copilot/concepts/agents/coding-agent/about-coding-agent); a "deployments" canvas in the app [2026-06-02](https://github.blog/2026-06-02-github-copilot-app-the-agent-native-desktop-experience/); previews unconfirmed | Through Sentry Seer handoff [n.d.](https://sentry.io/changelog/gh-copilot-agent-GA/) | Business $19 and Enterprise $39 per user, with credits included. AI Credits replaced premium requests on 2026-06-01 [n.d.](https://github.blog/news-insights/company-news/github-copilot-is-moving-to-usage-based-billing/); new $100 Max tier [n.d.](https://github.com/features/copilot/plans) | Org instructions; org and enterprise skills [2026-09-22](https://github.blog/changelog/2026-09-22-new-features-and-improvements-in-copilot-for-jetbrains/) |
| Cursor | Computer use and browser recordings; Bugbot Autofix [n.d.](https://cursor.com/docs/bugbot); deploys and previews unconfirmed | Automations on Sentry and PagerDuty [n.d.](https://cursor.com/docs/cloud-agent/automations) | Teams $30+/user; Premium $30. Bugbot moved to usage billing in May [n.d.](https://cursor.com/docs/bugbot/legacy-pricing). Sold through "X Subscription Corp", bundled with Grok [n.d.](https://cursor.com/pricing) | Enforced team rules [n.d.](https://cursor.com/docs/rules); team marketplace [n.d.](https://cursor.com/pricing) |
| Devin | Unconfirmed | Auto-Triage on Sentry and Datadog alerts, can open PRs [2026-05-18](https://cognition.ai/blog/auto-triage) | Teams $80/mo + $40/seat; Pro $20; Max $200; self-serve ACUs dropped [n.d.](https://devin.ai/pricing) | Enterprise-wide knowledge, skills and plugins [n.d.](https://docs.devin.ai/product-guides/knowledge) |
| Factory | "Investigates, implements, and tests" [n.d.](https://docs.factory.com/integrations/linear); deploys and previews unconfirmed | Incident automation: root cause, fix PRs, runbook updates [n.d.](https://docs.factory.ai/software-factory/incident-response) | Teams $60 + $40/seat, up to 10 seats [n.d.](https://factory.com/pricing) | Org settings pushed by MDM or endpoint, which users can't weaken [n.d.](https://docs.factory.com/enterprise/hierarchical-settings-and-org-control.md) |
| Claude Code | Auto-fixes CI failures and review comments on PRs [n.d.](https://code.claude.com/docs/en/claude-code-on-the-web); deploys and previews unconfirmed | Routines triggered by API, e.g. a Sentry alert [n.d.](https://code.claude.com/docs/en/routines) | Team $20/$25 standard seat, $100/$125 premium; Enterprise $20/seat + usage [n.d.](https://claude.com/pricing) | Server-managed settings: hooks, CLAUDE.md, plugins, MCP policy [n.d.](https://code.claude.com/docs/en/server-managed-settings) |
| OpenAI Codex | Setup scripts; tests shown only in examples [n.d.](https://learn.chatgpt.com/docs/cloud) | Only through Datadog's MCP [n.d.](https://www.datadoghq.com/blog/openai-datadog-ai-devops-agent.md); native unconfirmed | Business $20/$25 per user; a cloud task typically costs 2–15 credits [n.d.](https://learn.chatgpt.com/docs/pricing.md) | Enforced `requirements.toml`; policies per group [n.d.](https://learn.chatgpt.com/docs/enterprise/managed-configuration) |
| Warp | Unconfirmed | Sentry webhook → read-only triage report; stops before a fix [n.d.](https://docs.warp.dev/factories/use-cases/production-incident-resolution.md) | Business $50/user; cloud runs at API rates + 20% [n.d.](https://www.warp.dev/pricing) | Enforced org settings [n.d.](https://docs.warp.dev/enterprise/team-management/admin-panel/) |
| Amp | Authenticated preview URLs ("portals") [n.d.](https://ampcode.com/docs/orbs/portals); automations run tests after a push [n.d.](https://ampcode.com/docs/orbs/automations) | Hourly error-triage example [n.d.](https://ampcode.com/docs/orbs/automations) | Individual $20; Teams at no extra charge; Enterprise custom [n.d.](https://ampcode.com/pricing) | Workspace skills, plugins, MCP, global AGENTS.md [n.d.](https://ampcode.com/docs/collaborate/workspaces) |
| OpenHands | Unconfirmed | Datadog error → GitHub issue → fix PR, via Actions [n.d.](https://www.openhands.dev/blog/debugging-production-issues-with-ai-agents-automating-datadog-error-analysis) | Teams $500/mo for 10 users, $50 per extra seat [n.d.](https://openhands.dev/pricing) | Default models, plugin marketplace; pushing skills unconfirmed |
| Conductor | Checks tab with CI and deployments [n.d.](https://www.conductor.build/docs/guides/issue-to-pr) | None found | Pricing page lists no plans [n.d.](https://www.conductor.build/pricing); our README's $50/mo Pro is now unconfirmed | Repo `conductor.json`, shared loadouts |
| Pane | Built-in browser for dev servers; scheduled Loops [n.d.](https://github.com/greenfield-inc/Pane/releases) | None found | Free (AGPL); paid cloud and teams "planned" [n.d.](https://runpane.com/pricing) | Settings import/export only |
| Linear | Agent tests in a browser with screenshots [2026-08-20](https://linear.app/changelog/2026-08-20-coding-environments) | Through the Sentry agent | Business $16/user [n.d.](https://linear.app/pricing). AI credits are prepaid, none included: tokens at cost + sandbox time [n.d.](https://linear.app/docs/ai-credits) | Workspace and team guidance *(summary)*; model allowlist [2026-09-24](https://linear.app/changelog/2026-09-24-new-controls-for-linear-coding-agent) |
| Rovo Dev | Unconfirmed | Unconfirmed | $20/dev with 2,000 credits [n.d.](https://www.atlassian.com/software/rovo-dev/pricing); standalone being retired into Jira plans [n.d.](https://www.atlassian.com/licensing/rovo-dev) | Repo rules and skills [n.d.](https://www.atlassian.com/licensing/rovo-dev) |
| ClickUp | PR-review templates only [n.d.](https://clickup.com/brain/agents/templates/listings/pull-request-reviewer) | Unconfirmed | $9–$17/user with AI credits; Max $100 with unlimited agents [n.d.](https://clickup.com/pricing) | Custom Skills on Max [n.d.](https://clickup.com/pricing) |
| Sentry Seer | n/a | Analyzes new issues, auto-opens fix PRs [n.d.](https://sentry.io/product/seer/); alerts create Jira tickets [n.d.](https://docs.sentry.io/organization/integrations/issue-tracking/jira/) | Team $26, Business $80; Seer extra. Moves to usage credits and drops Seer Code Review on 2026-12-08 [n.d.](https://docs.sentry.io/product/ai-in-sentry/seer.md) | Org AI toggles; can block PR creation |

**Dropped or brief:**
- **Google Jules** stopped new sessions on 2026-11-11 and shuts down on 2026-12-31, pointing users to Antigravity [2026-10-29](https://jules.google/docs/changelog/2026-10-29/). It never had a team plan [n.d.](https://jules.google/docs/usage-limits/).
- **Coder Agents** is fully self-hosted, including air-gapped [n.d.](https://coder.com/blog/coder-agents-ga).
- **Ona** (formerly Gitpod) runs in the customer's VPC and has a Sentry → Linear ticket → draft PR template [n.d.](https://ona.com/docs/ona/automations/automations-in-practice).
- **AI SRE agents:**
  - Vercel Agent bills inference at cost plus $0.25 per 1M tokens [n.d.](https://vercel.com/docs/agent/pricing).
  - Datadog Bits averages about $4 per investigation [n.d.](https://www.datadoghq.com/blog/bits-investigations-cost-speed-accuracy/).
  - PagerDuty's agent pricing is unconfirmed.

## What nobody does well

1. **Analytics without watching individuals.** Every product above with org analytics shows per-person data:
   leaderboards, per-user spend, per-user CSVs. Factory comes closest, with an OpenTelemetry export that can be
   totals only [n.d.](https://docs.factory.com/enterprise/usage-cost-and-analytics.md). Nobody makes totals-only
   the design. **A catch for Bantu:** the Claude and Codex admin dashboards still show each person's usage to
   that vendor's admin [n.d.](https://code.claude.com/docs/en/analytics). Bantu can only promise that *Bantu*
   won't add per-person data.
2. **ClickUp.** No coding-agent vendor we checked integrates with ClickUp. ClickUp's own Codegen is cloud-only
   and sold through sales *(summary)*. Its public API does have task webhooks, including assignee changes
   *(summary)* [n.d.](https://developer.clickup.com/docs/webhooktaskpayloads).
3. **Writing status back to the tracker.** Most vendors post comments and PR links. Only Factory confirms it
   changes the issue's state [n.d.](https://docs.factory.com/integrations/linear). Fields, estimates and moving
   tickets through a board are mostly not done.
4. **One tracker, any agent, outside a vendor's cloud.** Linear, Jira and GitHub act as hubs, but each agent
   they host runs in its vendor's cloud. No hub lets a team use Claude for one ticket and Codex for the next on
   the team's own machines.
5. **Truly self-hosted at a team price.**
   - Cursor, Devin, Warp and Amp keep the planner or agent loop in their own cloud.
   - Factory's air-gapped mode, Coder's AI tiers and OpenHands Enterprise are sold to enterprises.
   - OpenHands Teams is $500/mo [n.d.](https://openhands.dev/pricing). Pane is free but has no team layer.
6. **Same product on the laptop and in the cloud.** Copilot's desktop app and cloud agent are separate
   surfaces, and so are Cursor's IDE and cloud agents. Pane and Conductor are close, but Conductor's cloud only
   supports GitHub [n.d.](https://www.conductor.build/docs/cloud/faq).
7. **From production to a ticket people can trust.** Seer, Devin, Cursor and Factory go from an alert straight
   to a PR. Warp deliberately stops at a report. Only Ona's template and Sentry's alert → Jira rule file a
   ticket first. Nobody shows evidence of deduplicating against existing tickets or scoring how viable a ticket is.
8. **Predictable cost.** Copilot, Seer, Rovo, Linear, Cursor's Bugbot and Devin all moved to credits or usage
   billing in 2026 (links above). Teams now pay a seat price *and* metered agent usage.
9. **Preview environments.** Only Amp's portals and Conductor's Vercel deploy status come close. Nobody spins
   up a full preview of each agent branch as a built-in feature.

## Where Bantu's position helps

- **No seat price on top of model plans.** Every competitor except Pane and Amp Teams charges per seat or per
  team, plus usage. Bantu can be the only free option with a team layer. Teams still pay Anthropic or OpenAI.
- **Agent-neutral and tracker-neutral.** Bantu already drives Claude Code, Codex and ACP agents. A Bantu queue
  can take a Jira, ClickUp or GitHub ticket and give it to any of them. The hubs are each tied to one tracker
  and one vendor's cloud.
- **Everything in your own cloud, including the planner.** `bantud` runs the agent loop itself. That makes it
  the only free option that matches what Factory and Coder sell to enterprises. Prompts never pass through a
  middle layer, and code goes only to the model vendor.
- **One daemon on the laptop and in the cloud.** One protocol, one event log and one set of workspaces. This
  is the gap in point 6, and Bantu already has it.
- **Totals only, as the design.** A unique position, and it fits teams that don't want monitoring tools. Be
  honest about the catch in point 1.
- **The timing.** Jules users need somewhere to go by 2026-12-31. Conductor's November signals were mixed.
  Bantu can already import from Conductor.

**What works against Bantu:**
- It's alpha. The daemon has one user and a token, with no SSO, roles or audit log, and enterprises ask for
  SSO/SCIM first.
- An agent that works tickets on its own needs credentials that aren't tied to one person. Running Claude
  Code for a shared bot means an API key or org account, not a personal subscription. Watch the vendors'
  terms on this.
- **The hosted offer conflicts with a written rule.** [ARCHITECTURE.md](../ARCHITECTURE.md) says "no feature
  may require a server run by the project". A hosted Bantu can fit only if it runs the same open-source
  control plane and nothing in the free path depends on it. That should be decided before building.
- **Assigning work to people sits awkwardly with "totals only".** If admins assign tickets to named people,
  per-person output is visible in the tracker anyway. Bantu should keep its own data at team level and say
  plainly that it can't hide what the tracker shows.

## What to build first, in order

### 1. Two-way tracker sync: GitHub Issues, then Jira, then ClickUp
- **What:** a ticket becomes a workspace. Assigning a ticket to a "Bantu agent" user (or adding a label) queues
  it. Bantu writes back status, a plan, progress comments, the PR link and a done/blocked state.
- **Why first:**
  - It's the entry point for every team product above.
  - It helps solo users too, and needs no org layer.
  - Bantu is already partway there: workspaces from issues, loops from GitHub labels.
  - ClickUp is open ground (gap 2), and writing state back is thin everywhere (gap 3).
- **Notes:**
  - GitHub and ClickUp have webhooks *(summary)* [n.d.](https://developer.clickup.com/docs/webhooktaskpayloads).
    Jira lets outside agents be assigned work through a Forge connector
    [n.d.](https://developer.atlassian.com/platform/forge/remote-agents-in-jira/).
  - Third-party guides say dynamic Jira webhooks expire after 30 days (unconfirmed).
  - A laptop daemon can keep polling. A cloud daemon can take webhooks directly, so no project server is needed.

### 2. A team pack: shared skills, prompts and settings, distributed through git
- **What:** an org config repo that every daemon pulls. It holds skills, per-repo prompts, `check` scripts,
  loadouts and allowed agents/models. Each value is marked either "enforced" or "default".
- **Why second:**
  - Admins expect this. Claude, Copilot, Cursor, Codex, Factory and Amp all have it (comparison 2).
  - It's cheap: git is the distribution channel, so there's no server and it fits the hard rule.
  - It's how "push processes to everyone" works with no org service running.
  - It can also write the matching Claude managed settings and AGENTS.md, so agents outside Bantu follow the
    same rules.

### 3. A self-hosted org service with team-totals analytics and a shared board
- **What:** a small open-source service (`bantud --org` or a separate binary) that the org runs in its own
  cloud. It provides:
  - OIDC sign-in and roles.
  - A board: tickets from the trackers, assignable to a person or an agent queue.
  - Analytics. Each daemon adds up its numbers locally and sends only team totals: tasks done, agent hours,
    spend, review turnaround. A team below a minimum size, say 5, shows nothing.
- **Why third:**
  - This is the position no competitor holds (gap 1).
  - It's the base for the hosted version: the hosted offer is this service run by the project, with the same
    code.
  - It needs multi-user auth in the daemon, which is the largest architecture change on this list. That's why
    it waits for steps 1 and 2.

### 4. Shared cloud workers that take agent tickets, test them and post a preview link
- **What:** a `bantud` in the org's cloud (the AWS template, or the Vercel and Cloudflare burst runners) takes
  tickets from the agent queue. It runs the repo's `check` as the test gate, starts the run script behind a
  preview URL, and posts the PR, test results and preview link to the ticket.
- **Why:**
  - This turns "assign to an AI agent" into something real.
  - It reuses what Bantu has: definition of done, port forwarding, the burst runners.
  - It covers testing and live previews (gap 9).
- **What to skip:** deploys. Trigger and watch the team's existing CI/CD instead of building a deploy agent.

### 5. A production watcher that files tickets, not PRs
- **What:**
  - Read Sentry, Datadog or Vercel alerts through their APIs or MCP.
  - Group them, and check for an existing ticket before filing.
  - File a ticket with the evidence (stack trace, release, count, suspected commit) and a viability note.
  - Optionally add the "agent" label, so step 4 picks it up.
- **Why last:**
  - Fixing straight from an alert is crowded: Seer, Devin, Cursor, Factory, Vercel Agent, Datadog.
  - The open niche is the *ticket* step (gap 7), and it depends on steps 1 and 4.
  - Seer can't be self-hosted [n.d.](https://develop.sentry.dev/self-hosted/), so a BYO-cloud watcher is a real
    alternative for teams on self-hosted Sentry.

**Not now:**
- A hosted Bantu before the self-hosted org service works.
- Native integrations beyond the three trackers (Linear is the obvious next one).
- Per-person anything.
