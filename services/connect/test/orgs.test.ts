import { describe, expect, test } from "bun:test";

import { INVITE_TTL_MS, removalError, roleChangeError } from "../src/core/orgs.ts";
import { as, Browser, invite, makeOrg, world } from "./helpers.ts";

const role = async (w: ReturnType<typeof world>, uid: number) =>
  (await w.db.get<{ role: string }>("SELECT role FROM members WHERE user_id = ?", uid))?.role ?? null;

describe("orgs", () => {
  test("the creator is the owner, and slugs are unique and validated", async () => {
    const w = world();
    const ada = await as(w, "ada");
    const slug = await makeOrg(ada, "Acme Inc", "acme");
    expect(await role(w, 101)).toBe("owner");
    const page = await ada.get(`/o/${slug}`);
    expect(page.status).toBe(200);
    expect(page.text).toContain("Acme Inc");

    await ada.get("/orgs/new");
    expect((await ada.post("/orgs", { name: "Again", slug: "acme" })).status).toBe(409);
    expect((await ada.post("/orgs", { name: "Bad", slug: "Not Valid!" })).status).toBe(400);
    expect((await ada.post("/orgs", { name: "", slug: "fine" })).status).toBe(400);
  });

  test("someone outside an org gets a 404 for it", async () => {
    const w = world();
    const slug = await makeOrg(await as(w, "ada"));
    const grace = await as(w, "grace");
    expect((await grace.get(`/o/${slug}`)).status).toBe(404);
    expect((await grace.get("/o/nope")).status).toBe(404);
  });
});

describe("invites", () => {
  test("a member accepts at /join/<invite> after signing in", async () => {
    const w = world();
    const ada = await as(w, "ada");
    const slug = await makeOrg(ada);
    await ada.get(`/o/${slug}`);
    const made = await ada.post(`/o/${slug}/invites`, { role: "member" });
    const path = "/join/" + made.text.match(/\/join\/([A-Za-z0-9_-]+)/)![1];

    // Signed out: the page offers sign-in and comes back here afterwards.
    const anon = await new Browser(w).get(path);
    expect(anon.status).toBe(200);
    expect(anon.text).toContain(`/auth/github?next=${encodeURIComponent(path)}`);

    const grace = await as(w, "grace");
    const page = await grace.get(path);
    expect(page.text).toContain("Join Acme");
    const accepted = await grace.post(path);
    expect(accepted.status).toBe(200);
    expect(accepted.text).toContain("Connect your Bantu");
    expect(await role(w, 102)).toBe("member");
  });

  test("an invite works once", async () => {
    const w = world();
    const ada = await as(w, "ada");
    const slug = await makeOrg(ada);
    const { path } = await invite(ada, slug, await as(w, "grace"));
    const linus = await as(w, "linus");
    const again = await linus.get(path);
    expect(again.status).toBe(410);
    expect((await linus.post(path)).status).toBe(410);
    expect(await role(w, 103)).toBeNull();
  });

  test("invites expire after 7 days", async () => {
    const w = world();
    const ada = await as(w, "ada");
    const slug = await makeOrg(ada);
    await ada.get(`/o/${slug}`);
    const made = await ada.post(`/o/${slug}/invites`, { role: "member" });
    const path = "/join/" + made.text.match(/\/join\/([A-Za-z0-9_-]+)/)![1];
    const row = await w.db.get<{ created_at: number; expires_at: number }>("SELECT created_at, expires_at FROM invites");
    expect(row!.expires_at - row!.created_at).toBe(INVITE_TTL_MS);

    w.clock.now += INVITE_TTL_MS - 1000;
    const grace = await as(w, "grace");
    expect((await grace.get(path)).status).toBe(200);
    w.clock.now += 2000;
    expect((await grace.get(path)).status).toBe(410);
    expect((await grace.post(path)).status).toBe(410);
    expect(await role(w, 102)).toBeNull();
    // Expired invites drop off the admin's list.
    expect((await ada.get(`/o/${slug}`)).text).toContain("No open invites.");
  });

  test("admins can revoke an invite", async () => {
    const w = world();
    const ada = await as(w, "ada");
    const slug = await makeOrg(ada);
    await ada.get(`/o/${slug}`);
    const made = await ada.post(`/o/${slug}/invites`, { role: "member" });
    const path = "/join/" + made.text.match(/\/join\/([A-Za-z0-9_-]+)/)![1];
    const { id } = (await w.db.get<{ id: string }>("SELECT id FROM invites"))!;
    expect((await ada.post(`/o/${slug}/invites/${id}/revoke`)).status).toBe(303);
    const grace = await as(w, "grace");
    const page = await grace.get(path);
    expect(page.status).toBe(410);
    expect(page.text).toContain("revoked");
    expect((await grace.post(path)).status).toBe(410);
    expect(await role(w, 102)).toBeNull();
  });

  test("members can't invite or revoke invites", async () => {
    const w = world();
    const ada = await as(w, "ada");
    const slug = await makeOrg(ada);
    const grace = await as(w, "grace");
    await invite(ada, slug, grace);
    await grace.get(`/o/${slug}`);
    expect((await grace.post(`/o/${slug}/invites`, { role: "member" })).status).toBe(403);
    await ada.post(`/o/${slug}/invites`, { role: "member" });
    const { id } = (await w.db.get<{ id: string }>("SELECT id FROM invites WHERE accepted_at IS NULL"))!;
    expect((await grace.post(`/o/${slug}/invites/${id}/revoke`)).status).toBe(403);
    expect((await w.db.get<{ revoked_at: number | null }>("SELECT revoked_at FROM invites WHERE id = ?", id))!.revoked_at).toBeNull();
  });

  test("an unknown invite is a 404", async () => {
    const w = world();
    expect((await (await as(w, "ada")).get("/join/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")).status).toBe(404);
  });
});

describe("role rules", () => {
  test("the rules themselves", () => {
    // Admins manage members and admins but never owners.
    expect(roleChangeError("admin", "member", "admin", 1)).toBeNull();
    expect(roleChangeError("admin", "admin", "member", 1)).toBeNull();
    expect(roleChangeError("admin", "member", "owner", 1)).not.toBeNull();
    expect(roleChangeError("admin", "owner", "member", 2)).not.toBeNull();
    expect(roleChangeError("member", "member", "admin", 1)).not.toBeNull();
    // Owners can make owners, and step down only when another owner remains.
    expect(roleChangeError("owner", "member", "owner", 1)).toBeNull();
    expect(roleChangeError("owner", "owner", "admin", 1)).not.toBeNull();
    expect(roleChangeError("owner", "owner", "admin", 2)).toBeNull();

    expect(removalError("admin", "member", 1, false)).toBeNull();
    expect(removalError("admin", "admin", 1, false)).toBeNull();
    expect(removalError("admin", "owner", 2, false)).not.toBeNull();
    expect(removalError("member", "member", 1, false)).not.toBeNull();
    expect(removalError("owner", "owner", 2, false)).toBeNull();
    // Leaving.
    expect(removalError("member", "member", 1, true)).toBeNull();
    expect(removalError("owner", "owner", 1, true)).toMatch(/last owner/);
    expect(removalError("owner", "owner", 2, true)).toBeNull();
  });

  test("through the pages", async () => {
    const w = world();
    const ada = await as(w, "ada");
    const slug = await makeOrg(ada);
    const grace = await as(w, "grace");
    const linus = await as(w, "linus");
    await invite(ada, slug, grace, "admin");
    await invite(ada, slug, linus);
    await grace.get(`/o/${slug}`);
    await linus.get(`/o/${slug}`);

    // A member can't change roles or remove anyone.
    expect((await linus.post(`/o/${slug}/members/102/role`, { role: "member" })).status).toBe(403);
    expect((await linus.post(`/o/${slug}/members/102/remove`)).status).toBe(403);

    // An admin can promote a member to admin, but not to owner, and can't touch the owner.
    expect((await grace.post(`/o/${slug}/members/103/role`, { role: "owner" })).status).toBe(403);
    expect((await grace.post(`/o/${slug}/members/101/role`, { role: "member" })).status).toBe(403);
    expect((await grace.post(`/o/${slug}/members/101/remove`)).status).toBe(403);
    expect((await grace.post(`/o/${slug}/members/103/role`, { role: "admin" })).status).toBe(303);
    expect(await role(w, 103)).toBe("admin");

    // The last owner can't leave, step down or be removed.
    const leave = await ada.post(`/o/${slug}/leave`);
    expect(leave.status).toBe(403);
    expect(leave.text).toContain("last owner");
    expect((await ada.post(`/o/${slug}/members/101/role`, { role: "admin" })).status).toBe(403);
    expect(await role(w, 101)).toBe("owner");

    // With a second owner, the first can leave.
    expect((await ada.post(`/o/${slug}/members/102/role`, { role: "owner" })).status).toBe(303);
    expect((await ada.post(`/o/${slug}/leave`)).status).toBe(303);
    expect(await role(w, 101)).toBeNull();

    // An admin removes a member; that member then can't see the org.
    expect((await linus.post(`/o/${slug}/members/103/role`, { role: "member" })).status).toBe(303);
    expect((await grace.post(`/o/${slug}/members/103/remove`)).status).toBe(303);
    expect((await linus.get(`/o/${slug}`)).status).toBe(404);

    // Members can leave on their own.
    const margaret = await as(w, "margaret");
    await invite(grace, slug, margaret);
    await margaret.get(`/o/${slug}`);
    expect((await margaret.post(`/o/${slug}/leave`)).status).toBe(303);
    expect(await role(w, 104)).toBeNull();
  });
});
