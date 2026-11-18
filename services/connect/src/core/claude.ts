// How an org's members reach Claude, and the environment each member's Bantu gives the
// `claude` CLI.
//
// There's deliberately no mode that pools or shares a personal Claude Pro or Max login.
// Anthropic's Consumer Terms (anthropic.com/legal/consumer-terms, Section 2): "You may not
// share your Account login information, Anthropic API key, or Account credentials with anyone
// else. You also may not make your Account available to anyone else."
//
// Variable names, checked against Claude Code's docs in November 2026:
// - ANTHROPIC_API_KEY: https://code.claude.com/docs/en/env-vars ("API key sent as X-Api-Key
//   header. When set, this key is used instead of your Claude Pro, Max, Ultra, Team, or
//   Enterprise subscription even if you are logged in.")
// - CLAUDE_CODE_USE_BEDROCK=1 and AWS_REGION: https://code.claude.com/docs/en/amazon-bedrock
//   ("3. Configure Claude Code"). Credentials come from the AWS SDK's default chain on the
//   member's machine, so none are stored here.
// - CLAUDE_CODE_USE_VERTEX=1, CLOUD_ML_REGION and ANTHROPIC_VERTEX_PROJECT_ID:
//   https://code.claude.com/docs/en/google-vertex-ai ("4. Configure Claude Code").
// - ANTHROPIC_DEFAULT_OPUS_MODEL, ANTHROPIC_DEFAULT_SONNET_MODEL and
//   ANTHROPIC_DEFAULT_HAIKU_MODEL pin model ids on both platforms ("Pin model versions" on
//   both pages; https://code.claude.com/docs/en/model-config). ANTHROPIC_SMALL_FAST_MODEL is
//   marked deprecated in the env-vars reference, so it isn't offered.

import { decrypt, encrypt } from "./crypto.ts";
import type { Db } from "./db.ts";
import { HttpError } from "./context.ts";
import type { Org } from "./orgs.ts";

export type ClaudeMode = "own" | "team" | "api_key" | "bedrock" | "vertex";
export const CLAUDE_MODES: readonly ClaudeMode[] = ["own", "team", "api_key", "bedrock", "vertex"];

export type ClaudeConfig = {
  /** team: the plan's name, such as "Claude Team" or "Claude Enterprise". */
  plan?: string;
  /** bedrock: AWS_REGION. vertex: CLOUD_ML_REGION. */
  region?: string;
  /** vertex: ANTHROPIC_VERTEX_PROJECT_ID. */
  project_id?: string;
  /** bedrock and vertex: optional model pins. */
  opus_model?: string;
  sonnet_model?: string;
  haiku_model?: string;
};

export type ClaudeAccess = { mode: ClaudeMode; plan: string | null; env: Record<string, string> };

export const MODE_LABELS: Record<ClaudeMode, string> = {
  own: "Each member's own Claude login",
  team: "Claude Team or Enterprise seats",
  api_key: "The org's Anthropic Console API key",
  bedrock: "Amazon Bedrock",
  vertex: "Google Cloud Vertex AI",
};

function isMode(v: unknown): v is ClaudeMode {
  return typeof v === "string" && (CLAUDE_MODES as readonly string[]).includes(v);
}

export function parseConfig(org: Pick<Org, "claude_config">): ClaudeConfig {
  try {
    const v = JSON.parse(org.claude_config);
    return v && typeof v === "object" ? (v as ClaudeConfig) : {};
  } catch {
    return {};
  }
}

const REGION = /^[a-z]{2,}(?:-[a-z0-9]+)*$/;
const PROJECT = /^[a-z][a-z0-9-]{4,28}[a-z0-9]$/;
// Bedrock ids (us.anthropic.claude-…:0), ARNs and Vertex ids (claude-…@2025…).
const MODEL_ID = /^[A-Za-z0-9._:@\/\[\]-]{1,200}$/;

function text(form: Record<string, string>, key: string): string {
  return (form[key] ?? "").trim();
}

/** The form has a set of pin fields per platform: bedrock_opus_model, vertex_opus_model, … */
function models(form: Record<string, string>, platform: "bedrock" | "vertex", config: ClaudeConfig): void {
  for (const key of ["opus_model", "sonnet_model", "haiku_model"] as const) {
    const v = text(form, `${platform}_${key}`);
    if (!v) continue;
    if (!MODEL_ID.test(v)) throw new HttpError(400, `“${v}” doesn't look like a model id.`);
    config[key] = v;
  }
}

/** The env a member's Bantu sets for `claude`. `apiKey` is the decrypted key in api_key mode. */
export function claudeEnv(mode: ClaudeMode, config: ClaudeConfig, apiKey: string | null): Record<string, string> {
  const pins = (env: Record<string, string>) => {
    if (config.opus_model) env.ANTHROPIC_DEFAULT_OPUS_MODEL = config.opus_model;
    if (config.sonnet_model) env.ANTHROPIC_DEFAULT_SONNET_MODEL = config.sonnet_model;
    if (config.haiku_model) env.ANTHROPIC_DEFAULT_HAIKU_MODEL = config.haiku_model;
    return env;
  };
  switch (mode) {
    case "own":
    case "team":
      // Both use the member's own login. In team mode Bantu tells them to sign in with their
      // work account; nothing in the environment changes.
      return {};
    case "api_key":
      return apiKey ? { ANTHROPIC_API_KEY: apiKey } : {};
    case "bedrock":
      return pins({ CLAUDE_CODE_USE_BEDROCK: "1", ...(config.region ? { AWS_REGION: config.region } : {}) });
    case "vertex":
      return pins({
        CLAUDE_CODE_USE_VERTEX: "1",
        ...(config.region ? { CLOUD_ML_REGION: config.region } : {}),
        ...(config.project_id ? { ANTHROPIC_VERTEX_PROJECT_ID: config.project_id } : {}),
      });
  }
}

export async function claudeAccess(org: Org, encryptionKey: string): Promise<ClaudeAccess> {
  const mode: ClaudeMode = isMode(org.claude_mode) ? org.claude_mode : "own";
  const config = parseConfig(org);
  const apiKey = mode === "api_key" && org.claude_secret ? await decrypt(encryptionKey, org.claude_secret, `claude:${org.id}`) : null;
  return { mode, plan: mode === "team" ? (config.plan ?? null) : null, env: claudeEnv(mode, config, apiKey) };
}

/**
 * Saves the admin form. In api_key mode a blank key field keeps the stored key, so the page
 * never needs to show it. Leaving api_key mode deletes the stored key.
 */
export async function saveClaudeAccess(db: Db, org: Org, form: Record<string, string>, encryptionKey: string): Promise<void> {
  const mode = form.mode;
  if (!isMode(mode)) throw new HttpError(400, "Choose how members reach Claude.");
  const config: ClaudeConfig = {};
  let secret: string | null = null;

  if (mode === "team") {
    const plan = text(form, "plan");
    if (!plan || plan.length > 60) throw new HttpError(400, "Name the plan, such as Claude Team or Claude Enterprise.");
    config.plan = plan;
  } else if (mode === "api_key") {
    const key = text(form, "api_key");
    if (key) {
      if (!/^sk-ant-[A-Za-z0-9_-]{20,}$/.test(key)) throw new HttpError(400, "That doesn't look like an Anthropic API key (sk-ant-…).");
      secret = await encrypt(encryptionKey, key, `claude:${org.id}`);
    } else if (org.claude_mode === "api_key" && org.claude_secret) {
      secret = org.claude_secret;
    } else {
      throw new HttpError(400, "Paste the org's Anthropic Console API key.");
    }
  } else if (mode === "bedrock") {
    const region = text(form, "aws_region");
    if (!REGION.test(region)) throw new HttpError(400, "Enter the AWS region, such as us-east-1.");
    config.region = region;
    models(form, mode, config);
  } else if (mode === "vertex") {
    const region = text(form, "vertex_region");
    const project = text(form, "vertex_project");
    if (!REGION.test(region)) throw new HttpError(400, "Enter the Vertex AI region, such as global or us-east5.");
    if (!PROJECT.test(project)) throw new HttpError(400, "Enter the Google Cloud project id.");
    config.region = region;
    config.project_id = project;
    models(form, mode, config);
  }

  await db.run(
    "UPDATE orgs SET claude_mode = ?, claude_config = ?, claude_secret = ? WHERE id = ?",
    mode,
    JSON.stringify(config),
    secret,
    org.id,
  );
}
