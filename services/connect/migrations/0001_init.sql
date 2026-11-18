-- Bantu Connect schema. Times are Unix milliseconds. Secrets are stored either as a SHA-256
-- hash (invite links, join codes, daemon tokens) or AES-GCM encrypted (tracker tokens, the
-- org's Anthropic API key); never in the clear.

-- People who have signed in with GitHub. The GitHub user id is the identity; the login can
-- change and is refreshed at each sign-in.
CREATE TABLE users (
  github_id   INTEGER PRIMARY KEY,
  login       TEXT NOT NULL,
  name        TEXT,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE orgs (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  slug          TEXT NOT NULL UNIQUE,
  created_at    INTEGER NOT NULL,
  -- Claude access: own | team | api_key | bedrock | vertex.
  claude_mode   TEXT NOT NULL DEFAULT 'own'
                CHECK (claude_mode IN ('own', 'team', 'api_key', 'bedrock', 'vertex')),
  -- Non-secret settings for the mode, as JSON: plan, region, project id, model pins.
  claude_config TEXT NOT NULL DEFAULT '{}',
  -- The org's Anthropic Console API key, encrypted. Only set in api_key mode.
  claude_secret TEXT
);

CREATE TABLE members (
  org_id      TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  user_id     INTEGER NOT NULL REFERENCES users(github_id),
  role        TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
  created_at  INTEGER NOT NULL,
  PRIMARY KEY (org_id, user_id)
);
CREATE INDEX members_by_user ON members(user_id);

-- One link lets one person join. Only the hash of the link's secret is kept.
CREATE TABLE invites (
  id           TEXT PRIMARY KEY,
  org_id       TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  token_hash   TEXT NOT NULL UNIQUE,
  role         TEXT NOT NULL CHECK (role IN ('admin', 'member')),
  created_by   INTEGER NOT NULL,
  created_at   INTEGER NOT NULL,
  expires_at   INTEGER NOT NULL,
  revoked_at   INTEGER,
  accepted_by  INTEGER,
  accepted_at  INTEGER
);
CREATE INDEX invites_by_org ON invites(org_id);

-- One-time codes a member's Bantu exchanges for a daemon token.
CREATE TABLE join_codes (
  code_hash   TEXT PRIMARY KEY,
  org_id      TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  user_id     INTEGER NOT NULL,
  created_at  INTEGER NOT NULL,
  expires_at  INTEGER NOT NULL,
  used_at     INTEGER
);

-- A linked Bantu. The token is 32 random bytes; only its SHA-256 is stored.
CREATE TABLE daemons (
  id            TEXT PRIMARY KEY,
  org_id        TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  user_id       INTEGER NOT NULL,
  name          TEXT NOT NULL,
  token_hash    TEXT NOT NULL UNIQUE,
  created_at    INTEGER NOT NULL,
  last_seen_at  INTEGER,
  revoked_at    INTEGER
);
CREATE INDEX daemons_by_org ON daemons(org_id);

-- Shared tracker connections. The token is AES-GCM encrypted.
CREATE TABLE connections (
  id          TEXT PRIMARY KEY,
  org_id      TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL CHECK (kind IN ('jira', 'clickup', 'gitlab')),
  name        TEXT NOT NULL,
  site        TEXT,
  scope       TEXT NOT NULL,
  account     TEXT,
  token_enc   TEXT NOT NULL,
  created_by  INTEGER NOT NULL,
  created_at  INTEGER NOT NULL
);
CREATE INDEX connections_by_org ON connections(org_id);

-- Daily counts, one row per daemon per day, so a resend replaces the row instead of adding
-- to it. Pages only ever show sums across the org.
CREATE TABLE daily_totals (
  daemon_id           TEXT NOT NULL,
  day                 TEXT NOT NULL,
  org_id              TEXT NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  workspaces_started  INTEGER NOT NULL DEFAULT 0,
  prs_opened          INTEGER NOT NULL DEFAULT 0,
  prs_merged          INTEGER NOT NULL DEFAULT 0,
  checks_passed       INTEGER NOT NULL DEFAULT 0,
  checks_failed       INTEGER NOT NULL DEFAULT 0,
  agent_turns         INTEGER NOT NULL DEFAULT 0,
  cost_usd            REAL NOT NULL DEFAULT 0,
  updated_at          INTEGER NOT NULL,
  PRIMARY KEY (daemon_id, day)
);
CREATE INDEX daily_totals_by_org_day ON daily_totals(org_id, day);
