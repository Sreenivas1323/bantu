// Users, orgs, members, roles and invites.

import { randomToken, sha256Hex } from "./crypto.ts";
import type { Db } from "./db.ts";
import { HttpError } from "./context.ts";

export type Role = "owner" | "admin" | "member";
export const ROLES: readonly Role[] = ["owner", "admin", "member"];

export type User = { github_id: number; login: string; name: string | null };

export type Org = {
  id: string;
  name: string;
  slug: string;
  created_at: number;
  claude_mode: string;
  claude_config: string;
  claude_secret: string | null;
};

export type Member = { user_id: number; login: string; name: string | null; role: Role; created_at: number };

export const INVITE_TTL_MS = 7 * 24 * 60 * 60 * 1000;

export function isRole(value: unknown): value is Role {
  return typeof value === "string" && (ROLES as readonly string[]).includes(value);
}

export function isAdmin(role: Role | null | undefined): boolean {
  return role === "owner" || role === "admin";
}

// ---- Users ------------------------------------------------------------------------------

export async function upsertUser(db: Db, now: number, user: User): Promise<void> {
  await db.run(
    `INSERT INTO users (github_id, login, name, created_at, updated_at) VALUES (?, ?, ?, ?, ?)
     ON CONFLICT (github_id) DO UPDATE SET login = excluded.login, name = excluded.name, updated_at = excluded.updated_at`,
    user.github_id,
    user.login,
    user.name,
    now,
    now,
  );
}

export function getUser(db: Db, id: number): Promise<User | null> {
  return db.get<User>("SELECT github_id, login, name FROM users WHERE github_id = ?", id);
}

// ---- Orgs ------------------------------------------------------------------------------

const SLUG = /^[a-z0-9](?:[a-z0-9-]{0,38}[a-z0-9])?$/;

export function validSlug(slug: string): boolean {
  return SLUG.test(slug) && !slug.includes("--");
}

export function cleanName(name: unknown, max = 80): string {
  const s = typeof name === "string" ? name.replace(/\s+/g, " ").trim() : "";
  if (!s) throw new HttpError(400, "A name is required.");
  if (s.length > max) throw new HttpError(400, `Names are at most ${max} characters.`);
  return s;
}

export async function createOrg(db: Db, now: number, ownerId: number, rawName: unknown, rawSlug: unknown): Promise<Org> {
  const name = cleanName(rawName);
  const slug = typeof rawSlug === "string" ? rawSlug.trim().toLowerCase() : "";
  if (!validSlug(slug)) {
    throw new HttpError(400, "The slug can use a–z, 0–9 and single hyphens, up to 40 characters.");
  }
  if (await db.get("SELECT 1 AS x FROM orgs WHERE slug = ?", slug)) {
    throw new HttpError(409, `The slug “${slug}” is taken.`);
  }
  const id = crypto.randomUUID();
  await db.run("INSERT INTO orgs (id, name, slug, created_at) VALUES (?, ?, ?, ?)", id, name, slug, now);
  await db.run("INSERT INTO members (org_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)", id, ownerId, now);
  return (await orgById(db, id))!;
}

export function orgById(db: Db, id: string): Promise<Org | null> {
  return db.get<Org>("SELECT * FROM orgs WHERE id = ?", id);
}

export function orgBySlug(db: Db, slug: string): Promise<Org | null> {
  return db.get<Org>("SELECT * FROM orgs WHERE slug = ?", slug);
}

export async function roleIn(db: Db, orgId: string, userId: number): Promise<Role | null> {
  const row = await db.get<{ role: Role }>("SELECT role FROM members WHERE org_id = ? AND user_id = ?", orgId, userId);
  return row?.role ?? null;
}

export function orgsForUser(db: Db, userId: number): Promise<Array<Org & { role: Role }>> {
  return db.all(
    `SELECT o.*, m.role FROM orgs o JOIN members m ON m.org_id = o.id WHERE m.user_id = ? ORDER BY o.name`,
    userId,
  );
}

export function listMembers(db: Db, orgId: string): Promise<Member[]> {
  return db.all<Member>(
    `SELECT m.user_id, u.login, u.name, m.role, m.created_at FROM members m JOIN users u ON u.github_id = m.user_id
     WHERE m.org_id = ? ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END, u.login`,
    orgId,
  );
}

async function ownerCount(db: Db, orgId: string): Promise<number> {
  const row = await db.get<{ n: number }>("SELECT COUNT(*) AS n FROM members WHERE org_id = ? AND role = 'owner'", orgId);
  return row?.n ?? 0;
}

// ---- Role rules ------------------------------------------------------------------------
//
// - Owners can do everything, including making and unmaking owners.
// - Admins invite, revoke invites, remove members and admins, move people between member
//   and admin, and manage connections and Claude access. They can't touch owners.
// - Members see the org, link their own Bantu and leave.
// - An org always keeps at least one owner: the last owner can't leave, be removed or be
//   demoted.
// Each rule returns why the change isn't allowed, or null when it is.

export function roleChangeError(actor: Role, target: Role, to: Role, owners: number): string | null {
  if (!isAdmin(actor)) return "Only owners and admins can change roles.";
  if ((target === "owner" || to === "owner") && actor !== "owner") return "Only an owner can add or change an owner.";
  if (target === "owner" && to !== "owner" && owners <= 1) return "The last owner can't step down. Make someone else an owner first.";
  return null;
}

export function removalError(actor: Role, target: Role, owners: number, self: boolean): string | null {
  if (target === "owner" && owners <= 1) {
    return self ? "The last owner can't leave. Make someone else an owner first." : "The last owner can't be removed.";
  }
  if (self) return null;
  if (!isAdmin(actor)) return "Only owners and admins can remove people.";
  if (target === "owner" && actor !== "owner") return "Only an owner can remove an owner.";
  return null;
}

export async function changeRole(db: Db, orgId: string, actorId: number, targetId: number, to: unknown): Promise<void> {
  if (!isRole(to)) throw new HttpError(400, "Unknown role.");
  const [actor, target] = await Promise.all([roleIn(db, orgId, actorId), roleIn(db, orgId, targetId)]);
  if (!actor) throw new HttpError(403, "You aren't in this org.");
  if (!target) throw new HttpError(404, "That person isn't in this org.");
  const why = roleChangeError(actor, target, to, await ownerCount(db, orgId));
  if (why) throw new HttpError(403, why);
  await db.run("UPDATE members SET role = ? WHERE org_id = ? AND user_id = ?", to, orgId, targetId);
}

/** Removes someone (or yourself, to leave) and revokes every Bantu they linked to the org. */
export async function removeMember(db: Db, now: number, orgId: string, actorId: number, targetId: number): Promise<void> {
  const [actor, target] = await Promise.all([roleIn(db, orgId, actorId), roleIn(db, orgId, targetId)]);
  if (!actor) throw new HttpError(403, "You aren't in this org.");
  if (!target) throw new HttpError(404, "That person isn't in this org.");
  const why = removalError(actor, target, await ownerCount(db, orgId), actorId === targetId);
  if (why) throw new HttpError(403, why);
  await db.run("DELETE FROM members WHERE org_id = ? AND user_id = ?", orgId, targetId);
  await db.run("UPDATE daemons SET revoked_at = ? WHERE org_id = ? AND user_id = ? AND revoked_at IS NULL", now, orgId, targetId);
  await db.run("UPDATE join_codes SET used_at = ? WHERE org_id = ? AND user_id = ? AND used_at IS NULL", now, orgId, targetId);
}

// ---- Invites ---------------------------------------------------------------------------

export type Invite = {
  id: string;
  org_id: string;
  role: Role;
  created_by: number;
  created_at: number;
  expires_at: number;
  revoked_at: number | null;
  accepted_by: number | null;
  accepted_at: number | null;
};

/** Creates a link that lets one person join. The secret is returned once and stored hashed. */
export async function createInvite(
  db: Db,
  now: number,
  orgId: string,
  actorId: number,
  role: unknown,
): Promise<{ invite: Invite; token: string }> {
  const actor = await roleIn(db, orgId, actorId);
  if (!isAdmin(actor)) throw new HttpError(403, "Only owners and admins can invite people.");
  if (role !== "member" && role !== "admin") throw new HttpError(400, "Invites are for members or admins.");
  const id = crypto.randomUUID();
  const token = randomToken(24);
  await db.run(
    `INSERT INTO invites (id, org_id, token_hash, role, created_by, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)`,
    id,
    orgId,
    await sha256Hex(token),
    role,
    actorId,
    now,
    now + INVITE_TTL_MS,
  );
  return { invite: (await db.get<Invite>("SELECT * FROM invites WHERE id = ?", id))!, token };
}

/** Invites nobody has used, revoked or let expire. */
export function pendingInvites(db: Db, now: number, orgId: string): Promise<Invite[]> {
  return db.all<Invite>(
    `SELECT * FROM invites WHERE org_id = ? AND revoked_at IS NULL AND accepted_at IS NULL AND expires_at > ?
     ORDER BY created_at DESC`,
    orgId,
    now,
  );
}

export async function revokeInvite(db: Db, now: number, orgId: string, actorId: number, inviteId: string): Promise<void> {
  if (!isAdmin(await roleIn(db, orgId, actorId))) throw new HttpError(403, "Only owners and admins can revoke invites.");
  await db.run("UPDATE invites SET revoked_at = ? WHERE id = ? AND org_id = ? AND revoked_at IS NULL", now, inviteId, orgId);
}

export type InviteState = "valid" | "unknown" | "expired" | "revoked" | "used";

export async function inviteByToken(db: Db, now: number, token: string): Promise<{ state: InviteState; invite?: Invite; org?: Org }> {
  const invite = await db.get<Invite>("SELECT * FROM invites WHERE token_hash = ?", await sha256Hex(token));
  if (!invite) return { state: "unknown" };
  const org = (await orgById(db, invite.org_id)) ?? undefined;
  if (!org) return { state: "unknown" };
  const state: InviteState = invite.revoked_at
    ? "revoked"
    : invite.accepted_at
      ? "used"
      : invite.expires_at <= now
        ? "expired"
        : "valid";
  return { state, invite, org };
}

export type AcceptResult = { state: InviteState | "already_member"; org?: Org };

export async function acceptInvite(db: Db, now: number, token: string, userId: number): Promise<AcceptResult> {
  const found = await inviteByToken(db, now, token);
  if (found.state !== "valid" || !found.invite || !found.org) return { state: found.state, org: found.org };
  if (await roleIn(db, found.org.id, userId)) return { state: "already_member", org: found.org };
  // The WHERE clause makes this the one place an invite is used, so two people racing for
  // the same link can't both get in.
  const { changes } = await db.run(
    `UPDATE invites SET accepted_by = ?, accepted_at = ?
     WHERE id = ? AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > ?`,
    userId,
    now,
    found.invite.id,
    now,
  );
  if (changes !== 1) return { state: "used", org: found.org };
  await db.run(
    "INSERT INTO members (org_id, user_id, role, created_at) VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
    found.org.id,
    userId,
    found.invite.role,
    now,
  );
  return { state: "valid", org: found.org };
}
