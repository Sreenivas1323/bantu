import { describe, expect, test } from "bun:test";

import { handle } from "../src/core/app.ts";
import { safeNext, SESSION_COOKIE, STATE_COOKIE } from "../src/core/session.ts";
import { as, Browser, ENV, makeOrg, ORIGIN, world } from "./helpers.ts";

describe("GitHub sign-in", () => {
  test("redirects to GitHub with a state, and signs in when the callback's state matches", async () => {
    const w = world();
    const b = new Browser(w);
    const start = await b.get("/auth/github?next=/orgs/new");
    expect(start.status).toBe(303);
    const to = new URL(start.location!);
    expect(to.origin + to.pathname).toBe("https://github.com/login/oauth/authorize");
    expect(to.searchParams.get("client_id")).toBe(ENV.GITHUB_CLIENT_ID!);
    expect(to.searchParams.get("redirect_uri")).toBe(`${ORIGIN}/auth/github/callback`);
    const state = to.searchParams.get("state")!;
    expect(state.length).toBeGreaterThanOrEqual(20);

    const setCookie = start.res.headers.getSetCookie().join("\n");
    expect(setCookie).toContain(`${STATE_COOKIE}=`);
    expect(setCookie).toContain("HttpOnly");
    expect(setCookie).toContain("Secure");
    expect(setCookie).toContain("SameSite=Lax");

    const back = await b.get(`/auth/github/callback?code=ada&state=${state}`);
    expect(back.status).toBe(303);
    expect(back.location).toBe("/orgs/new");
    const cookies = back.res.headers.getSetCookie().join("\n");
    expect(cookies).toMatch(new RegExp(`${SESSION_COOKIE}=[^;]+; Path=/; HttpOnly; Secure; SameSite=Lax`));
    expect(b.cookies.has(STATE_COOKIE)).toBe(false);

    // GitHub was asked for a token with the app's secret, then for the user, and nothing else.
    expect(w.gh.calls.map((c) => c.url)).toEqual(["https://github.com/login/oauth/access_token", "https://api.github.com/user"]);
    expect(new URLSearchParams(w.gh.calls[0]!.body).get("redirect_uri")).toBe(`${ORIGIN}/auth/github/callback`);

    const orgs = await b.get("/orgs");
    expect(orgs.status).toBe(200);
    expect(orgs.text).toContain("ada");
    const row = await w.db.get<{ login: string }>("SELECT login FROM users WHERE github_id = 101");
    expect(row?.login).toBe("ada");
  });

  test("rejects a callback whose state doesn't match the cookie, without calling GitHub", async () => {
    const w = world();
    const b = new Browser(w);
    await b.get("/auth/github");
    const r = await b.get("/auth/github/callback?code=ada&state=forged-state-value");
    expect(r.status).toBe(400);
    expect(w.gh.calls).toHaveLength(0);
    expect(b.cookies.has(SESSION_COOKIE)).toBe(false);
  });

  test("rejects a callback with no state cookie (a sign-in started in another browser)", async () => {
    const w = world();
    const victim = new Browser(w);
    const start = await new Browser(w).get("/auth/github");
    const state = new URL(start.location!).searchParams.get("state")!;
    const r = await victim.get(`/auth/github/callback?code=ada&state=${state}`);
    expect(r.status).toBe(400);
    expect(w.gh.calls).toHaveLength(0);
  });

  test("rejects a state cookie older than ten minutes", async () => {
    const w = world();
    const b = new Browser(w);
    const start = await b.get("/auth/github");
    const state = new URL(start.location!).searchParams.get("state")!;
    w.clock.now += 11 * 60 * 1000;
    expect((await b.get(`/auth/github/callback?code=ada&state=${state}`)).status).toBe(400);
  });

  test("rejects a tampered state cookie", async () => {
    const w = world();
    const b = new Browser(w);
    const start = await b.get("/auth/github");
    const state = new URL(start.location!).searchParams.get("state")!;
    const v = b.cookies.get(STATE_COOKIE)!;
    b.cookies.set(STATE_COOKIE, v.slice(0, -2) + (v.endsWith("AA") ? "BB" : "AA"));
    expect((await b.get(`/auth/github/callback?code=ada&state=${state}`)).status).toBe(400);
  });

  test("a code GitHub refuses doesn't sign anyone in", async () => {
    const w = world();
    const b = new Browser(w);
    const start = await b.get("/auth/github");
    const state = new URL(start.location!).searchParams.get("state")!;
    const r = await b.get(`/auth/github/callback?code=nobody&state=${state}`);
    expect(r.status).toBe(400);
    expect(b.cookies.has(SESSION_COOKIE)).toBe(false);
  });

  test("only same-origin paths are used after sign-in", () => {
    expect(safeNext("/join/abc")).toBe("/join/abc");
    expect(safeNext("https://evil.example")).toBe("/orgs");
    expect(safeNext("//evil.example")).toBe("/orgs");
    expect(safeNext("/\\evil.example")).toBe("/orgs");
    expect(safeNext(null)).toBe("/orgs");
  });

  test("a forged or expired session cookie is ignored", async () => {
    const w = world();
    const b = await as(w, "ada");
    const good = b.cookies.get(SESSION_COOKIE)!;
    b.cookies.set(SESSION_COOKIE, good.replace(/.$/, good.endsWith("x") ? "y" : "x"));
    expect((await b.get("/orgs")).status).toBe(303);
    b.cookies.set(SESSION_COOKIE, good);
    expect((await b.get("/orgs")).status).toBe(200);
    w.clock.now += 31 * 24 * 60 * 60 * 1000;
    expect((await b.get("/orgs")).status).toBe(303);
  });
});

describe("CSRF", () => {
  test("every form POST needs the session's token", async () => {
    const w = world();
    const ada = await as(w, "ada");
    await ada.get("/orgs/new");

    const none = await ada.request("POST", "/orgs", { form: { name: "Acme", slug: "acme" } });
    expect(none.status).toBe(403);
    const wrong = await ada.request("POST", "/orgs", { form: { csrf: "not-the-token", name: "Acme", slug: "acme" } });
    expect(wrong.status).toBe(403);

    // Another person's token doesn't work for this session.
    const grace = await as(w, "grace");
    const theirs = await ada.request("POST", "/orgs", { form: { csrf: grace.csrf, name: "Acme", slug: "acme" } });
    expect(theirs.status).toBe(403);

    expect(await w.db.get("SELECT 1 AS x FROM orgs")).toBeNull();

    const ok = await ada.post("/orgs", { name: "Acme", slug: "acme" });
    expect(ok.status).toBe(303);
    expect(ok.location).toBe("/o/acme?done=created");
  });

  test("a POST from another origin is refused even with a token", async () => {
    const w = world();
    const ada = await as(w, "ada");
    await ada.get("/orgs/new");
    const r = await ada.request("POST", "/orgs", {
      form: { csrf: ada.csrf, name: "Acme", slug: "acme" },
      headers: { origin: "https://evil.example" },
    });
    expect(r.status).toBe(403);
  });

  test("org actions, sign-out and joining are all protected", async () => {
    const w = world();
    const ada = await as(w, "ada");
    const slug = await makeOrg(ada);
    for (const path of [
      `/o/${slug}/invites`,
      `/o/${slug}/code`,
      `/o/${slug}/claude`,
      `/o/${slug}/connections`,
      `/o/${slug}/leave`,
      `/o/${slug}/members/101/role`,
      "/logout",
    ]) {
      const r = await ada.request("POST", path, { form: { role: "member", mode: "own" } });
      expect([path, r.status]).toEqual([path, 403]);
    }
    expect(await w.db.get("SELECT 1 AS x FROM invites")).toBeNull();
    expect(await w.db.get("SELECT 1 AS x FROM join_codes")).toBeNull();
  });

  test("a POST while signed out is refused", async () => {
    const w = world();
    const r = await new Browser(w).request("POST", "/orgs", { form: { csrf: "x", name: "A", slug: "a" } });
    expect(r.status).toBe(403);
  });

  test("sign-out clears the session", async () => {
    const w = world();
    const ada = await as(w, "ada");
    const r = await ada.post("/logout");
    expect(r.status).toBe(303);
    expect(ada.cookies.has(SESSION_COOKIE)).toBe(false);
    expect((await ada.get("/orgs")).status).toBe(303);
  });
});

describe("headers", () => {
  test("pages are no-store with a strict CSP and no referrer", async () => {
    const w = world();
    const res = await handle(new Request(`${ORIGIN}/`), w.deps);
    expect(res.headers.get("cache-control")).toBe("no-store");
    expect(res.headers.get("content-security-policy")).toContain("default-src 'none'");
    expect(res.headers.get("referrer-policy")).toBe("no-referrer");
    const text = await res.text();
    expect(text).not.toMatch(/style="/);
    expect(text).not.toMatch(/<script/);
  });
});
