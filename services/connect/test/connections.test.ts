import { describe, expect, test } from "bun:test";

import { api, as, invite, linkDaemon, makeOrg, world } from "./helpers.ts";

const TOKENS = {
  jira: "ATATT3xFfGF0-jira-token-value",
  clickup: "pk_12345_CLICKUPTOKENVALUE",
  gitlab: "glpat-GITLABTOKENVALUE123",
  claude: "sk-ant-api03-" + "z".repeat(40),
};

async function connected() {
  const w = world();
  const ada = await as(w, "ada");
  const slug = await makeOrg(ada);
  await ada.get(`/o/${slug}`);
  const add = (fields: Record<string, string>) => ada.post(`/o/${slug}/connections`, fields);
  expect((await add({ kind: "jira", site: "https://acme.atlassian.net", account: "bot@acme.com", scope: "project = PAY", token: TOKENS.jira })).status).toBe(303);
  expect((await add({ kind: "clickup", scope: "901234", token: TOKENS.clickup })).status).toBe(303);
  expect((await add({ kind: "gitlab", site: "https://gitlab.example.com/", scope: "platform/api", token: TOKENS.gitlab, name: "API" })).status).toBe(303);
  expect((await ada.post(`/o/${slug}/claude`, { mode: "api_key", api_key: TOKENS.claude })).status).toBe(303);
  return { w, ada, slug };
}

describe("tracker connections", () => {
  test("tokens are encrypted at rest and reach daemons intact", async () => {
    const { w, ada, slug } = await connected();
    const dump = JSON.stringify(w.db.sqlite.query("SELECT * FROM connections").all());
    for (const t of Object.values(TOKENS)) expect(dump).not.toContain(t);

    const { body } = await linkDaemon(ada, slug);
    const sync = (await (await api(w, "GET", "/api/daemon/sync", undefined, body.daemon_token)).json()) as any;
    expect(sync.connections.map((c: any) => [c.kind, c.name, c.site, c.scope, c.account, c.token])).toEqual([
      ["jira", "Jira project = PAY", "https://acme.atlassian.net", "project = PAY", "bot@acme.com", TOKENS.jira],
      ["clickup", "ClickUp 901234", null, "901234", null, TOKENS.clickup],
      ["gitlab", "API", "https://gitlab.example.com", "platform/api", null, TOKENS.gitlab],
    ]);
  });

  test("no stored token ever appears in a page, for admins or members", async () => {
    const { w, ada, slug } = await connected();
    const grace = await as(w, "grace");
    await invite(ada, slug, grace);
    const pages = [
      await ada.get(`/o/${slug}`),
      await ada.get(`/o/${slug}?done=connection_added`),
      await grace.get(`/o/${slug}`),
      await ada.post(`/o/${slug}/invites`, { role: "member" }),
      // An error re-renders the whole dashboard; it mustn't echo stored secrets either.
      await ada.post(`/o/${slug}/connections`, { kind: "jira", site: "", token: "x" }),
      await ada.post(`/o/${slug}/claude`, { mode: "team", plan: "" }),
    ];
    for (const p of pages) {
      expect(p.text).toContain("Tracker connections");
      for (const t of Object.values(TOKENS)) expect(p.text).not.toContain(t);
    }
    expect(pages[0]!.text).toContain("Token stored");
    expect(pages[0]!.text).toContain("A key is stored.");
  });

  test("validation", async () => {
    const { ada, slug } = await connected();
    const add = (f: Record<string, string>) => ada.post(`/o/${slug}/connections`, f);
    expect((await add({ kind: "jira", site: "http://insecure.example", account: "a@b.c", scope: "X", token: "t" })).status).toBe(400);
    expect((await add({ kind: "jira", site: "https://a.atlassian.net", account: "not-an-email", scope: "X", token: "t" })).status).toBe(400);
    expect((await add({ kind: "clickup", scope: "9/../1", token: "t" })).status).toBe(400);
    expect((await add({ kind: "gitlab", site: "https://gitlab.com", scope: "noslash", token: "t" })).status).toBe(400);
    expect((await add({ kind: "gitlab", site: "https://gitlab.com", scope: "a/b", token: "" })).status).toBe(400);
    expect((await add({ kind: "github", scope: "a/b", token: "t" })).status).toBe(400);
  });

  test("only admins add or delete connections", async () => {
    const { w, ada, slug } = await connected();
    const grace = await as(w, "grace");
    await invite(ada, slug, grace);
    await grace.get(`/o/${slug}`);
    expect((await grace.post(`/o/${slug}/connections`, { kind: "clickup", scope: "1", token: "t" })).status).toBe(403);
    const { id } = (await w.db.get<{ id: string }>("SELECT id FROM connections LIMIT 1"))!;
    expect((await grace.post(`/o/${slug}/connections/${id}/delete`)).status).toBe(403);
    expect((await ada.post(`/o/${slug}/connections/${id}/delete`)).status).toBe(303);
    expect(await w.db.get<{ n: number }>("SELECT COUNT(*) AS n FROM connections")).toEqual({ n: 2 });
  });
});
