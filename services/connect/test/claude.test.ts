import { describe, expect, test } from "bun:test";

import { claudeEnv } from "../src/core/claude.ts";
import { api, as, invite, linkDaemon, makeOrg, world } from "./helpers.ts";

const API_KEY = "sk-ant-api03-" + "k".repeat(40);

async function setup() {
  const w = world();
  const ada = await as(w, "ada");
  const slug = await makeOrg(ada);
  const { body } = await linkDaemon(ada, slug);
  await ada.get(`/o/${slug}`);
  const claude = async () => ((await (await api(w, "GET", "/api/daemon/sync", undefined, body.daemon_token)).json()) as any).claude;
  return { w, ada, slug, claude };
}

describe("Claude modes", () => {
  test("the env each mode produces", () => {
    expect(claudeEnv("own", {}, null)).toEqual({});
    expect(claudeEnv("team", { plan: "Claude Team" }, null)).toEqual({});
    expect(claudeEnv("api_key", {}, API_KEY)).toEqual({ ANTHROPIC_API_KEY: API_KEY });
    expect(claudeEnv("bedrock", { region: "us-east-1" }, null)).toEqual({ CLAUDE_CODE_USE_BEDROCK: "1", AWS_REGION: "us-east-1" });
    expect(
      claudeEnv("bedrock", { region: "us-east-1", opus_model: "us.anthropic.claude-opus-4-8", haiku_model: "us.anthropic.claude-haiku-4-5-20251001-v1:0" }, null),
    ).toEqual({
      CLAUDE_CODE_USE_BEDROCK: "1",
      AWS_REGION: "us-east-1",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "us.anthropic.claude-opus-4-8",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "us.anthropic.claude-haiku-4-5-20251001-v1:0",
    });
    expect(claudeEnv("vertex", { region: "global", project_id: "acme-ai" }, null)).toEqual({
      CLAUDE_CODE_USE_VERTEX: "1",
      CLOUD_ML_REGION: "global",
      ANTHROPIC_VERTEX_PROJECT_ID: "acme-ai",
    });
  });

  test("own is the default", async () => {
    const { claude } = await setup();
    expect(await claude()).toEqual({ mode: "own", plan: null, env: {} });
  });

  test("team stores and shows the plan, and tells members to use their work account", async () => {
    const { ada, slug, claude } = await setup();
    expect((await ada.post(`/o/${slug}/claude`, { mode: "team", plan: "Claude Enterprise" })).status).toBe(303);
    expect(await claude()).toEqual({ mode: "team", plan: "Claude Enterprise", env: {} });
    const page = (await ada.get(`/o/${slug}`)).text;
    expect(page).toContain("<strong>Claude Enterprise</strong>");
    expect(page).toContain("work account");
    expect((await ada.post(`/o/${slug}/claude`, { mode: "team", plan: "" })).status).toBe(400);
  });

  test("api_key is delivered as ANTHROPIC_API_KEY, encrypted at rest, and kept when the field is left blank", async () => {
    const { w, ada, slug, claude } = await setup();
    expect((await ada.post(`/o/${slug}/claude`, { mode: "api_key", api_key: "" })).status).toBe(400);
    expect((await ada.post(`/o/${slug}/claude`, { mode: "api_key", api_key: "not-a-key" })).status).toBe(400);
    expect((await ada.post(`/o/${slug}/claude`, { mode: "api_key", api_key: API_KEY })).status).toBe(303);
    expect(await claude()).toEqual({ mode: "api_key", plan: null, env: { ANTHROPIC_API_KEY: API_KEY } });

    const stored = JSON.stringify(w.db.sqlite.query("SELECT * FROM orgs").all());
    expect(stored).not.toContain(API_KEY);

    expect((await ada.post(`/o/${slug}/claude`, { mode: "api_key", api_key: "" })).status).toBe(303);
    expect((await claude()).env.ANTHROPIC_API_KEY).toBe(API_KEY);

    // Leaving api_key mode deletes the key.
    await ada.post(`/o/${slug}/claude`, { mode: "own" });
    expect((await w.db.get<{ claude_secret: string | null }>("SELECT claude_secret FROM orgs"))!.claude_secret).toBeNull();
    expect((await ada.post(`/o/${slug}/claude`, { mode: "api_key", api_key: "" })).status).toBe(400);
  });

  test("bedrock needs a region and stores no AWS credentials", async () => {
    const { w, ada, slug, claude } = await setup();
    expect((await ada.post(`/o/${slug}/claude`, { mode: "bedrock", aws_region: "" })).status).toBe(400);
    const r = await ada.post(`/o/${slug}/claude`, { mode: "bedrock", aws_region: "us-west-2", vertex_sonnet_model: "ignored", bedrock_sonnet_model: "us.anthropic.claude-sonnet-4-6" });
    expect(r.status).toBe(303);
    expect(await claude()).toEqual({
      mode: "bedrock",
      plan: null,
      env: { CLAUDE_CODE_USE_BEDROCK: "1", AWS_REGION: "us-west-2", ANTHROPIC_DEFAULT_SONNET_MODEL: "us.anthropic.claude-sonnet-4-6" },
    });
    expect((await w.db.get<{ claude_secret: string | null }>("SELECT claude_secret FROM orgs"))!.claude_secret).toBeNull();
  });

  test("vertex needs a region and a project", async () => {
    const { ada, slug, claude } = await setup();
    expect((await ada.post(`/o/${slug}/claude`, { mode: "vertex", vertex_region: "us-east5", vertex_project: "" })).status).toBe(400);
    expect((await ada.post(`/o/${slug}/claude`, { mode: "vertex", vertex_region: "us-east5", vertex_project: "acme-ai-prod" })).status).toBe(303);
    expect(await claude()).toEqual({
      mode: "vertex",
      plan: null,
      env: { CLAUDE_CODE_USE_VERTEX: "1", CLOUD_ML_REGION: "us-east5", ANTHROPIC_VERTEX_PROJECT_ID: "acme-ai-prod" },
    });
  });

  test("there's no shared-subscription mode, and the page says why", async () => {
    const { ada, slug } = await setup();
    expect((await ada.post(`/o/${slug}/claude`, { mode: "shared" })).status).toBe(400);
    const page = (await ada.get(`/o/${slug}`)).text;
    expect(page).toContain("Why there's no shared-subscription option.");
    expect(page).toContain("You may not share your Account login information");
  });

  test("only admins change Claude access", async () => {
    const { w, ada, slug } = await setup();
    const grace = await as(w, "grace");
    await invite(ada, slug, grace);
    await grace.get(`/o/${slug}`);
    expect((await grace.post(`/o/${slug}/claude`, { mode: "team", plan: "Claude Team" })).status).toBe(403);
    expect((await grace.get(`/o/${slug}`)).text).not.toContain('action="/o/acme/claude"');
  });
});
