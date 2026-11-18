// Shared tracker connections. GitHub isn't here: each member's Bantu uses their own `gh` login.
// Tokens are encrypted with AES-GCM and only ever leave in a daemon's sync response.

import { decrypt, encrypt } from "./crypto.ts";
import type { Db } from "./db.ts";
import { HttpError } from "./context.ts";
import { cleanName } from "./orgs.ts";

export type Kind = "jira" | "clickup" | "gitlab";
export const KINDS: readonly Kind[] = ["jira", "clickup", "gitlab"];
export const KIND_LABELS: Record<Kind, string> = { jira: "Jira", clickup: "ClickUp", gitlab: "GitLab" };

/** What a page may show. There's no token field, so it can't be rendered by mistake. */
export type ConnectionSummary = {
  id: string;
  kind: Kind;
  name: string;
  site: string | null;
  scope: string;
  account: string | null;
  created_at: number;
};

/** What a daemon receives. */
export type ConnectionWithToken = Omit<ConnectionSummary, "created_at"> & { token: string };

type Row = ConnectionSummary & { token_enc: string };

function field(form: Record<string, string>, key: string, label: string, max = 500): string {
  const v = (form[key] ?? "").trim();
  if (!v) throw new HttpError(400, `${label} is required.`);
  if (v.length > max) throw new HttpError(400, `${label} is too long.`);
  return v;
}

/** An https origin, such as https://acme.atlassian.net or https://gitlab.example.com. */
function site(form: Record<string, string>, label: string): string {
  const raw = field(form, "site", label, 200);
  let url: URL;
  try {
    url = new URL(/^[a-z]+:\/\//i.test(raw) ? raw : `https://${raw}`);
  } catch {
    throw new HttpError(400, `${label} isn't a valid URL.`);
  }
  if (url.protocol !== "https:") throw new HttpError(400, `${label} must use https.`);
  return url.origin + url.pathname.replace(/\/+$/, "");
}

export async function addConnection(
  db: Db,
  now: number,
  orgId: string,
  actorId: number,
  form: Record<string, string>,
  encryptionKey: string,
): Promise<string> {
  const kind = form.kind as Kind;
  if (!KINDS.includes(kind)) throw new HttpError(400, "Unknown tracker.");
  let siteValue: string | null = null;
  let account: string | null = null;
  let scope: string;
  if (kind === "jira") {
    siteValue = site(form, "The Jira site");
    account = field(form, "account", "The Atlassian account email", 200);
    if (!/^[^\s@]+@[^\s@]+$/.test(account)) throw new HttpError(400, "The Atlassian account must be an email address.");
    scope = field(form, "scope", "A project key or JQL", 1000);
  } else if (kind === "clickup") {
    scope = field(form, "scope", "The ClickUp list id", 40);
    if (!/^[A-Za-z0-9]+$/.test(scope)) throw new HttpError(400, "A ClickUp list id is letters and digits.");
  } else {
    siteValue = site(form, "The GitLab site");
    scope = field(form, "scope", "The project path", 300);
    if (!/^[\w.-]+(?:\/[\w.-]+)+$/.test(scope)) throw new HttpError(400, "The project path looks like group/project.");
  }
  const token = field(form, "token", "The API token", 4000);
  const name = cleanName(form.name || `${KIND_LABELS[kind]} ${scope}`.slice(0, 80));
  const id = crypto.randomUUID();
  await db.run(
    `INSERT INTO connections (id, org_id, kind, name, site, scope, account, token_enc, created_by, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    id,
    orgId,
    kind,
    name,
    siteValue,
    scope,
    account,
    await encrypt(encryptionKey, token, `connection:${id}`),
    actorId,
    now,
  );
  return id;
}

export function listConnections(db: Db, orgId: string): Promise<ConnectionSummary[]> {
  return db.all<ConnectionSummary>(
    "SELECT id, kind, name, site, scope, account, created_at FROM connections WHERE org_id = ? ORDER BY created_at",
    orgId,
  );
}

export async function connectionsWithTokens(db: Db, orgId: string, encryptionKey: string): Promise<ConnectionWithToken[]> {
  const rows = await db.all<Row>("SELECT * FROM connections WHERE org_id = ? ORDER BY created_at", orgId);
  return Promise.all(
    rows.map(async (r) => ({
      id: r.id,
      kind: r.kind,
      name: r.name,
      site: r.site,
      scope: r.scope,
      account: r.account,
      token: await decrypt(encryptionKey, r.token_enc, `connection:${r.id}`),
    })),
  );
}

export async function deleteConnection(db: Db, orgId: string, id: string): Promise<void> {
  await db.run("DELETE FROM connections WHERE id = ? AND org_id = ?", id, orgId);
}
