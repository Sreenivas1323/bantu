// Every route. src/worker.ts calls `handle` with D1 behind the Db interface; the tests call it
// with bun:sqlite. See docs/CONNECT.md for the daemon API.

import { parseConfig, saveClaudeAccess, type ClaudeMode } from "./claude.ts";
import { addConnection, deleteConnection, listConnections } from "./connections.ts";
import { HttpError, required, type Deps } from "./context.ts";
import { randomToken } from "./crypto.ts";
import { authDaemon, createJoinCode, exchangeCode, linkedDaemons, recordTotals, revokeDaemon, sync } from "./daemons.ts";
import { authorizeUrl, exchangeForUser } from "./github.ts";
import {
  acceptInvite,
  changeRole,
  createInvite,
  createOrg,
  getUser,
  inviteByToken,
  isAdmin,
  listMembers,
  orgBySlug,
  orgsForUser,
  pendingInvites,
  removeMember,
  revokeInvite,
  roleIn,
  upsertUser,
  type Org,
  type Role,
  type User,
} from "./orgs.ts";
import {
  codeLinkPage,
  codePage,
  dashboardPage,
  errorPage,
  joinPage,
  landingPage,
  newOrgPage,
  orgsPage,
  type DashboardData,
} from "./pages.ts";
import {
  checkCsrf,
  checkState,
  clearCookie,
  csrfToken,
  readSession,
  safeNext,
  sessionCookie,
  SESSION_COOKIE,
  SESSION_TTL_MS,
  STATE_COOKIE,
  STATE_TTL_MS,
  stateCookie,
  type Session,
} from "./session.ts";
import { STYLES } from "./styles.ts";
import { teamTotals } from "./totals.ts";

const MAX_BODY = 64 * 1024;
type HeaderBag = Headers | Record<string, string>;

const CSP = "default-src 'none'; style-src 'self'; img-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

function htmlResponse(body: string, status = 200, headers: HeaderBag = {}): Response {
  const h = new Headers(headers);
  h.set("content-type", "text/html; charset=utf-8");
  h.set("cache-control", "no-store");
  h.set("content-security-policy", CSP);
  return new Response(body, { status, headers: h });
}

function json(body: unknown, status = 200, headers: HeaderBag = {}): Response {
  const h = new Headers(headers);
  h.set("content-type", "application/json; charset=utf-8");
  h.set("cache-control", "no-store");
  return new Response(JSON.stringify(body), { status, headers: h });
}

function redirect(location: string, headers: HeaderBag = {}): Response {
  const h = new Headers(headers);
  h.set("location", location);
  h.set("cache-control", "no-store");
  return new Response(null, { status: 303, headers: h });
}

function withSecurityHeaders(res: Response): Response {
  const h = new Headers(res.headers);
  h.set("x-content-type-options", "nosniff");
  // Invite and join links carry secrets in the path; never send them on as a Referer.
  h.set("referrer-policy", "no-referrer");
  h.set("x-frame-options", "DENY");
  h.set("strict-transport-security", "max-age=31536000");
  return new Response(res.body, { status: res.status, statusText: res.statusText, headers: h });
}

export function publicOrigin(deps: Deps, request: Request): string {
  return (deps.env.PUBLIC_URL || new URL(request.url).origin).replace(/\/+$/, "");
}

async function readForm(request: Request): Promise<Record<string, string>> {
  const len = Number(request.headers.get("content-length") ?? 0);
  if (len > MAX_BODY) throw new HttpError(413, "That form is too large.");
  const type = request.headers.get("content-type") ?? "";
  if (!type.startsWith("application/x-www-form-urlencoded") && !type.startsWith("multipart/form-data")) {
    throw new HttpError(415, "Expected a form.");
  }
  const out: Record<string, string> = {};
  for (const [k, v] of await request.formData()) if (typeof v === "string") out[k] = v;
  return out;
}

async function readJson(request: Request): Promise<unknown> {
  const text = await request.text();
  if (text.length > MAX_BODY) throw new HttpError(413, "body too large");
  try {
    return JSON.parse(text);
  } catch {
    throw new HttpError(400, "body must be JSON");
  }
}

type Web = { session: Session; user: User; csrf: string };

async function currentUser(deps: Deps, request: Request): Promise<Web | null> {
  const secret = required(deps.env, "SESSION_SECRET");
  const session = await readSession(secret, request, deps.now());
  if (!session) return null;
  const user = await getUser(deps.db, session.uid);
  if (!user) return null;
  return { session, user, csrf: await csrfToken(secret, session) };
}

/**
 * Every POST from a page: a signed-in session and the form's CSRF token. A browser's Origin
 * header, when it sends one, must also be this site.
 */
async function postedForm(deps: Deps, request: Request, web: Web | null): Promise<{ web: Web; form: Record<string, string> }> {
  if (!web) throw new HttpError(403, "You're signed out. Sign in and try again.");
  const origin = request.headers.get("origin");
  if (origin && origin !== "null" && origin !== new URL(request.url).origin && origin !== publicOrigin(deps, request)) {
    throw new HttpError(403, "That form came from another site.");
  }
  const form = await readForm(request);
  if (!(await checkCsrf(required(deps.env, "SESSION_SECRET"), web.session, form.csrf))) {
    throw new HttpError(403, "This form has expired. Reload the page and try again.");
  }
  return { web, form };
}

const FLASH: Record<string, string> = {
  created: "Org created. You're its owner.",
  role: "Role changed.",
  removed: "Removed from the org. Their linked Bantu apps were revoked.",
  invite_revoked: "Invite revoked.",
  daemon_revoked: "That Bantu was revoked. It can't sync any more.",
  connection_added: "Connection added. Its token is encrypted and won't be shown again.",
  connection_deleted: "Connection deleted.",
  claude_saved: "Claude access saved. Linked Bantu apps pick it up at their next sync.",
};

async function dashboard(deps: Deps, request: Request, web: Web, org: Org, role: Role, extra: Partial<DashboardData> = {}): Promise<DashboardData> {
  const now = deps.now();
  const [members, invites, daemons, connections, totals] = await Promise.all([
    listMembers(deps.db, org.id),
    isAdmin(role) ? pendingInvites(deps.db, now, org.id) : Promise.resolve([]),
    linkedDaemons(deps.db, org.id),
    listConnections(deps.db, org.id),
    teamTotals(deps.db, now, org.id),
  ]);
  const flashKey = new URL(request.url).searchParams.get("done");
  return {
    user: web.user,
    csrf: web.csrf,
    now,
    origin: publicOrigin(deps, request),
    org,
    role,
    members,
    invites,
    daemons,
    connections,
    claude: { mode: org.claude_mode as ClaudeMode, config: parseConfig(org), hasApiKey: org.claude_mode === "api_key" && !!org.claude_secret },
    totals,
    flash: flashKey ? FLASH[flashKey] : undefined,
    ...extra,
  };
}

// ---- Routes ------------------------------------------------------------------------------

async function api(deps: Deps, request: Request, path: string): Promise<Response> {
  const method = request.method;
  const now = deps.now();
  if (path === "/api/daemon/join") {
    if (method !== "POST") return json({ error: "method not allowed" }, 405, { allow: "POST" });
    const body = (await readJson(request)) as { code?: unknown; name?: unknown } | null;
    return json(await exchangeCode(deps.db, now, body?.code, body?.name));
  }
  const routes: Record<string, string> = { "/api/daemon/sync": "GET", "/api/daemon/totals": "POST", "/api/daemon/leave": "POST" };
  const want = routes[path];
  if (!want) return json({ error: "not found" }, 404);
  if (method !== want) return json({ error: "method not allowed" }, 405, { allow: want });
  const auth = await authDaemon(deps.db, request.headers.get("authorization"));
  if (!auth) return json({ error: "unauthorized" }, 401, { "www-authenticate": 'Bearer realm="bantu-connect"' });
  if (path === "/api/daemon/sync") return json(await sync(deps.db, now, auth, required(deps.env, "ENCRYPTION_KEY")));
  if (path === "/api/daemon/totals") {
    await recordTotals(deps.db, now, auth, await readJson(request));
    return new Response(null, { status: 204 });
  }
  await revokeDaemon(deps.db, now, auth.daemon.id);
  return new Response(null, { status: 204 });
}

async function auth(deps: Deps, request: Request, path: string): Promise<Response> {
  const url = new URL(request.url);
  const secret = required(deps.env, "SESSION_SECRET");
  const clientId = required(deps.env, "GITHUB_CLIENT_ID");
  const callback = `${publicOrigin(deps, request)}/auth/github/callback`;
  if (path === "/auth/github") {
    const state = randomToken(16);
    const next = safeNext(url.searchParams.get("next"));
    const setCookie = await stateCookie(secret, { state, next, exp: deps.now() + STATE_TTL_MS });
    return redirect(authorizeUrl(clientId, callback, state), { "set-cookie": setCookie });
  }
  // The callback.
  if (url.searchParams.get("error")) {
    return htmlResponse(errorPage(400, "GitHub sign-in was cancelled. You can try again from the start page."), 400);
  }
  const st = await checkState(secret, request, url.searchParams.get("state"), deps.now());
  const code = url.searchParams.get("code");
  if (!st || !code) {
    return htmlResponse(errorPage(400, "This sign-in expired or didn't start here. Start again from the sign-in page."), 400, {
      "set-cookie": clearCookie(STATE_COOKIE),
    });
  }
  const user = await exchangeForUser(deps.fetch, clientId, required(deps.env, "GITHUB_CLIENT_SECRET"), code, callback);
  const now = deps.now();
  await upsertUser(deps.db, now, user);
  const headers = new Headers();
  headers.append("set-cookie", clearCookie(STATE_COOKIE));
  headers.append("set-cookie", await sessionCookie(secret, { uid: user.github_id, sid: randomToken(16), exp: now + SESSION_TTL_MS }));
  return redirect(st.next, headers);
}

async function orgRoutes(deps: Deps, request: Request, web: Web | null, slug: string, rest: string): Promise<Response> {
  if (!web) return redirect(`/?next=${encodeURIComponent(new URL(request.url).pathname)}`);
  const org = await orgBySlug(deps.db, slug);
  const role = org ? await roleIn(deps.db, org.id, web.user.github_id) : null;
  // Someone outside the org can't tell it exists.
  if (!org || !role) throw new HttpError(404, "There's no org here, or you aren't in it.");
  const base = `/o/${org.slug}`;

  if (request.method === "GET" && rest === "") return htmlResponse(dashboardPage(await dashboard(deps, request, web, org, role)));
  if (request.method !== "POST") throw new HttpError(405, "Method not allowed.");

  const { form } = await postedForm(deps, request, web);
  const now = deps.now();
  const me = web.user.github_id;
  const key = () => required(deps.env, "ENCRYPTION_KEY");
  try {
    let m: RegExpMatchArray | null;
    if (rest === "/code") {
      const { code, expiresAt } = await createJoinCode(deps.db, now, org.id, me);
      return htmlResponse(codePage({ user: web.user, csrf: web.csrf, org, code, expiresAt, link: `${publicOrigin(deps, request)}/c/${code}` }));
    }
    if (rest === "/invites") {
      const { invite, token } = await createInvite(deps.db, now, org.id, me, form.role);
      const link = `${publicOrigin(deps, request)}/join/${token}`;
      const data = await dashboard(deps, request, web, org, role, { newInvite: { link, role: invite.role, expiresAt: invite.expires_at } });
      return htmlResponse(dashboardPage(data));
    }
    if ((m = rest.match(/^\/invites\/([\w-]+)\/revoke$/))) {
      await revokeInvite(deps.db, now, org.id, me, m[1]!);
      return redirect(`${base}?done=invite_revoked`);
    }
    if ((m = rest.match(/^\/members\/(\d+)\/role$/))) {
      await changeRole(deps.db, org.id, me, Number(m[1]), form.role);
      return redirect(`${base}?done=role`);
    }
    if ((m = rest.match(/^\/members\/(\d+)\/remove$/))) {
      await removeMember(deps.db, now, org.id, me, Number(m[1]));
      return redirect(Number(m[1]) === me ? "/orgs" : `${base}?done=removed`);
    }
    if (rest === "/leave") {
      await removeMember(deps.db, now, org.id, me, me);
      return redirect("/orgs");
    }
    if ((m = rest.match(/^\/daemons\/([\w-]+)\/revoke$/))) {
      const d = await deps.db.get<{ user_id: number }>("SELECT user_id FROM daemons WHERE id = ? AND org_id = ?", m[1]!, org.id);
      if (!d) throw new HttpError(404, "That Bantu isn't linked to this org.");
      if (d.user_id !== me && !isAdmin(role)) throw new HttpError(403, "You can only revoke your own Bantu.");
      await revokeDaemon(deps.db, now, m[1]!);
      return redirect(`${base}?done=daemon_revoked`);
    }
    if (rest === "/connections") {
      if (!isAdmin(role)) throw new HttpError(403, "Only owners and admins can add connections.");
      await addConnection(deps.db, now, org.id, me, form, key());
      return redirect(`${base}?done=connection_added`);
    }
    if ((m = rest.match(/^\/connections\/([\w-]+)\/delete$/))) {
      if (!isAdmin(role)) throw new HttpError(403, "Only owners and admins can delete connections.");
      await deleteConnection(deps.db, org.id, m[1]!);
      return redirect(`${base}?done=connection_deleted`);
    }
    if (rest === "/claude") {
      if (!isAdmin(role)) throw new HttpError(403, "Only owners and admins can change Claude access.");
      await saveClaudeAccess(deps.db, org, form, key());
      return redirect(`${base}?done=claude_saved`);
    }
  } catch (e) {
    // Show the problem on the org page, so the rest of the page is still there.
    if (e instanceof HttpError && e.status < 500 && e.status !== 404) {
      const fresh = (await orgBySlug(deps.db, slug)) ?? org;
      const freshRole = (await roleIn(deps.db, fresh.id, me)) ?? role;
      return htmlResponse(dashboardPage(await dashboard(deps, request, web, fresh, freshRole, { error: e.message, flash: undefined })), e.status);
    }
    throw e;
  }
  throw new HttpError(404, "Not found.");
}

async function joinRoutes(deps: Deps, request: Request, web: Web | null, token: string, path: string): Promise<Response> {
  const now = deps.now();
  if (request.method === "GET") {
    const found = await inviteByToken(deps.db, now, token);
    let state: Parameters<typeof joinPage>[0]["state"] = found.state;
    if (state === "valid" && web && found.org && (await roleIn(deps.db, found.org.id, web.user.github_id))) state = "already_member";
    const status = state === "valid" || state === "already_member" ? 200 : state === "unknown" ? 404 : 410;
    return htmlResponse(joinPage({ state, org: found.org, role: found.invite?.role, user: web?.user ?? null, csrf: web?.csrf, path }), status);
  }
  if (request.method !== "POST") throw new HttpError(405, "Method not allowed.");
  const { web: w } = await postedForm(deps, request, web);
  const result = await acceptInvite(deps.db, now, token, w.user.github_id);
  if (result.state === "already_member" && result.org) return redirect(`/o/${result.org.slug}`);
  if (result.state !== "valid" || !result.org) {
    const status = result.state === "unknown" ? 404 : 410;
    return htmlResponse(joinPage({ state: result.state, org: result.org, user: w.user, csrf: w.csrf, path }), status);
  }
  // Straight to the one-time link, so connecting Bantu is the next thing they see.
  const { code, expiresAt } = await createJoinCode(deps.db, now, result.org.id, w.user.github_id);
  return htmlResponse(
    codePage({ user: w.user, csrf: w.csrf, org: result.org, code, expiresAt, link: `${publicOrigin(deps, request)}/c/${code}`, joined: true }),
  );
}

async function route(deps: Deps, request: Request): Promise<Response> {
  const url = new URL(request.url);
  const path = url.pathname.length > 1 ? url.pathname.replace(/\/+$/, "") : url.pathname;
  const method = request.method;

  if (path.startsWith("/api/")) return api(deps, request, path);
  if (path === "/styles.css") {
    return new Response(STYLES, { headers: { "content-type": "text/css; charset=utf-8", "cache-control": "public, max-age=3600" } });
  }
  if (path === "/auth/github" || path === "/auth/github/callback") {
    if (method !== "GET") throw new HttpError(405, "Method not allowed.");
    return auth(deps, request, path);
  }
  let m: RegExpMatchArray | null;
  if ((m = path.match(/^\/c\/([A-Za-z0-9-]{1,40})$/)) && method === "GET") return htmlResponse(codeLinkPage());

  const web = await currentUser(deps, request);

  if (path === "/" && method === "GET") {
    const next = safeNext(url.searchParams.get("next"));
    return web ? redirect(next) : htmlResponse(landingPage(next));
  }
  if (path === "/logout" && method === "POST") {
    await postedForm(deps, request, web);
    return redirect("/", { "set-cookie": clearCookie(SESSION_COOKIE) });
  }
  if (path === "/orgs" && method === "GET") {
    if (!web) return redirect("/?next=/orgs");
    return htmlResponse(orgsPage(web.user, web.csrf, await orgsForUser(deps.db, web.user.github_id)));
  }
  if (path === "/orgs/new" && method === "GET") {
    if (!web) return redirect("/?next=/orgs/new");
    return htmlResponse(newOrgPage(web.user, web.csrf));
  }
  if (path === "/orgs" && method === "POST") {
    const { web: w, form } = await postedForm(deps, request, web);
    try {
      const org = await createOrg(deps.db, deps.now(), w.user.github_id, form.name, form.slug);
      return redirect(`/o/${org.slug}?done=created`);
    } catch (e) {
      if (e instanceof HttpError && e.status < 500) {
        return htmlResponse(newOrgPage(w.user, w.csrf, e.message, { name: form.name, slug: form.slug }), e.status);
      }
      throw e;
    }
  }
  if ((m = path.match(/^\/o\/([a-z0-9-]{1,40})(\/.*)?$/))) return orgRoutes(deps, request, web, m[1]!, m[2] ?? "");
  if ((m = path.match(/^\/join\/([A-Za-z0-9_-]{16,64})$/))) return joinRoutes(deps, request, web, m[1]!, path);

  throw new HttpError(404, "There's nothing at this address.");
}

export async function handle(request: Request, deps: Deps): Promise<Response> {
  const isApi = new URL(request.url).pathname.startsWith("/api/");
  try {
    return withSecurityHeaders(await route(deps, request));
  } catch (e) {
    if (e instanceof HttpError) {
      if (isApi) return withSecurityHeaders(json({ error: e.message }, e.status));
      let chrome = {};
      try {
        chrome = (await currentUser(deps, request)) ?? {};
      } catch {
        // A missing SESSION_SECRET is the error being reported; render without the header.
      }
      return withSecurityHeaders(htmlResponse(errorPage(e.status, e.message, chrome), e.status));
    }
    console.error("unhandled error", e);
    return withSecurityHeaders(
      isApi ? json({ error: "internal error" }, 500) : htmlResponse(errorPage(500, "Something went wrong on our side. Try again."), 500),
    );
  }
}
