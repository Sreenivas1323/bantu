// Linking a member's Bantu: one-time join codes, daemon tokens, and what a daemon may read
// and send.

import { claudeAccess, type ClaudeAccess } from "./claude.ts";
import { connectionsWithTokens, type ConnectionWithToken } from "./connections.ts";
import { randomBytes, randomToken, sha256Hex } from "./crypto.ts";
import type { Db } from "./db.ts";
import { HttpError } from "./context.ts";
import { orgById, roleIn, type Org, type Role } from "./orgs.ts";

export const CODE_TTL_MS = 15 * 60 * 1000;
export const CODE_LENGTH = 10;
// Crockford's base32: no I, L, O or U, so a code read aloud or retyped survives.
const ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** 10 characters, 50 random bits. 256 is a multiple of 32, so `b & 31` has no bias. */
export function newCode(): string {
  return Array.from(randomBytes(CODE_LENGTH), (b) => ALPHABET[b & 31]).join("");
}

/** Accepts any case, spaces and hyphens, and the letters Crockford maps to digits. */
export function normalizeCode(input: string): string {
  return input
    .toUpperCase()
    .replace(/[\s-]/g, "")
    .replace(/[IL]/g, "1")
    .replace(/O/g, "0");
}

/** ABCDE-FGHJK, for reading. Links use the code without the hyphen. */
export function displayCode(code: string): string {
  return `${code.slice(0, 5)}-${code.slice(5)}`;
}

/** Makes a fresh code for this member and voids any earlier unused one. */
export async function createJoinCode(db: Db, now: number, orgId: string, userId: number): Promise<{ code: string; expiresAt: number }> {
  if (!(await roleIn(db, orgId, userId))) throw new HttpError(403, "You aren't in this org.");
  await db.run("UPDATE join_codes SET used_at = ? WHERE org_id = ? AND user_id = ? AND used_at IS NULL", now, orgId, userId);
  const code = newCode();
  const expiresAt = now + CODE_TTL_MS;
  await db.run(
    "INSERT INTO join_codes (code_hash, org_id, user_id, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
    await sha256Hex(code),
    orgId,
    userId,
    now,
    expiresAt,
  );
  return { code, expiresAt };
}

/** Daemon tokens are 32 random bytes, base64url. Only the SHA-256 is stored. */
export function newDaemonToken(): string {
  return randomToken(32);
}

export function hashToken(token: string): Promise<string> {
  return sha256Hex(token);
}

const TOKEN_SHAPE = /^[A-Za-z0-9_-]{43}$/;

export type JoinResponse = {
  daemon_token: string;
  org: { id: string; name: string; slug: string };
  member: { login: string; role: Role };
};

function orgJson(org: Org) {
  return { id: org.id, name: org.name, slug: org.slug };
}

function machineName(raw: unknown): string {
  const s = typeof raw === "string" ? raw.replace(/[\u0000-\u001f\u007f]/g, "").replace(/\s+/g, " ").trim() : "";
  return (s || "Bantu").slice(0, 100);
}

/** POST /api/daemon/join. 400 for a code that never existed, 410 for one used or expired. */
export async function exchangeCode(db: Db, now: number, rawCode: unknown, rawName: unknown): Promise<JoinResponse> {
  if (typeof rawCode !== "string") throw new HttpError(400, "code is required");
  const code = normalizeCode(rawCode);
  if (code.length !== CODE_LENGTH || ![...code].every((c) => ALPHABET.includes(c))) throw new HttpError(400, "invalid code");
  const hash = await sha256Hex(code);
  const { changes } = await db.run(
    "UPDATE join_codes SET used_at = ? WHERE code_hash = ? AND used_at IS NULL AND expires_at > ?",
    now,
    hash,
    now,
  );
  const row = await db.get<{ org_id: string; user_id: number }>("SELECT org_id, user_id FROM join_codes WHERE code_hash = ?", hash);
  if (!row) throw new HttpError(400, "invalid code");
  if (changes !== 1) throw new HttpError(410, "this code was already used or has expired");
  const [org, role, user] = await Promise.all([
    orgById(db, row.org_id),
    roleIn(db, row.org_id, row.user_id),
    db.get<{ login: string }>("SELECT login FROM users WHERE github_id = ?", row.user_id),
  ]);
  if (!org || !role || !user) throw new HttpError(410, "this code is no longer valid");

  const token = newDaemonToken();
  await db.run(
    "INSERT INTO daemons (id, org_id, user_id, name, token_hash, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    crypto.randomUUID(),
    org.id,
    row.user_id,
    machineName(rawName),
    await hashToken(token),
    now,
  );
  return { daemon_token: token, org: orgJson(org), member: { login: user.login, role } };
}

export type DaemonAuth = {
  daemon: { id: string; name: string; user_id: number };
  org: Org;
  member: { login: string; role: Role };
};

/** The daemon behind a bearer token, or null when it's unknown, revoked or its member left. */
export async function authDaemon(db: Db, authorization: string | null): Promise<DaemonAuth | null> {
  const m = /^Bearer\s+(\S+)\s*$/i.exec(authorization ?? "");
  if (!m || !TOKEN_SHAPE.test(m[1]!)) return null;
  const row = await db.get<{ id: string; name: string; user_id: number; org_id: string; role: Role; login: string }>(
    `SELECT d.id, d.name, d.user_id, d.org_id, m.role, u.login
     FROM daemons d
     JOIN members m ON m.org_id = d.org_id AND m.user_id = d.user_id
     JOIN users u ON u.github_id = d.user_id
     WHERE d.token_hash = ? AND d.revoked_at IS NULL`,
    await hashToken(m[1]!),
  );
  if (!row) return null;
  const org = await orgById(db, row.org_id);
  if (!org) return null;
  return { daemon: { id: row.id, name: row.name, user_id: row.user_id }, org, member: { login: row.login, role: row.role } };
}

export type SyncResponse = {
  org: { id: string; name: string; slug: string };
  member: { login: string; role: Role };
  connections: ConnectionWithToken[];
  claude: ClaudeAccess;
  synced_at: string;
};

/** GET /api/daemon/sync. */
export async function sync(db: Db, now: number, auth: DaemonAuth, encryptionKey: string): Promise<SyncResponse> {
  await db.run("UPDATE daemons SET last_seen_at = ? WHERE id = ?", now, auth.daemon.id);
  return {
    org: orgJson(auth.org),
    member: auth.member,
    connections: await connectionsWithTokens(db, auth.org.id, encryptionKey),
    claude: await claudeAccess(auth.org, encryptionKey),
    synced_at: new Date(now).toISOString(),
  };
}

export const COUNT_FIELDS = [
  "workspaces_started",
  "prs_opened",
  "prs_merged",
  "checks_passed",
  "checks_failed",
  "agent_turns",
  "cost_usd",
] as const;
export type Counts = Record<(typeof COUNT_FIELDS)[number], number>;

const DAY_MS = 24 * 60 * 60 * 1000;

function parseDay(raw: unknown, now: number): string {
  if (typeof raw !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(raw)) throw new HttpError(400, "day must be YYYY-MM-DD");
  const t = Date.parse(`${raw}T00:00:00Z`);
  if (Number.isNaN(t) || new Date(t).toISOString().slice(0, 10) !== raw) throw new HttpError(400, "day isn't a real date");
  // A day ahead allows for time zones; a year back allows a daemon that was offline.
  if (t > now + DAY_MS || t < now - 400 * DAY_MS) throw new HttpError(400, "day is out of range");
  return raw;
}

export function parseCounts(raw: unknown): Counts {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) throw new HttpError(400, "counts must be an object");
  const src = raw as Record<string, unknown>;
  const out = {} as Counts;
  for (const key of COUNT_FIELDS) {
    const v = src[key] ?? 0;
    const ok =
      typeof v === "number" && Number.isFinite(v) && v >= 0 && v <= 10_000_000 && (key === "cost_usd" || Number.isInteger(v));
    if (!ok) throw new HttpError(400, `counts.${key} must be a ${key === "cost_usd" ? "non-negative number" : "non-negative integer"}`);
    out[key] = key === "cost_usd" ? Math.round(v * 10_000) / 10_000 : v;
  }
  return out;
}

/** POST /api/daemon/totals. Keyed by (daemon, day), so sending a day again replaces it. */
export async function recordTotals(db: Db, now: number, auth: DaemonAuth, body: unknown): Promise<void> {
  const b = (body ?? {}) as Record<string, unknown>;
  const day = parseDay(b.day, now);
  const c = parseCounts(b.counts);
  await db.run(
    `INSERT INTO daily_totals (daemon_id, day, org_id, workspaces_started, prs_opened, prs_merged, checks_passed,
       checks_failed, agent_turns, cost_usd, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT (daemon_id, day) DO UPDATE SET
       workspaces_started = excluded.workspaces_started, prs_opened = excluded.prs_opened,
       prs_merged = excluded.prs_merged, checks_passed = excluded.checks_passed,
       checks_failed = excluded.checks_failed, agent_turns = excluded.agent_turns,
       cost_usd = excluded.cost_usd, updated_at = excluded.updated_at`,
    auth.daemon.id,
    day,
    auth.org.id,
    c.workspaces_started,
    c.prs_opened,
    c.prs_merged,
    c.checks_passed,
    c.checks_failed,
    c.agent_turns,
    c.cost_usd,
    now,
  );
}

/** POST /api/daemon/leave, and the revoke button on the org page. */
export async function revokeDaemon(db: Db, now: number, daemonId: string): Promise<void> {
  await db.run("UPDATE daemons SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL", now, daemonId);
}

export type LinkedDaemon = { id: string; name: string; user_id: number; login: string; created_at: number; last_seen_at: number | null };

export function linkedDaemons(db: Db, orgId: string): Promise<LinkedDaemon[]> {
  return db.all<LinkedDaemon>(
    `SELECT d.id, d.name, d.user_id, u.login, d.created_at, d.last_seen_at
     FROM daemons d JOIN users u ON u.github_id = d.user_id
     JOIN members m ON m.org_id = d.org_id AND m.user_id = d.user_id
     WHERE d.org_id = ? AND d.revoked_at IS NULL ORDER BY u.login, d.created_at`,
    orgId,
  );
}
