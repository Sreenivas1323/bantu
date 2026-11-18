// Server-rendered pages. Every value goes through `html`, which escapes it. No page is given
// a stored token: the types passed in here (ConnectionSummary, ClaudeView) have no field for one.

import { CLAUDE_MODES, MODE_LABELS, type ClaudeConfig, type ClaudeMode } from "./claude.ts";
import { KIND_LABELS, type ConnectionSummary } from "./connections.ts";
import { displayCode, type LinkedDaemon } from "./daemons.ts";
import { html, trusted, type Html } from "./html.ts";
import type { Invite, InviteState, Member, Org, Role, User } from "./orgs.ts";
import { isAdmin } from "./orgs.ts";
import { MIN_MEMBERS, type TeamTotals } from "./totals.ts";

export const CONSUMER_TERMS_QUOTE =
  "You may not share your Account login information, Anthropic API key, or Account credentials with anyone else. You also may not make your Account available to anyone else.";

type Chrome = { user?: User | null; csrf?: string };

export function layout(title: string, body: Html, chrome: Chrome = {}): string {
  const nav = chrome.user
    ? html`<nav aria-label="Account">
        <a href="/orgs">Your orgs</a>
        <span class="muted">${chrome.user.login}</span>
        <form class="inline" method="post" action="/logout">
          <input type="hidden" name="csrf" value="${chrome.csrf ?? ""}" />
          <button class="link" type="submit">Sign out</button>
        </form>
      </nav>`
    : html`<nav aria-label="Account"><a href="/">Sign in</a></nav>`;
  return html`${trusted("<!doctype html>")}
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<meta name="referrer" content="no-referrer" />
<title>${title} · Bantu Connect</title>
<link rel="stylesheet" href="/styles.css" />
</head>
<body>
<a class="skip" href="#main">Skip to content</a>
<header class="top">
  <a class="wordmark" href="${chrome.user ? "/orgs" : "/"}"><span class="dots" aria-hidden="true"><i class="ok"></i><i class="run"></i><i class="wait"></i></span>Bantu Connect</a>
  ${nav}
</header>
<main id="main">
${body}
</main>
<footer>Bantu Connect is open source. It holds no code: agents and compute stay on members' machines and in your own cloud.</footer>
</body>
</html>`.value;
}

// ---- Formatting --------------------------------------------------------------------------

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

export function when(ms: number): Html {
  const d = new Date(ms);
  const label = `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}, ${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")} UTC`;
  return html`<time datetime="${d.toISOString()}">${label}</time>`;
}

export function ago(ms: number | null, now: number): Html {
  if (ms === null) return html`<span class="muted">Never</span>`;
  const s = Math.max(0, Math.round((now - ms) / 1000));
  const label =
    s < 60 ? "just now" : s < 3600 ? `${Math.floor(s / 60)} min ago` : s < 86400 ? `${Math.floor(s / 3600)} h ago` : `${Math.floor(s / 86400)} d ago`;
  return html`<time datetime="${new Date(ms).toISOString()}" title="${new Date(ms).toISOString()}">${label}</time>`;
}

const int = (n: number) => Math.round(n).toLocaleString("en-US");
const usd = (n: number) => `$${n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;

function csrfField(csrf: string): Html {
  return html`<input type="hidden" name="csrf" value="${csrf}" />`;
}

function notice(kind: "ok" | "err" | "info", body: Html | string): Html {
  const cls = kind === "ok" ? "notice ok-n" : kind === "err" ? "notice err" : "notice";
  return html`<div class="${cls}" role="${kind === "err" ? "alert" : "status"}">${body}</div>`;
}

// ---- Landing and errors ----------------------------------------------------------------

export function landingPage(next = "/orgs"): string {
  const href = `/auth/github?next=${encodeURIComponent(next)}`;
  return layout(
    "Sign in",
    html`<section>
      <h1>Your team's Bantu, in step.</h1>
      <p class="lede">Bantu Connect signs your team in with GitHub, shares tracker connections and how
      everyone reaches Claude, and shows team totals. It holds no code. Agents and compute stay on
      members' machines and in your own cloud.</p>
      <p><a class="button primary" href="${href}">Sign in with GitHub</a></p>
      <p class="muted small">GitHub is asked for your public profile only: your id and login.</p>
    </section>`,
  );
}

export function errorPage(status: number, message: string, chrome: Chrome = {}): string {
  const title = status === 404 ? "Not found" : status === 403 ? "Not allowed" : status === 410 ? "No longer valid" : "Something went wrong";
  return layout(
    title,
    html`<section><h1>${title}</h1><p class="lede">${message}</p><p><a href="${chrome.user ? "/orgs" : "/"}">Back</a></p></section>`,
    chrome,
  );
}

// ---- Orgs --------------------------------------------------------------------------------

export function orgsPage(user: User, csrf: string, orgs: Array<Org & { role: Role }>): string {
  const list = orgs.length
    ? html`<ul class="plain">${orgs.map(
        (o) => html`<li><a href="/o/${o.slug}"><strong>${o.name}</strong></a><span class="muted small">${o.role}</span></li>`,
      )}</ul>`
    : html`<p class="muted">You aren't in an org yet. Create one, or open an invite link from an admin.</p>`;
  return layout(
    "Your orgs",
    html`<section>
      <h1>Your orgs</h1>
      ${list}
      <p class="mt"><a class="button primary" href="/orgs/new">New org</a></p>
    </section>`,
    { user, csrf },
  );
}

export function newOrgPage(user: User, csrf: string, error?: string, values: { name?: string; slug?: string } = {}): string {
  return layout(
    "New org",
    html`<section>
      <h1>New org</h1>
      <p class="lede">You'll be its owner. You can invite people once it exists.</p>
      ${error ? notice("err", error) : ""}
      <form class="stack" method="post" action="/orgs">
        ${csrfField(csrf)}
        <label>Name <input type="text" name="name" required maxlength="80" value="${values.name ?? ""}" autocomplete="organization" /></label>
        <label>Slug <span class="hint">Used in the org's address: lowercase letters, digits and hyphens.</span>
          <input type="text" name="slug" required maxlength="40" pattern="[a-z0-9](?:[a-z0-9\\-]{0,38}[a-z0-9])?" value="${values.slug ?? ""}" autocapitalize="none" spellcheck="false" /></label>
        <div><button class="primary" type="submit">Create org</button></div>
      </form>
    </section>`,
    { user, csrf },
  );
}

// ---- Org dashboard ---------------------------------------------------------------------

export type ClaudeView = { mode: ClaudeMode; config: ClaudeConfig; hasApiKey: boolean };

export type DashboardData = {
  user: User;
  csrf: string;
  now: number;
  origin: string;
  org: Org;
  role: Role;
  members: Member[];
  invites: Invite[];
  daemons: LinkedDaemon[];
  connections: ConnectionSummary[];
  claude: ClaudeView;
  totals: TeamTotals;
  flash?: string;
  error?: string;
  newInvite?: { link: string; role: Role; expiresAt: number };
};

function totalsSection(t: TeamTotals): Html {
  const why = html`<p class="muted small">Bantu Connect only ever shows sums for the whole team, never numbers for one
    person, and nothing at all until at least ${MIN_MEMBERS} members have linked a Bantu.</p>`;
  if (!t.visible) {
    return html`<section aria-labelledby="totals-h">
      <h2 id="totals-h">Team totals</h2>
      ${notice(
        "info",
        html`<p><strong>Hidden until ${MIN_MEMBERS} members have linked a Bantu.</strong> ${t.linkedMembers} of ${MIN_MEMBERS} so far.
        With fewer, a total could stand in for one person.</p>`,
      )}
      ${why}
    </section>`;
  }
  const cards = t.windows.map((w) => {
    if (!w.counts) {
      return html`<div class="stat"><h3>Last ${w.days} days</h3>
        <p class="muted small">Hidden: fewer than ${MIN_MEMBERS} members sent counts in this period.</p></div>`;
    }
    const c = w.counts;
    const checks = c.checks_passed + c.checks_failed;
    return html`<div class="stat"><h3>Last ${w.days} days</h3><dl>
      <dt>Workspaces started</dt><dd>${int(c.workspaces_started)}</dd>
      <dt>PRs opened</dt><dd>${int(c.prs_opened)}</dd>
      <dt>PRs merged</dt><dd>${int(c.prs_merged)}</dd>
      <dt>Checks passed</dt><dd>${int(c.checks_passed)}</dd>
      <dt>Checks failed</dt><dd>${int(c.checks_failed)}</dd>
      <dt>Check pass rate</dt><dd>${checks ? `${Math.round((c.checks_passed / checks) * 100)}%` : "–"}</dd>
      <dt>Agent turns</dt><dd>${int(c.agent_turns)}</dd>
      <dt>Cost</dt><dd>${usd(c.cost_usd)}</dd>
    </dl></div>`;
  });
  return html`<section aria-labelledby="totals-h">
    <h2 id="totals-h">Team totals</h2>
    <div class="grid">${cards}</div>
    ${why}
  </section>`;
}

function membersSection(d: DashboardData): Html {
  const admin = isAdmin(d.role);
  const rows = d.members.map((m) => {
    const self = m.user_id === d.user.github_id;
    const canEdit = admin && !self && (m.role !== "owner" || d.role === "owner");
    const roleOptions = (d.role === "owner" ? (["member", "admin", "owner"] as Role[]) : (["member", "admin"] as Role[])).map(
      (r) => html`<option value="${r}" ${r === m.role ? trusted("selected") : ""}>${r}</option>`,
    );
    const actions = self
      ? html`<form class="inline" method="post" action="/o/${d.org.slug}/leave">${csrfField(d.csrf)}<button class="link danger" type="submit">Leave org</button></form>`
      : canEdit
        ? html`<div class="row-actions">
            <form class="inline" method="post" action="/o/${d.org.slug}/members/${m.user_id}/role">
              ${csrfField(d.csrf)}
              <label class="inline"><span class="sr">Role for ${m.login}</span><select name="role">${roleOptions}</select></label>
              <button class="link" type="submit">Change role</button>
            </form>
            <form class="inline" method="post" action="/o/${d.org.slug}/members/${m.user_id}/remove">
              ${csrfField(d.csrf)}<button class="link danger" type="submit">Remove</button>
            </form>
          </div>`
        : "";
    return html`<tr><td><a href="https://github.com/${m.login}" rel="noreferrer">${m.login}</a>${self ? html` <span class="muted small">(you)</span>` : ""}</td>
      <td>${m.role}</td><td>${actions}</td></tr>`;
  });
  return html`<section aria-labelledby="members-h">
    <h2 id="members-h">Members</h2>
    <table><thead><tr><th scope="col">GitHub login</th><th scope="col">Role</th><th scope="col"><span class="sr">Actions</span></th></tr></thead>
    <tbody>${rows}</tbody></table>
  </section>`;
}

function invitesSection(d: DashboardData): Html {
  if (!isAdmin(d.role)) return html``;
  const rows = d.invites.map(
    (i) => html`<tr><td>${i.role}</td><td>${when(i.created_at)}</td><td>${when(i.expires_at)}</td>
      <td><form class="inline" method="post" action="/o/${d.org.slug}/invites/${i.id}/revoke">${csrfField(d.csrf)}<button class="link danger" type="submit">Revoke</button></form></td></tr>`,
  );
  return html`<section aria-labelledby="invites-h">
    <h2 id="invites-h">Invites</h2>
    ${
      d.newInvite
        ? notice(
            "ok",
            html`<p><strong>New ${d.newInvite.role} invite.</strong> Send this link to one person. It works once, until ${when(d.newInvite.expiresAt)}, and
            isn't shown again.</p><code class="secret">${d.newInvite.link}</code>`,
          )
        : ""
    }
    <form class="row-actions" method="post" action="/o/${d.org.slug}/invites">
      ${csrfField(d.csrf)}
      <label class="radio">Invite as <select name="role"><option value="member">member</option><option value="admin">admin</option></select></label>
      <button class="primary" type="submit">Create invite link</button>
    </form>
    <p class="muted small">Each link lets one person join and expires after 7 days.</p>
    ${
      d.invites.length
        ? html`<table><thead><tr><th scope="col">Role</th><th scope="col">Created</th><th scope="col">Expires</th><th scope="col"><span class="sr">Actions</span></th></tr></thead><tbody>${rows}</tbody></table>`
        : html`<p class="muted">No open invites.</p>`
    }
  </section>`;
}

function daemonsSection(d: DashboardData): Html {
  const admin = isAdmin(d.role);
  const shown = admin ? d.daemons : d.daemons.filter((x) => x.user_id === d.user.github_id);
  const rows = shown.map(
    (x) => html`<tr><td>${x.name}</td>${admin ? html`<td>${x.login}</td>` : ""}<td>${when(x.created_at)}</td><td>${ago(x.last_seen_at, d.now)}</td>
      <td><form class="inline" method="post" action="/o/${d.org.slug}/daemons/${x.id}/revoke">${csrfField(d.csrf)}<button class="link danger" type="submit">Revoke</button></form></td></tr>`,
  );
  return html`<section aria-labelledby="bantu-h">
    <h2 id="bantu-h">Linked Bantu apps</h2>
    <p class="lede">Connect your Bantu to get the org's tracker connections and Claude settings. You'll get a link
    that works once, for 15 minutes.</p>
    <form method="post" action="/o/${d.org.slug}/code">${csrfField(d.csrf)}<button class="primary" type="submit">Connect a Bantu</button></form>
    ${
      shown.length
        ? html`<table class="mt"><thead><tr><th scope="col">Name</th>${admin ? html`<th scope="col">Member</th>` : ""}<th scope="col">Linked</th><th scope="col">Last sync</th><th scope="col"><span class="sr">Actions</span></th></tr></thead><tbody>${rows}</tbody></table>`
        : html`<p class="muted">${admin ? "No Bantu is linked yet." : "You haven't linked a Bantu yet."}</p>`
    }
  </section>`;
}

function connectionsSection(d: DashboardData): Html {
  const admin = isAdmin(d.role);
  const rows = d.connections.map(
    (c) => html`<tr><td>${KIND_LABELS[c.kind]}</td><td>${c.name}</td><td class="mono small">${c.site ?? "–"}</td><td class="mono small">${c.scope}</td>
      <td>${c.account ?? "–"}</td><td><span class="pill">Token stored</span></td>
      <td>${admin ? html`<form class="inline" method="post" action="/o/${d.org.slug}/connections/${c.id}/delete">${csrfField(d.csrf)}<button class="link danger" type="submit">Delete</button></form>` : ""}</td></tr>`,
  );
  const tokenField = html`<label>API token <span class="hint">Encrypted when saved. It's never shown here again.</span>
    <input type="password" name="token" required autocomplete="off" spellcheck="false" /></label>`;
  const form = (kind: string, title: string, fields: Html) => html`<details><summary>Add ${title}</summary>
    <form class="stack" method="post" action="/o/${d.org.slug}/connections">
      ${csrfField(d.csrf)}<input type="hidden" name="kind" value="${kind}" />
      <label>Name <span class="hint">Optional, such as “Payments board”.</span><input type="text" name="name" maxlength="80" /></label>
      ${fields}${tokenField}
      <div><button class="primary" type="submit">Add ${title}</button></div>
    </form></details>`;
  return html`<section aria-labelledby="conn-h">
    <h2 id="conn-h">Tracker connections</h2>
    <p class="lede">Shared with every linked Bantu in the org. GitHub needs no connection: each member's Bantu uses their own
    <code>gh</code> login.</p>
    ${
      d.connections.length
        ? html`<table><thead><tr><th scope="col">Tracker</th><th scope="col">Name</th><th scope="col">Site</th><th scope="col">Scope</th><th scope="col">Account</th><th scope="col">Token</th><th scope="col"><span class="sr">Actions</span></th></tr></thead><tbody>${rows}</tbody></table>`
        : html`<p class="muted">No connections yet.</p>`
    }
    ${
      admin
        ? html`${form(
            "jira",
            "Jira",
            html`<label>Site <input type="url" name="site" required placeholder="https://acme.atlassian.net" /></label>
            <label>Account email <input type="email" name="account" required autocomplete="off" /></label>
            <label>Project key or JQL <input type="text" name="scope" required placeholder="PAY or project = PAY AND status != Done" spellcheck="false" /></label>`,
          )}
          ${form("clickup", "ClickUp", html`<label>List id <input type="text" name="scope" required inputmode="numeric" spellcheck="false" /></label>`)}
          ${form(
            "gitlab",
            "GitLab",
            html`<label>Site <input type="url" name="site" required value="https://gitlab.com" /></label>
            <label>Project path <input type="text" name="scope" required placeholder="group/project" spellcheck="false" /></label>`,
          )}`
        : ""
    }
  </section>`;
}

function claudeSection(d: DashboardData): Html {
  const c = d.claude;
  const current =
    c.mode === "team"
      ? html`${MODE_LABELS.team}: <strong>${c.config.plan ?? ""}</strong>. Members sign in to Claude with their work account.`
      : c.mode === "api_key"
        ? html`${MODE_LABELS.api_key}. ${c.hasApiKey ? "A key is stored." : "No key is stored yet."}`
        : c.mode === "bedrock"
          ? html`${MODE_LABELS.bedrock} in <code>${c.config.region ?? ""}</code>, with each member's own AWS login.`
          : c.mode === "vertex"
            ? html`${MODE_LABELS.vertex}: project <code>${c.config.project_id ?? ""}</code> in <code>${c.config.region ?? ""}</code>, with each member's own Google Cloud login.`
            : html`${MODE_LABELS.own}.`;
  const why = notice(
    "info",
    html`<p><strong>Why there's no shared-subscription option.</strong> A Claude Pro or Max plan is for one person. Anthropic's
    <a href="https://www.anthropic.com/legal/consumer-terms" rel="noreferrer">Consumer Terms</a> (Section 2) say: “${CONSUMER_TERMS_QUOTE}”
    So Bantu Connect never stores or passes around anyone's Claude login. To give the team access, use Team or Enterprise seats,
    an API key the org owns, or Bedrock or Vertex AI.</p>`,
  );
  if (!isAdmin(d.role)) {
    return html`<section aria-labelledby="claude-h"><h2 id="claude-h">Claude access</h2><p>${current}</p>${why}</section>`;
  }
  const radio = (m: ClaudeMode) =>
    html`<label class="radio"><input type="radio" name="mode" value="${m}" ${c.mode === m ? trusted("checked") : ""} required /> ${MODE_LABELS[m]}</label>`;
  const val = (k: keyof ClaudeConfig, m: ClaudeMode) => (c.mode === m ? (c.config[k] ?? "") : "");
  const pins = (m: ClaudeMode, prefix: string) => html`<details><summary>Model pins (optional)</summary><div class="stack">
    <p class="muted small">Sets ANTHROPIC_DEFAULT_OPUS_MODEL, ANTHROPIC_DEFAULT_SONNET_MODEL and ANTHROPIC_DEFAULT_HAIKU_MODEL.</p>
    <label>Opus model id <input type="text" name="${m}_opus_model" value="${val("opus_model", m)}" placeholder="${prefix}claude-opus-4-8" spellcheck="false" /></label>
    <label>Sonnet model id <input type="text" name="${m}_sonnet_model" value="${val("sonnet_model", m)}" placeholder="${prefix}claude-sonnet-4-6" spellcheck="false" /></label>
    <label>Haiku model id <input type="text" name="${m}_haiku_model" value="${val("haiku_model", m)}" spellcheck="false" /></label>
  </div></details>`;
  return html`<section aria-labelledby="claude-h">
    <h2 id="claude-h">Claude access</h2>
    <p>Now: ${current}</p>
    ${why}
    <form class="stack" method="post" action="/o/${d.org.slug}/claude">
      ${csrfField(d.csrf)}
      <fieldset><legend>How members reach Claude</legend>${CLAUDE_MODES.map(radio)}</fieldset>
      <fieldset><legend>Team or Enterprise</legend>
        <label>Plan name <input type="text" name="plan" maxlength="60" value="${val("plan", "team")}" placeholder="Claude Team" /></label>
      </fieldset>
      <fieldset><legend>API key</legend>
        <label>Anthropic Console API key <span class="hint">${c.hasApiKey ? "A key is stored. Leave this blank to keep it." : "Encrypted when saved, and never shown again."} Every linked Bantu in the org receives it as ANTHROPIC_API_KEY.</span>
          <input type="password" name="api_key" autocomplete="off" spellcheck="false" placeholder="${c.hasApiKey ? "Stored" : "sk-ant-…"}" /></label>
      </fieldset>
      <fieldset><legend>Amazon Bedrock</legend>
        <p class="muted small">Members use their own AWS login. No AWS credentials are stored here.</p>
        <label>AWS region <input type="text" name="aws_region" value="${val("region", "bedrock")}" placeholder="us-east-1" spellcheck="false" /></label>
        ${pins("bedrock", "us.anthropic.")}
      </fieldset>
      <fieldset><legend>Google Cloud Vertex AI</legend>
        <p class="muted small">Members use their own Google Cloud login.</p>
        <label>Region <input type="text" name="vertex_region" value="${val("region", "vertex")}" placeholder="global" spellcheck="false" /></label>
        <label>Project id <input type="text" name="vertex_project" value="${val("project_id", "vertex")}" spellcheck="false" /></label>
        ${pins("vertex", "")}
      </fieldset>
      <div><button class="primary" type="submit">Save Claude access</button></div>
    </form>
  </section>`;
}

export function dashboardPage(d: DashboardData): string {
  return layout(
    d.org.name,
    html`<header>
      <h1>${d.org.name}</h1>
      <p class="muted">You're ${d.role === "member" ? "a" : "an"} ${d.role}. <span class="mono small">/o/${d.org.slug}</span></p>
      ${d.flash ? notice("ok", d.flash) : ""}
      ${d.error ? notice("err", d.error) : ""}
    </header>
    ${daemonsSection(d)}
    ${totalsSection(d.totals)}
    ${membersSection(d)}
    ${invitesSection(d)}
    ${connectionsSection(d)}
    ${claudeSection(d)}`,
    { user: d.user, csrf: d.csrf },
  );
}

// ---- Join and the one-time code ----------------------------------------------------------

export function joinPage(opts: {
  state: InviteState | "already_member";
  org?: Org;
  role?: Role;
  user: User | null;
  csrf?: string;
  path: string;
}): string {
  const { state, org, user } = opts;
  const chrome = { user, csrf: opts.csrf };
  if (state !== "valid" || !org) {
    const msg =
      state === "already_member"
        ? html`You're already in <a href="/o/${org!.slug}">${org!.name}</a>.`
        : state === "expired"
          ? "This invite has expired. Ask an admin for a new one."
          : state === "revoked"
            ? "This invite was revoked. Ask an admin for a new one."
            : state === "used"
              ? "This invite has already been used. Each link lets one person join."
              : "This invite link isn't valid. Check that you copied all of it.";
    return layout("Join", html`<section><h1>${state === "already_member" ? "Already a member" : "Invite not valid"}</h1><p class="lede">${msg}</p></section>`, chrome);
  }
  if (!user) {
    return layout(
      "Join",
      html`<section>
        <h1>Join ${org.name}</h1>
        <p class="lede">You've been invited to join <strong>${org.name}</strong> as ${opts.role === "admin" ? "an admin" : "a member"}. Sign in with GitHub to accept.</p>
        <p><a class="button primary" href="/auth/github?next=${encodeURIComponent(opts.path)}">Sign in with GitHub</a></p>
      </section>`,
    );
  }
  return layout(
    "Join",
    html`<section>
      <h1>Join ${org.name}</h1>
      <p class="lede">Join <strong>${org.name}</strong> as ${opts.role === "admin" ? "an admin" : "a member"}, signed in as ${user.login}.
      Next you'll get a link to connect your Bantu.</p>
      <form method="post" action="${opts.path}">${csrfField(opts.csrf ?? "")}<button class="primary" type="submit">Join ${org.name}</button></form>
    </section>`,
    chrome,
  );
}

export function codePage(opts: { user: User; csrf: string; org: Org; code: string; link: string; expiresAt: number; joined?: boolean }): string {
  return layout(
    "Connect your Bantu",
    html`<section>
      ${opts.joined ? notice("ok", html`You've joined <strong>${opts.org.name}</strong>.`) : ""}
      <h1>Connect your Bantu</h1>
      <p class="lede">Paste this link into Bantu on the machine you want to connect to <strong>${opts.org.name}</strong>. It works
      once, until ${when(opts.expiresAt)}.</p>
      <code class="secret">${opts.link}</code>
      <p class="muted">Or type the code: <span class="code" aria-label="Code ${opts.code.split("").join(" ")}">${displayCode(opts.code)}</span></p>
      <p class="muted small">Your Bantu exchanges it for its own token, which you can revoke from the org page. Letters aren't
      case-sensitive. Don't share this link: whoever uses it first links their Bantu as you.</p>
      <p><a class="button" href="/o/${opts.org.slug}">Back to ${opts.org.name}</a></p>
    </section>`,
    { user: opts.user, csrf: opts.csrf },
  );
}

/** What someone sees if they open a join link in a browser. It doesn't use the code. */
export function codeLinkPage(): string {
  return layout(
    "Bantu join link",
    html`<section>
      <h1>This link is for Bantu</h1>
      <p class="lede">Copy this page's address and paste it into Bantu on the machine you want to connect. Opening it in a
      browser doesn't use it up.</p>
      <p class="muted small">Links work once and expire 15 minutes after they're made. If yours has expired, get a new one from
      your org's page.</p>
    </section>`,
  );
}
