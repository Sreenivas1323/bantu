# Bantu Connect

Bantu Connect is the org service for Bantu: a Cloudflare Worker with a D1 database that an org
deploys to its own Cloudflare account. The project will also host the same code as a free
connector. It's the only server in Bantu, and step 3 of the [teams plan](TEAMS.md#3-the-org-service).
The code is in [`services/connect`](../services/connect).

> Status: **not deployed yet.** The Worker typechecks against `@cloudflare/workers-types`, and
> `bun test` runs every route against the real schema on `bun:sqlite`, with a stubbed GitHub.
> `wrangler deploy --dry-run` bundles it, and the daemon API has been driven by hand against
> `wrangler dev` with a local D1 database. Sign-in hasn't been tried against a real GitHub
> OAuth app, and nothing has run on Cloudflare itself.

## What it is, and what it isn't

It does five things:

- signs people in with GitHub;
- keeps orgs, roles and invites;
- links each member's Bantu to the org;
- shares the org's tracker connections and Claude settings with every linked Bantu;
- shows team totals.

What it never does:

- **No code.** Repositories, diffs, prompts and chats never reach it. Agents and compute stay
  on members' machines and in the org's own cloud. GitHub needs no connection, because each
  member's Bantu uses their own `gh` login.
- **Totals only.** Each Bantu sends daily counts. They're stored per Bantu per day, so a
  resend replaces rather than doubles, and the rows carry no person. Pages show sums for the
  whole org over 7 and 30 days, and nothing at all until 3 members have linked a Bantu. A
  period that fewer than 3 members sent counts for is hidden too.
- **No shared personal Claude logins.** There's no option to pool or share a Claude Pro or
  Max login. Anthropic's [Consumer Terms](https://www.anthropic.com/legal/consumer-terms),
  Section 2: "You may not share your Account login information, Anthropic API key, or Account
  credentials with anyone else. You also may not make your Account available to anyone else."
  The org page explains this next to the Claude settings.

## Claude access

Admins choose how members reach Claude. Each linked Bantu receives the matching environment
for the `claude` CLI at every sync.

| Mode | What members do | Environment sent to each Bantu |
| --- | --- | --- |
| `own` (default) | Use their own Claude login | none |
| `team` | Sign in with their own seat on the org's Claude Team or Enterprise plan. Bantu tells them to use their work account. | none; the plan's name is sent as `plan` |
| `api_key` | Use the org's Anthropic Console API key | `ANTHROPIC_API_KEY` |
| `bedrock` | Use their own AWS login | `CLAUDE_CODE_USE_BEDROCK=1`, `AWS_REGION`, and optionally `ANTHROPIC_DEFAULT_OPUS_MODEL`, `ANTHROPIC_DEFAULT_SONNET_MODEL`, `ANTHROPIC_DEFAULT_HAIKU_MODEL` |
| `vertex` | Use their own Google Cloud login | `CLAUDE_CODE_USE_VERTEX=1`, `CLOUD_ML_REGION`, `ANTHROPIC_VERTEX_PROJECT_ID`, and the same optional model pins |

Variable names were checked in November 2026 against Claude Code's
[environment variables](https://code.claude.com/docs/en/env-vars),
[Amazon Bedrock](https://code.claude.com/docs/en/amazon-bedrock) and
[Google Vertex AI](https://code.claude.com/docs/en/google-vertex-ai) pages.
`ANTHROPIC_SMALL_FAST_MODEL` is marked deprecated there, so it isn't offered. For Bedrock and
Vertex, no cloud credentials are stored: Claude Code uses the AWS SDK's default credential
chain or Google's Application Default Credentials on each member's machine.

In `api_key` mode every linked Bantu in the org gets the key. A Console key falls under
Anthropic's [Commercial Terms](https://www.anthropic.com/legal/commercial-terms), where the org
is "responsible for all activity under its account", so only use this mode for people the org
is entitled to give access to.

## Deploy it to your own Cloudflare account

You need a Cloudflare account; the Workers Free plan is enough. From the repository:

```sh
cd services/connect
bun install
bunx wrangler@^4.148.0 login

# 1. The database. Copy the database_id it prints into wrangler.jsonc.
bunx wrangler@^4.148.0 d1 create bantu-connect

# 2. The schema.
bun run migrate          # wrangler d1 migrations apply bantu-connect --remote
```

**3. A GitHub OAuth app.** In GitHub, open *Settings → Developer settings → OAuth Apps → New
OAuth App* (or the same under an organization's settings):

- **Homepage URL:** your Worker's address, such as `https://bantu-connect.<you>.workers.dev`
  or a custom domain.
- **Authorization callback URL:** that address plus `/auth/github/callback`, for example
  `https://bantu-connect.<you>.workers.dev/auth/github/callback`.

Bantu Connect asks for no scopes, so the app can only read public profile information, which
it uses for the GitHub user id and login. Generate a client secret.

**4. Secrets.** `wrangler.jsonc` lists all four under `secrets.required`, so deploy refuses to
go ahead until they're set.

```sh
bunx wrangler@^4.148.0 secret put GITHUB_CLIENT_ID        # from the OAuth app
bunx wrangler@^4.148.0 secret put GITHUB_CLIENT_SECRET    # from the OAuth app
openssl rand -base64 48 | bunx wrangler@^4.148.0 secret put SESSION_SECRET
openssl rand -base64 32 | bunx wrangler@^4.148.0 secret put ENCRYPTION_KEY
```

- `SESSION_SECRET` signs the session cookie, the OAuth `state` cookie and CSRF tokens.
  Changing it signs everyone out.
- `ENCRYPTION_KEY` is 32 bytes, base64. It encrypts tracker tokens and the org's API key with
  AES-256-GCM, a random IV per value, and the row's id as additional data. **If you lose it,
  every stored token has to be entered again**, so keep a copy somewhere safe.

**5. Deploy.**

```sh
bun run deploy           # wrangler deploy
```

If the Worker is reached at an address other than the one requests arrive on (behind another
proxy, say), set `PUBLIC_URL` in `wrangler.jsonc`. It's used for the callback URL and for
invite and join links.

To run it locally: put the four secrets in `services/connect/.dev.vars`, run
`bun run migrate:local`, then `bun run dev`. GitHub sign-in locally needs a second OAuth app
whose callback is `http://localhost:8787/auth/github/callback`.

## Using it

1. Sign in with GitHub and create an org. You're its owner.
2. Invite people from the org page. Each invite link lets one person join, expires after 7
   days, and can be revoked. The link is shown once.
3. They open `/join/<invite>`, sign in, and accept. Right after, they get a one-time join link
   for their Bantu.
4. **Connect a Bantu** on the org page makes a new one at any time:
   `https://<connector>/c/<CODE>`. The code is 10 characters of Crockford base32, not
   case-sensitive, works once, and expires after 15 minutes. Opening it in a browser doesn't
   use it.
5. Bantu exchanges the code for its own daemon token. The org page lists each linked Bantu
   with its name and last sync time, and has a revoke button for each.

Roles:

- **Owners** can do everything, including adding and removing owners.
- **Admins** invite and revoke invites, remove members and admins, move people between member
  and admin, and manage tracker connections and Claude access. They can't change or remove an
  owner.
- **Members** see the org, link and revoke their own Bantu, and leave.
- The last owner can't leave, step down or be removed.

Removing a member, or a member leaving, revokes every Bantu they linked.

Tracker connections, added by admins:

- **Jira:** site, account email, API token, and a project key or JQL.
- **ClickUp:** API token and list id.
- **GitLab:** site, access token and project path.

The page never shows a stored token again; to change one, delete the connection and add it
again.

## The daemon API

All JSON. Errors are `{"error": "<message>"}`. Responses are `Cache-Control: no-store`.

### `POST /api/daemon/join`

```json
{"code": "ABCDE-FGHJK", "name": "ada-mbp"}
```

`code` can be in any case, with or without the hyphen, or as it appears at the end of the
`/c/<CODE>` link. `name` is the machine's name, at most 100 characters.

- `200 {"daemon_token", "org": {"id","name","slug"}, "member": {"login","role"}}`
- `400` for a code that never existed or is malformed, or a body that isn't JSON
- `410` for a code that was already used or has expired, or whose member has since left

`daemon_token` is 32 random bytes, base64url without padding (43 characters). Only its
SHA-256 is stored. Send it as `Authorization: Bearer <daemon_token>` on the calls below. Every
one of them answers `401` once the token is revoked or the member was removed.

### `GET /api/daemon/sync`

```json
{
  "org": {"id": "…", "name": "Acme", "slug": "acme"},
  "member": {"login": "ada", "role": "owner"},
  "connections": [
    {"id": "…", "kind": "jira", "name": "Payments", "site": "https://acme.atlassian.net",
     "scope": "PAY", "account": "bot@acme.com", "token": "…"},
    {"id": "…", "kind": "clickup", "name": "ClickUp 901234", "site": null,
     "scope": "901234", "account": null, "token": "…"},
    {"id": "…", "kind": "gitlab", "name": "API", "site": "https://gitlab.com",
     "scope": "platform/api", "account": null, "token": "…"}
  ],
  "claude": {"mode": "bedrock", "plan": null,
             "env": {"CLAUDE_CODE_USE_BEDROCK": "1", "AWS_REGION": "us-east-1"}},
  "synced_at": "2026-11-18T12:00:00.000Z"
}
```

`scope` is the Jira project key or JQL, the ClickUp list id, or the GitLab project path.
`account` is the Jira account email. `plan` is set only in `team` mode. Each sync updates the
Bantu's last-seen time.

### `POST /api/daemon/totals`

```json
{"day": "2026-11-18", "counts": {"workspaces_started": 4, "prs_opened": 2, "prs_merged": 1,
  "checks_passed": 9, "checks_failed": 1, "agent_turns": 120, "cost_usd": 3.42}}
```

`204`. The row is keyed by (Bantu, day), so sending a day again replaces it. Counts are
non-negative integers and `cost_usd` a non-negative number. A missing count is 0. `day` is a
UTC date from at most a year ago to a day ahead. Anything else is a `400`.

### `POST /api/daemon/leave`

`204`, and the token is revoked.

## Security notes

- The session is a cookie signed with HMAC-SHA256, named `__Host-bantu_session`: `HttpOnly`,
  `Secure`, `SameSite=Lax`, 30 days. Sign-in uses a random `state` in a signed cookie that
  lasts 10 minutes and is compared in constant time. The GitHub access token is used once
  and never stored.
- Every form POST carries a CSRF token derived from the session, and a POST whose `Origin` is
  another site is refused.
- Pages send a CSP of `default-src 'none'` (styles from the Worker only, no scripts), `no-store`
  and `Referrer-Policy: no-referrer`, because invite and join links carry secrets in the path.
- Invite links, join codes and daemon tokens are stored only as SHA-256 hashes. Tracker tokens
  and the API key are encrypted, and pages are built from types that have no token field.
- Someone outside an org gets a 404 for its page.

## Left to do

- Deploy it: run sign-in against a real GitHub OAuth app, and the whole flow on Cloudflare.
- Rate-limit `POST /api/daemon/join` (Cloudflare's rate-limiting binding). A code has 50
  random bits and lives 15 minutes, so guessing is already impractical.
- Sessions are stateless, so signing out clears the cookie but can't end a copied one before
  it expires. A sessions table would allow "sign out everywhere".
- The Bantu side: pasting the `/c/<CODE>` link into Bantu, syncing, and sending totals.
