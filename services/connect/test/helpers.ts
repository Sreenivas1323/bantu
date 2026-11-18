// Test harness: the real app and schema on an in-memory bun:sqlite database, a stubbed GitHub,
// a fixed clock, and a small browser with a cookie jar.

import { Database } from "bun:sqlite";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { handle } from "../src/core/app.ts";
import type { Deps, Secrets } from "../src/core/context.ts";
import type { Db, Param } from "../src/core/db.ts";
import { AUTHORIZE_URL, TOKEN_URL, USER_URL } from "../src/core/github.ts";

export const ORIGIN = "https://connect.test";
export const T0 = Date.parse("2026-11-18T12:00:00Z");

export class SqliteDb implements Db {
  readonly sqlite = new Database(":memory:");

  constructor() {
    this.sqlite.exec("PRAGMA foreign_keys = ON");
    const dir = join(import.meta.dir, "..", "migrations");
    for (const f of readdirSync(dir).filter((f) => f.endsWith(".sql")).sort()) {
      this.sqlite.exec(readFileSync(join(dir, f), "utf8"));
    }
  }
  async run(sql: string, ...params: Param[]) {
    return { changes: this.sqlite.query(sql).run(...params).changes };
  }
  async get<T>(sql: string, ...params: Param[]) {
    return (this.sqlite.query(sql).get(...params) as T | null) ?? null;
  }
  async all<T>(sql: string, ...params: Param[]) {
    return this.sqlite.query(sql).all(...params) as T[];
  }
}

export const ENV: Secrets = {
  GITHUB_CLIENT_ID: "test-client-id",
  GITHUB_CLIENT_SECRET: "test-client-secret",
  SESSION_SECRET: "test-session-secret-0123456789abcdef",
  ENCRYPTION_KEY: Buffer.alloc(32, 7).toString("base64"),
};

export type GhUser = { id: number; login: string; name?: string };

/** GitHub's token and user endpoints. OAuth `code` values are the user's login. */
export function stubGitHub(users: GhUser[]) {
  const calls: Array<{ url: string; body?: string; auth?: string | null }> = [];
  const byLogin = new Map(users.map((u) => [u.login, u]));
  const fetcher = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    const headers = new Headers(init?.headers);
    if (url === TOKEN_URL) {
      const body = String(init?.body);
      calls.push({ url, body });
      const p = new URLSearchParams(body);
      if (p.get("client_secret") !== ENV.GITHUB_CLIENT_SECRET || !byLogin.has(p.get("code") ?? "")) {
        return Response.json({ error: "bad_verification_code" });
      }
      return Response.json({ access_token: `gho_${p.get("code")}`, token_type: "bearer", scope: "" });
    }
    if (url === USER_URL) {
      const auth = headers.get("authorization");
      calls.push({ url, auth });
      const u = byLogin.get((auth ?? "").replace("Bearer gho_", ""));
      return u ? Response.json({ id: u.id, login: u.login, name: u.name ?? null }) : Response.json({ message: "Bad credentials" }, { status: 401 });
    }
    throw new Error(`unexpected fetch ${url}`);
  }) as typeof fetch;
  return { fetcher, calls };
}

export type World = { db: SqliteDb; deps: Deps; clock: { now: number }; gh: ReturnType<typeof stubGitHub> };

export function world(users: GhUser[] = DEFAULT_USERS): World {
  const db = new SqliteDb();
  const clock = { now: T0 };
  const gh = stubGitHub(users);
  return { db, clock, gh, deps: { db, env: { ...ENV }, fetch: gh.fetcher, now: () => clock.now } };
}

export const DEFAULT_USERS: GhUser[] = [
  { id: 101, login: "ada" },
  { id: 102, login: "grace" },
  { id: 103, login: "linus" },
  { id: 104, login: "margaret" },
  { id: 105, login: "ken" },
];

export function csrfFrom(html: string): string {
  const m = html.match(/name="csrf" value="([^"]+)"/);
  if (!m) throw new Error("no csrf field on the page");
  return m[1]!;
}

export class Browser {
  readonly cookies = new Map<string, string>();
  csrf = "";

  constructor(readonly w: World) {}

  async request(method: string, path: string, opts: { form?: Record<string, string>; headers?: Record<string, string>; body?: string } = {}) {
    const headers = new Headers(opts.headers);
    if (this.cookies.size) headers.set("cookie", [...this.cookies].map(([k, v]) => `${k}=${v}`).join("; "));
    let body: string | undefined = opts.body;
    if (opts.form) {
      headers.set("content-type", "application/x-www-form-urlencoded");
      body = new URLSearchParams(opts.form).toString();
    }
    const res = await handle(new Request(ORIGIN + path, { method, headers, body }), this.w.deps);
    for (const c of res.headers.getSetCookie()) {
      const [pair] = c.split(";");
      const eq = pair!.indexOf("=");
      const name = pair!.slice(0, eq);
      const value = pair!.slice(eq + 1);
      if (/Max-Age=0/.test(c) || value === "") this.cookies.delete(name);
      else this.cookies.set(name, value);
    }
    const text = await res.text();
    if (text.includes('name="csrf"')) this.csrf = csrfFrom(text);
    return { res, text, status: res.status, location: res.headers.get("location") };
  }

  get(path: string) {
    return this.request("GET", path);
  }

  /** Posts a form with the CSRF token from the last page this browser loaded. */
  post(path: string, fields: Record<string, string> = {}) {
    return this.request("POST", path, { form: { csrf: this.csrf, ...fields } });
  }

  async signIn(login: string) {
    const start = await this.get("/auth/github?next=/orgs");
    const authorize = new URL(start.location!);
    if (authorize.origin + authorize.pathname !== AUTHORIZE_URL) throw new Error("didn't redirect to GitHub");
    const state = authorize.searchParams.get("state")!;
    const back = await this.get(`/auth/github/callback?code=${login}&state=${state}`);
    if (back.status !== 303) throw new Error(`sign-in failed: ${back.status} ${back.text.slice(0, 200)}`);
    await this.get("/orgs");
    return back;
  }
}

/** Signs `login` in, in a fresh browser. */
export async function as(w: World, login: string): Promise<Browser> {
  const b = new Browser(w);
  await b.signIn(login);
  return b;
}

/** Creates an org owned by `owner` and returns its slug. */
export async function makeOrg(owner: Browser, name = "Acme", slug = "acme"): Promise<string> {
  await owner.get("/orgs/new");
  const r = await owner.post("/orgs", { name, slug });
  if (r.status !== 303) throw new Error(`org create failed: ${r.status} ${r.text.slice(0, 300)}`);
  await owner.get(`/o/${slug}`);
  return slug;
}

/** Makes an invite as `admin` and has `who` accept it. */
export async function invite(admin: Browser, slug: string, who: Browser, role: "member" | "admin" = "member") {
  await admin.get(`/o/${slug}`);
  const r = await admin.post(`/o/${slug}/invites`, { role });
  const link = r.text.match(/https:\/\/connect\.test\/join\/([A-Za-z0-9_-]+)/);
  if (!link) throw new Error("no invite link on the page");
  const path = `/join/${link[1]}`;
  await who.get(path);
  const accepted = await who.post(path);
  return { path, accepted };
}

export function codeFrom(html: string): string {
  const m = html.match(/https:\/\/connect\.test\/c\/([0-9A-Z]{10})/);
  if (!m) throw new Error("no join link on the page");
  return m[1]!;
}

/** Gets a join code as `member` and exchanges it like a daemon would. */
export async function linkDaemon(member: Browser, slug: string, name = "laptop") {
  await member.get(`/o/${slug}`);
  const page = await member.post(`/o/${slug}/code`);
  const code = codeFrom(page.text);
  const res = await api(member.w, "POST", "/api/daemon/join", { code, name });
  return { code, res, body: (await res.json()) as any };
}

export async function api(w: World, method: string, path: string, body?: unknown, token?: string): Promise<Response> {
  const headers = new Headers();
  if (body !== undefined) headers.set("content-type", "application/json");
  if (token) headers.set("authorization", `Bearer ${token}`);
  return handle(new Request(ORIGIN + path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), w.deps);
}
