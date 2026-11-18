import { describe, expect, test } from "bun:test";

import { sha256Hex } from "../src/core/crypto.ts";
import { CODE_TTL_MS, displayCode, newCode, normalizeCode } from "../src/core/daemons.ts";
import { api, as, codeFrom, invite, linkDaemon, makeOrg, world, type World } from "./helpers.ts";

async function team(w: World, n = 3) {
  const owner = await as(w, "ada");
  const slug = await makeOrg(owner);
  const people = [owner];
  for (const login of ["grace", "linus", "margaret", "ken"].slice(0, n - 1)) {
    const b = await as(w, login);
    await invite(owner, slug, b);
    people.push(b);
  }
  return { slug, people, owner };
}

const counts = (n: number) => ({
  workspaces_started: n,
  prs_opened: n,
  prs_merged: n,
  checks_passed: n,
  checks_failed: 0,
  agent_turns: n * 10,
  cost_usd: n * 1.5,
});

describe("one-time codes", () => {
  test("are 10 characters of Crockford base32, read case-insensitively", () => {
    const c = newCode();
    expect(c).toMatch(/^[0-9A-HJKMNP-TV-Z]{10}$/);
    expect(normalizeCode(c.toLowerCase())).toBe(c);
    expect(normalizeCode(displayCode(c).toLowerCase())).toBe(c);
    expect(normalizeCode("ab1o-il 0xz")).toBe("AB10110XZ");
  });

  test("exchange once, in any case, and only within 15 minutes", async () => {
    const w = world();
    const { slug, owner } = await team(w, 1);
    await owner.get(`/o/${slug}`);
    const page = await owner.post(`/o/${slug}/code`);
    expect(page.status).toBe(200);
    expect(page.res.headers.get("cache-control")).toBe("no-store");
    const code = codeFrom(page.text);
    expect(page.text).toContain(displayCode(code));

    // Only the hash is stored.
    const row = await w.db.get<{ code_hash: string; expires_at: number; created_at: number }>("SELECT * FROM join_codes");
    expect(row!.code_hash).toBe(await sha256Hex(code));
    expect(row!.expires_at - row!.created_at).toBe(CODE_TTL_MS);

    const lower = displayCode(code).toLowerCase();
    const first = await api(w, "POST", "/api/daemon/join", { code: lower, name: "ada's laptop" });
    expect(first.status).toBe(200);
    const second = await api(w, "POST", "/api/daemon/join", { code, name: "again" });
    expect(second.status).toBe(410);
    expect(await w.db.get<{ n: number }>("SELECT COUNT(*) AS n FROM daemons")).toEqual({ n: 1 });
  });

  test("expire after 15 minutes", async () => {
    const w = world();
    const { slug, owner } = await team(w, 1);
    await owner.get(`/o/${slug}`);
    const code = codeFrom((await owner.post(`/o/${slug}/code`)).text);
    w.clock.now += CODE_TTL_MS;
    const r = await api(w, "POST", "/api/daemon/join", { code, name: "late" });
    expect(r.status).toBe(410);
  });

  test("a code that never existed is a 400, as is a malformed body", async () => {
    const w = world();
    expect((await api(w, "POST", "/api/daemon/join", { code: "ZZZZZZZZZZ", name: "x" })).status).toBe(400);
    expect((await api(w, "POST", "/api/daemon/join", { code: "short", name: "x" })).status).toBe(400);
    expect((await api(w, "POST", "/api/daemon/join", { name: "x" })).status).toBe(400);
    const bad = await api(w, "POST", "/api/daemon/join");
    expect(bad.status).toBe(400);
    expect(await bad.json()).toEqual({ error: "body must be JSON" });
  });

  test("a new code voids the member's earlier unused one", async () => {
    const w = world();
    const { slug, owner } = await team(w, 1);
    await owner.get(`/o/${slug}`);
    const a = codeFrom((await owner.post(`/o/${slug}/code`)).text);
    const b = codeFrom((await owner.post(`/o/${slug}/code`)).text);
    expect((await api(w, "POST", "/api/daemon/join", { code: a, name: "x" })).status).toBe(410);
    expect((await api(w, "POST", "/api/daemon/join", { code: b, name: "x" })).status).toBe(200);
  });

  test("opening the join link in a browser doesn't use it", async () => {
    const w = world();
    const { slug, owner } = await team(w, 1);
    await owner.get(`/o/${slug}`);
    const code = codeFrom((await owner.post(`/o/${slug}/code`)).text);
    const page = await owner.get(`/c/${code}`);
    expect(page.status).toBe(200);
    expect(page.text).toContain("This link is for Bantu");
    expect((await api(w, "POST", "/api/daemon/join", { code, name: "x" })).status).toBe(200);
  });
});

describe("daemon API", () => {
  test("join returns a 32-byte token, stored only as its SHA-256", async () => {
    const w = world();
    const { slug, owner } = await team(w, 1);
    const { res, body } = await linkDaemon(owner, slug, "ada-mbp");
    expect(res.status).toBe(200);
    expect(Object.keys(body).sort()).toEqual(["daemon_token", "member", "org"]);
    expect(body.org).toEqual({ id: expect.any(String), name: "Acme", slug: "acme" });
    expect(body.member).toEqual({ login: "ada", role: "owner" });
    const token: string = body.daemon_token;
    expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(Buffer.from(token, "base64url")).toHaveLength(32);

    const row = await w.db.get<{ token_hash: string; name: string }>("SELECT token_hash, name FROM daemons");
    expect(row!.token_hash).toBe(await sha256Hex(token));
    expect(row!.name).toBe("ada-mbp");
    const dump = JSON.stringify(w.db.sqlite.query("SELECT * FROM daemons").all());
    expect(dump).not.toContain(token);
  });

  test("sync returns the org, member, connections and Claude settings, and records the time", async () => {
    const w = world();
    const { slug, owner } = await team(w, 1);
    await owner.get(`/o/${slug}`);
    await owner.post(`/o/${slug}/connections`, {
      kind: "jira",
      name: "Payments",
      site: "acme.atlassian.net",
      account: "bot@acme.com",
      scope: "PAY",
      token: "jira-secret-token-123",
    });
    await owner.post(`/o/${slug}/claude`, { mode: "bedrock", aws_region: "eu-west-1" });
    const { body } = await linkDaemon(owner, slug);

    w.clock.now += 60_000;
    const res = await api(w, "GET", "/api/daemon/sync", undefined, body.daemon_token);
    expect(res.status).toBe(200);
    const s = (await res.json()) as any;
    expect(s.org).toEqual(body.org);
    expect(s.member).toEqual({ login: "ada", role: "owner" });
    expect(s.connections).toEqual([
      {
        id: expect.any(String),
        kind: "jira",
        name: "Payments",
        site: "https://acme.atlassian.net",
        scope: "PAY",
        account: "bot@acme.com",
        token: "jira-secret-token-123",
      },
    ]);
    expect(s.claude).toEqual({ mode: "bedrock", plan: null, env: { CLAUDE_CODE_USE_BEDROCK: "1", AWS_REGION: "eu-west-1" } });
    expect(s.synced_at).toBe(new Date(w.clock.now).toISOString());
    const seen = await w.db.get<{ last_seen_at: number }>("SELECT last_seen_at FROM daemons");
    expect(seen!.last_seen_at).toBe(w.clock.now);
    expect(res.headers.get("cache-control")).toBe("no-store");
  });

  test("sync needs a valid bearer token", async () => {
    const w = world();
    for (const token of [undefined, "garbage", "A".repeat(43)]) {
      const r = await api(w, "GET", "/api/daemon/sync", undefined, token);
      expect(r.status).toBe(401);
      expect(r.headers.get("www-authenticate")).toContain("Bearer");
    }
  });

  test("401 once the token is revoked from the org page", async () => {
    const w = world();
    const { slug, owner } = await team(w, 1);
    const { body } = await linkDaemon(owner, slug);
    const { id } = (await w.db.get<{ id: string }>("SELECT id FROM daemons"))!;
    await owner.get(`/o/${slug}`);
    expect((await owner.post(`/o/${slug}/daemons/${id}/revoke`)).status).toBe(303);
    expect((await api(w, "GET", "/api/daemon/sync", undefined, body.daemon_token)).status).toBe(401);
  });

  test("401 after the member is removed, and their codes stop working", async () => {
    const w = world();
    const { slug, owner, people } = await team(w, 2);
    const grace = people[1]!;
    const { body } = await linkDaemon(grace, slug);
    expect((await api(w, "GET", "/api/daemon/sync", undefined, body.daemon_token)).status).toBe(200);
    await grace.get(`/o/${slug}`);
    const unused = codeFrom((await grace.post(`/o/${slug}/code`)).text);

    await owner.get(`/o/${slug}`);
    expect((await owner.post(`/o/${slug}/members/102/remove`)).status).toBe(303);
    expect((await api(w, "GET", "/api/daemon/sync", undefined, body.daemon_token)).status).toBe(401);
    expect((await api(w, "POST", "/api/daemon/totals", { day: "2026-11-18", counts: counts(1) }, body.daemon_token)).status).toBe(401);
    expect((await api(w, "POST", "/api/daemon/join", { code: unused, name: "x" })).status).toBe(410);

    // Rejoining doesn't bring the old token back.
    await invite(owner, slug, grace);
    expect((await api(w, "GET", "/api/daemon/sync", undefined, body.daemon_token)).status).toBe(401);
  });

  test("a member can revoke only their own Bantu; admins can revoke any", async () => {
    const w = world();
    const { slug, owner, people } = await team(w, 3);
    const [grace, linus] = [people[1]!, people[2]!];
    await linkDaemon(owner, slug, "ada-box");
    await linkDaemon(linus, slug, "linus-box");
    const ids = await w.db.all<{ id: string; name: string }>("SELECT id, name FROM daemons ORDER BY name");
    await grace.get(`/o/${slug}`);
    expect((await grace.post(`/o/${slug}/daemons/${ids[0]!.id}/revoke`)).status).toBe(403);
    // Members see only their own Bantu in the list.
    const page = await grace.get(`/o/${slug}`);
    expect(page.text).not.toContain("ada-box");
    expect((await owner.get(`/o/${slug}`)).text).toContain("linus-box");
    expect((await owner.post(`/o/${slug}/daemons/${ids[1]!.id}/revoke`)).status).toBe(303);
  });

  test("leave revokes the token", async () => {
    const w = world();
    const { slug, owner } = await team(w, 1);
    const { body } = await linkDaemon(owner, slug);
    const r = await api(w, "POST", "/api/daemon/leave", {}, body.daemon_token);
    expect(r.status).toBe(204);
    expect((await api(w, "GET", "/api/daemon/sync", undefined, body.daemon_token)).status).toBe(401);
    expect((await api(w, "POST", "/api/daemon/leave", {}, body.daemon_token)).status).toBe(401);
  });

  test("wrong methods and unknown paths", async () => {
    const w = world();
    expect((await api(w, "GET", "/api/daemon/join")).status).toBe(405);
    expect((await api(w, "POST", "/api/daemon/sync")).status).toBe(405);
    expect((await api(w, "GET", "/api/daemon/nope")).status).toBe(404);
  });
});

describe("totals", () => {
  test("204, and a resend for the same day replaces rather than adds", async () => {
    const w = world();
    const { slug, people } = await team(w, 3);
    const tokens: string[] = [];
    for (const p of people) tokens.push((await linkDaemon(p, slug)).body.daemon_token);

    const day = "2026-11-18";
    expect((await api(w, "POST", "/api/daemon/totals", { day, counts: counts(1) }, tokens[0])).status).toBe(204);
    expect((await api(w, "POST", "/api/daemon/totals", { day, counts: counts(1) }, tokens[0])).status).toBe(204);
    expect((await api(w, "POST", "/api/daemon/totals", { day, counts: counts(4) }, tokens[0])).status).toBe(204);
    expect(await w.db.get("SELECT COUNT(*) AS n FROM daily_totals")).toEqual({ n: 1 });
    await api(w, "POST", "/api/daemon/totals", { day, counts: counts(2) }, tokens[1]);
    await api(w, "POST", "/api/daemon/totals", { day: "2026-11-15", counts: counts(3) }, tokens[2]);

    const page = (await people[0]!.get(`/o/${slug}`)).text;
    // 4 + 2 + 3 workspaces; turns 40 + 20 + 30; cost 6 + 3 + 4.5.
    expect(page).toContain("<dt>Workspaces started</dt><dd>9</dd>");
    expect(page).toContain("<dt>Agent turns</dt><dd>90</dd>");
    expect(page).toContain("<dt>Cost</dt><dd>$13.50</dd>");
  });

  test("bad days and counts are refused", async () => {
    const w = world();
    const { slug, owner } = await team(w, 1);
    const t = (await linkDaemon(owner, slug)).body.daemon_token;
    for (const body of [
      { day: "2026-13-01", counts: counts(1) },
      { day: "2026-02-30", counts: counts(1) },
      { day: "18/11/2026", counts: counts(1) },
      { day: "2027-01-01", counts: counts(1) },
      { day: "2026-11-18", counts: { ...counts(1), prs_opened: -1 } },
      { day: "2026-11-18", counts: { ...counts(1), agent_turns: 1.5 } },
      { day: "2026-11-18", counts: { ...counts(1), cost_usd: "3" } },
      { day: "2026-11-18", counts: [] },
      { day: "2026-11-18" },
    ]) {
      expect([body, (await api(w, "POST", "/api/daemon/totals", body, t)).status]).toEqual([body, 400]);
    }
    expect(await w.db.get("SELECT COUNT(*) AS n FROM daily_totals")).toEqual({ n: 0 });
  });

  test("totals are stored per daemon and day, with no person on the row", async () => {
    const cols = world().db.sqlite.query("PRAGMA table_info(daily_totals)").all() as Array<{ name: string }>;
    expect(cols.map((c) => c.name)).not.toContain("user_id");
  });

  test("hidden until 3 members have linked a Bantu, and the page says so", async () => {
    const w = world();
    const { slug, people } = await team(w, 3);
    const day = "2026-11-18";
    // Two people, one of them with two machines: still only 2 members.
    const a1 = (await linkDaemon(people[0]!, slug, "a1")).body.daemon_token;
    const a2 = (await linkDaemon(people[0]!, slug, "a2")).body.daemon_token;
    const g = (await linkDaemon(people[1]!, slug)).body.daemon_token;
    for (const t of [a1, a2, g]) await api(w, "POST", "/api/daemon/totals", { day, counts: counts(5) }, t);

    let page = (await people[0]!.get(`/o/${slug}`)).text;
    expect(page).toContain("Hidden until 3 members have linked a Bantu.");
    expect(page).toContain("2 of 3 so far");
    expect(page).not.toContain("Workspaces started");

    const l = (await linkDaemon(people[2]!, slug)).body.daemon_token;
    // Linked, but only two people have sent counts: each period stays hidden.
    page = (await people[0]!.get(`/o/${slug}`)).text;
    expect(page).not.toContain("Hidden until");
    expect(page).toContain("fewer than 3 members sent counts");
    expect(page).not.toContain("Workspaces started");

    await api(w, "POST", "/api/daemon/totals", { day, counts: counts(1) }, l);
    page = (await people[0]!.get(`/o/${slug}`)).text;
    expect(page).toContain("<dt>Workspaces started</dt><dd>16</dd>");

    // Someone leaves: back under 3, everything is hidden again.
    expect((await people[2]!.post(`/o/${slug}/leave`)).status).toBe(303);
    page = (await people[0]!.get(`/o/${slug}`)).text;
    expect(page).toContain("Hidden until 3 members have linked a Bantu.");
    expect(page).not.toContain("Workspaces started");
  });

  test("the 7-day window leaves out older days that the 30-day window counts", async () => {
    const w = world();
    const { slug, people } = await team(w, 3);
    const tokens = [];
    for (const p of people) tokens.push((await linkDaemon(p, slug)).body.daemon_token);
    for (const t of tokens) {
      await api(w, "POST", "/api/daemon/totals", { day: "2026-11-18", counts: counts(1) }, t);
      await api(w, "POST", "/api/daemon/totals", { day: "2026-11-01", counts: counts(10) }, t);
    }
    const page = (await people[0]!.get(`/o/${slug}`)).text;
    const [seven, thirty] = page.split("<h3>Last 30 days</h3>");
    expect(seven).toContain("<dt>Workspaces started</dt><dd>3</dd>");
    expect(thirty).toContain("<dt>Workspaces started</dt><dd>33</dd>");
  });
});
