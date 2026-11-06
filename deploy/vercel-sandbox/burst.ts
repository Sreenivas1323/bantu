#!/usr/bin/env bun
// Burst runner: run many coding agents at once in Firecracker microVMs in your
// own Vercel account, or in Cloudflare Containers behind your own Worker, and
// bring their work back as local branches.
//
//   bun run burst.ts --task "Add a health check" --task "Fix the login redirect" --yes
//   bun run burst.ts --provider cloudflare --task "Add a health check" --yes
//
// Each task gets a throwaway VM with an egress allowlist. The model API key is
// added on the way out (by Vercel's firewall, or by the Worker), so it never
// enters the VM, and work comes back as a git bundle, so the VM needs no push
// access. Nothing is spent without --yes. See docs/VERCEL.md and
// docs/CLOUDFLARE-BURST.md.

import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { Writable } from "node:stream";

/** Vercel's Pro price per vCPU-hour, for the estimate. Hobby includes 5 free hours. */
const USD_PER_VCPU_HOUR = 0.13;
const BUNDLE = ".git/bantu-work.bundle";
const PROMPT = ".git/bantu-prompt.txt";
const WORK_REF = "bantu-burst-work";
/** The firewall replaces this, so the VM never holds the real key. */
const PLACEHOLDER_KEY = "injected-by-the-sandbox-firewall";
/**
 * On Cloudflare the key is a Worker secret, so the script never has it. This stands in for it
 * in the egress policy, whose header transforms the Cloudflare client doesn't send anyway.
 */
export const WORKER_HELD_KEY = "held-by-the-worker";

export type Provider = "vercel" | "cloudflare";

/**
 * Cloudflare Containers list prices on Workers Paid ($5/month). CPU is billed while active;
 * memory and disk while the container runs. The plan includes 375 vCPU-minutes, 25 GiB-hours
 * of memory and 200 GB-hours of disk each month.
 */
export const CLOUDFLARE = {
  usdPerVcpuSecond: 0.00002,
  usdPerGibSecond: 0.0000025,
  usdPerGbSecond: 0.00000007,
  includedVcpuSeconds: 375 * 60,
  includedGibSeconds: 25 * 3600,
  includedGbSeconds: 200 * 3600,
  /** What the Worker's INSTANCE_TYPE defaults to. */
  instance: { name: "standard-2", vcpus: 1, memoryGib: 6, diskGb: 12 },
};

export type RunResult = { exitCode: number; stdout: string; stderr: string };

/** The little this runner needs from a VM, so tests can run it on this machine. */
export interface Box {
  run(script: string, options?: { sudo?: boolean; onOutput?: (chunk: string) => void }): Promise<RunResult>;
  write(path: string, content: Buffer): Promise<void>;
  read(path: string): Promise<Buffer | null>;
  stop(): Promise<void>;
}

export type BoxRequest = {
  url: string;
  revision: string;
  token: string | undefined;
  vcpus: number;
  timeoutMs: number;
  policy: EgressPolicy;
  env: Record<string, string>;
};

export type EgressPolicy = {
  allow: Record<string, { transform: { headers: Record<string, string> }[] }[]>;
};

export type Task = { index: number; prompt: string };

export type Options = {
  repo: string;
  base: string;
  tasks: Task[];
  vcpus: number;
  minutes: number;
  concurrency: number;
  setup: string | undefined;
  model: string | undefined;
  allow: string[];
  /** Replaces the Claude Code command, for tests. */
  agentCommand?: string;
};

export type Outcome =
  /** `problem` says why the agent stopped early, when it did; its commits still count. */
  | { task: Task; status: "changed"; branch: string; commits: number; problem?: string }
  | { task: Task; status: "unchanged" }
  | { task: Task; status: "failed"; error: string };

/**
 * Where a VM may connect. Only TLS traffic is matched by name, so this limits
 * ordinary tools rather than a determined attacker, which is why the key stays
 * outside the VM either way.
 */
export function egressPolicy(apiKey: string, extra: string[]): EgressPolicy {
  const allow: EgressPolicy["allow"] = {
    "api.anthropic.com": [{ transform: [{ headers: { "x-api-key": apiKey } }] }],
    "github.com": [],
    "*.github.com": [],
    "*.githubusercontent.com": [],
    "registry.npmjs.org": [],
    "*.npmjs.org": [],
  };
  for (const domain of extra) allow[domain] ??= [];
  return { allow };
}

/** A clone URL that needs no SSH key in the VM: `git@github.com:o/r.git` → https. */
export function httpsUrl(remote: string): string {
  const ssh = remote.match(/^(?:ssh:\/\/)?git@([^:/]+)[:/](.+)$/);
  return ssh ? `https://${ssh[1]}/${ssh[2]}` : remote;
}

/** A short branch name from a task: "Fix the login!" → `fix-the-login`. */
export function slug(text: string): string {
  const words = text
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, "")
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 6);
  return words.join("-").slice(0, 40).replace(/-+$/, "") || "task";
}

export function estimate(o: Pick<Options, "tasks" | "vcpus" | "minutes" | "concurrency">, provider: Provider = "vercel"): string {
  if (provider === "cloudflare") return cloudflareEstimate(o);
  const vcpuHours = (o.tasks.length * o.vcpus * o.minutes) / 60;
  const cost = (vcpuHours * USD_PER_VCPU_HOUR).toFixed(2);
  return `${o.tasks.length} VM(s) with ${o.vcpus} vCPU, at most ${o.minutes} min each and ${o.concurrency} at a time: at most ${vcpuHours.toFixed(1)} vCPU-hours (≈ $${cost} on Vercel Pro), plus model usage on your API key.`;
}

/** The most a run can cost: every container busy for its whole time limit. */
function cloudflareEstimate(o: Pick<Options, "tasks" | "minutes" | "concurrency">): string {
  const c = CLOUDFLARE;
  const seconds = o.tasks.length * o.minutes * 60;
  const vcpuS = seconds * c.instance.vcpus;
  const gibS = seconds * c.instance.memoryGib;
  const gbS = seconds * c.instance.diskGb;
  const price = (v: number, g: number, d: number) => v * c.usdPerVcpuSecond + g * c.usdPerGibSecond + d * c.usdPerGbSecond;
  const list = price(vcpuS, gibS, gbS);
  const afterAllowance = price(
    Math.max(0, vcpuS - c.includedVcpuSeconds),
    Math.max(0, gibS - c.includedGibSeconds),
    Math.max(0, gbS - c.includedGbSeconds),
  );
  const i = c.instance;
  return `${o.tasks.length} container(s) on ${i.name} (${i.vcpus} vCPU, ${i.memoryGib} GiB, ${i.diskGb} GB), at most ${o.minutes} min each and ${o.concurrency} at a time: at most ${(vcpuS / 3600).toFixed(1)} vCPU-hours (≈ $${list.toFixed(2)} at Cloudflare's list price, or $${afterAllowance.toFixed(2)} if this month's Workers Paid allowance is still unused; CPU is billed only while busy, so usually less), plus the $5/month Workers Paid plan and model usage on the Worker's API key.`;
}

function git(repo: string, args: string[]): { ok: boolean; out: string; err: string } {
  const r = spawnSync("git", ["-C", repo, ...args], { encoding: "utf8" });
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: (r.stderr ?? "").trim() };
}

/** One line per tool call, so a long run shows it's alive. */
export function progressLines(chunk: string): string[] {
  const out: string[] = [];
  for (const line of chunk.split("\n")) {
    if (!line.startsWith("{")) continue;
    try {
      const v = JSON.parse(line) as { type?: string; message?: { content?: { type?: string; name?: string }[] } };
      if (v.type !== "assistant") continue;
      for (const block of v.message?.content ?? []) {
        if (block.type === "tool_use" && block.name) out.push(block.name);
      }
    } catch {
      // A line split across chunks; progress is best effort.
    }
  }
  return out;
}

/**
 * Leaves time to collect the work: if the VM's own limit hit first, its disk
 * and everything the agent did would go with it.
 */
const COLLECT_SECONDS = 120;

function agentScript(o: Options): string {
  if (o.agentCommand) return `${o.agentCommand} < ${PROMPT}`;
  const seconds = Math.max(60, o.minutes * 60 - COLLECT_SECONDS);
  const model = o.model ? ` --model '${o.model.replaceAll("'", "'\\''")}'` : "";
  // The VM is the boundary here, so the agent doesn't stop to ask.
  return `command -v claude >/dev/null || npm install -g @anthropic-ai/claude-code >&2
timeout ${seconds} claude -p --output-format stream-json --verbose --permission-mode bypassPermissions${model} < ${PROMPT}`;
}

/** Runs one task in its own VM. Always stops the VM. */
export async function runTask(
  o: Options,
  task: Task,
  baseSha: string,
  makeBox: (request: BoxRequest) => Promise<Box>,
  request: BoxRequest,
  log: (line: string) => void,
): Promise<Outcome> {
  let box: Box | undefined;
  try {
    box = await makeBox(request);
    const must = async (script: string, what: string, onOutput?: (chunk: string) => void) => {
      const r = await box!.run(script, { onOutput });
      if (r.exitCode !== 0) throw new Error(`${what} failed (exit ${r.exitCode}): ${r.stderr.trim().slice(-500)}`);
      return r;
    };
    // Whatever credentials the clone used shouldn't stay where the agent can read them.
    await box.run("git remote remove origin");
    if (o.setup) await must(o.setup, "the setup command");

    await box.write(
      PROMPT,
      Buffer.from(
        `You're working in a throwaway copy of this repository. Commit your work to the current branch with clear messages. Nothing can be pushed from here; your commits are collected when you finish.\n\n${task.prompt}\n`,
      ),
    );
    // An agent that stops early may still have done useful work, so collect it.
    const agent = await box.run(agentScript(o), {
      onOutput: (chunk) => {
        for (const tool of progressLines(chunk)) log(tool);
      },
    });
    const problem = agent.exitCode === 0 ? undefined : `the agent failed (exit ${agent.exitCode}): ${agent.stderr.trim().slice(-500)}`;

    // Collect anything the agent didn't commit.
    await must(
      `git add -A && (git diff --cached --quiet || git -c user.name=Bantu -c user.email=bantu@localhost commit -qm 'Work left uncommitted by the agent')`,
      "committing leftovers",
    );
    const count = await must(`git rev-list --count ${baseSha}..HEAD`, "counting commits");
    const commits = Number(count.stdout.trim());
    if (!commits) {
      if (problem) throw new Error(problem);
      return { task, status: "unchanged" };
    }

    await must(`git branch -f ${WORK_REF} HEAD && git bundle create ${BUNDLE} ${WORK_REF} ^${baseSha}`, "bundling the work");
    const bundle = await box.read(BUNDLE);
    if (!bundle) throw new Error("the bundle wasn't where it should be");
    const branch = bringBack(o.repo, bundle, `bantu/burst-${slug(task.prompt)}`);
    return { task, status: "changed", branch, commits, problem };
  } catch (err) {
    return { task, status: "failed", error: err instanceof Error ? err.message : String(err) };
  } finally {
    await box?.stop().catch(() => {});
  }
}

/** Fetches the bundle into a new local branch. Nothing is checked out or pushed. */
function bringBack(repo: string, bundle: Buffer, wanted: string): string {
  const dir = mkdtempSync(join(tmpdir(), "bantu-burst-"));
  try {
    const file = join(dir, "work.bundle");
    writeFileSync(file, bundle);
    let branch = wanted;
    for (let n = 2; git(repo, ["rev-parse", "--verify", "--quiet", `refs/heads/${branch}`]).ok; n += 1) {
      branch = `${wanted}-${n}`;
    }
    const fetched = git(repo, ["fetch", "--quiet", file, `${WORK_REF}:refs/heads/${branch}`]);
    if (!fetched.ok) throw new Error(`couldn't bring the work back: ${fetched.err}`);
    return branch;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** Runs every task, at most `concurrency` at once. */
export async function runAll(
  o: Options,
  makeBox: (request: BoxRequest) => Promise<Box>,
  apiKey: string,
  log: (task: Task, line: string) => void = () => {},
): Promise<Outcome[]> {
  // The VMs start from what's on the remote, so the bundle's base must be here too.
  const fetched = git(o.repo, ["fetch", "--quiet", "origin", o.base]);
  if (!fetched.ok) throw new Error(`couldn't fetch ${o.base} from origin: ${fetched.err}`);
  const baseSha = git(o.repo, ["rev-parse", "FETCH_HEAD"]).out;
  const remote = git(o.repo, ["remote", "get-url", "origin"]);
  if (!remote.ok) throw new Error("this repository has no origin remote for the VMs to clone");

  const request: BoxRequest = {
    url: httpsUrl(remote.out),
    revision: baseSha,
    token: process.env.GITHUB_TOKEN,
    vcpus: o.vcpus,
    timeoutMs: o.minutes * 60_000,
    policy: egressPolicy(apiKey, o.allow),
    env: { ANTHROPIC_API_KEY: PLACEHOLDER_KEY, DISABLE_TELEMETRY: "1", DISABLE_AUTOUPDATER: "1" },
  };

  const results: Outcome[] = [];
  const queue = [...o.tasks];
  const worker = async () => {
    for (let task = queue.shift(); task; task = queue.shift()) {
      const current = task;
      results.push(await runTask(o, current, baseSha, makeBox, request, (line) => log(current, line)));
    }
  };
  await Promise.all(Array.from({ length: Math.min(o.concurrency, o.tasks.length) }, worker));
  return results.sort((a, b) => a.task.index - b.task.index);
}

/** A real VM in your Vercel account. Only loaded when something's actually run. */
export async function vercelBox(r: BoxRequest): Promise<Box> {
  const { Sandbox } = await import("@vercel/sandbox");
  const sandbox = await Sandbox.create({
    persistent: false,
    resources: { vcpus: r.vcpus },
    timeout: r.timeoutMs,
    networkPolicy: r.policy,
    env: r.env,
    source: {
      type: "git",
      url: r.url,
      revision: r.revision,
      ...(r.token ? { username: "x-access-token", password: r.token } : {}),
    },
  });
  const cwd = "/vercel/sandbox";
  return {
    async run(script, options) {
      let stdout = "";
      let stderr = "";
      const sink = (append: (s: string) => void) =>
        new Writable({
          write(chunk, _encoding, done) {
            append(String(chunk));
            done();
          },
        });
      const done = await sandbox.runCommand({
        cmd: "bash",
        args: ["-c", script],
        cwd,
        sudo: options?.sudo,
        stdout: sink((s) => {
          stdout += s;
          options?.onOutput?.(s);
        }),
        stderr: sink((s) => {
          stderr += s;
        }),
      });
      return { exitCode: done.exitCode, stdout, stderr };
    },
    async write(path, content) {
      await sandbox.writeFiles([{ path, content }]);
    },
    async read(path) {
      return sandbox.readFileToBuffer({ path });
    },
    async stop() {
      await sandbox.stop();
    },
  };
}

/** Where the Worker from deploy/cloudflare-sandbox is, and the bearer secret it checks. */
export type WorkerConfig = { url: string; secret: string };

export function workerFromEnv(): WorkerConfig {
  const url = process.env.BANTU_BURST_URL;
  const secret = process.env.BANTU_BURST_SECRET;
  if (!url || !secret) throw new Error("BANTU_BURST_URL and BANTU_BURST_SECRET must both be set");
  return { url, secret };
}

/** The secret goes in a header, so only send it over HTTPS, or to this machine for tests. */
function workerBase(url: string): string {
  if (!/^https:\/\//.test(url) && !/^http:\/\/(localhost|127\.0\.0\.1|\[::1\])(:\d+)?(\/|$)/.test(url)) {
    throw new Error(`BANTU_BURST_URL must be an https:// URL, not ${url}`);
  }
  return url.replace(/\/+$/, "");
}

/**
 * A container in your Cloudflare account, through the Worker in deploy/cloudflare-sandbox.
 * The Worker holds the model key and adds it on the way out, so neither this script nor the
 * container has it. Only the allowlist's hostnames are sent, not the policy's header values.
 */
export async function cloudflareBox(r: BoxRequest, worker: WorkerConfig = workerFromEnv()): Promise<Box> {
  const base = workerBase(worker.url);
  const call = (path: string, init: RequestInit = {}) =>
    fetch(`${base}${path}`, { ...init, headers: { authorization: `Bearer ${worker.secret}`, ...init.headers } });
  const failure = async (res: Response, what: string) => {
    const detail = (await res.text().catch(() => "")).trim().slice(0, 500);
    if (res.status === 401) return new Error(`${what}: the Worker rejected BANTU_BURST_SECRET (401)`);
    return new Error(`${what}: the Worker answered ${res.status}${detail ? `: ${detail}` : ""}`);
  };

  const created = await call("/sandboxes", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      url: r.url,
      revision: r.revision,
      ...(r.token ? { token: r.token } : {}),
      allow: Object.keys(r.policy.allow),
      env: r.env,
      timeoutMs: r.timeoutMs,
    }),
  });
  if (!created.ok) throw await failure(created, "couldn't start a container");
  const { id } = (await created.json()) as { id: string };
  const at = `/sandboxes/${encodeURIComponent(id)}`;
  const file = (path: string) => `${at}/files?path=${encodeURIComponent(path)}`;

  return {
    // The container runs as root, so `sudo` needs nothing.
    async run(script, options) {
      const res = await call(`${at}/run`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ script }),
      });
      if (!res.ok || !res.body) throw await failure(res, "couldn't run a command");
      let stdout = "";
      let stderr = "";
      let exitCode: number | undefined;
      const handle = (line: string) => {
        if (!line.trim()) return;
        const v = JSON.parse(line) as { stdout?: string; stderr?: string; exitCode?: number; error?: string };
        if (typeof v.stdout === "string") {
          stdout += v.stdout;
          options?.onOutput?.(v.stdout);
        }
        if (typeof v.stderr === "string") stderr += v.stderr;
        if (typeof v.exitCode === "number") exitCode = v.exitCode;
        if (typeof v.error === "string") throw new Error(`the command's output stream failed: ${v.error}`);
      };
      const decoder = new TextDecoder();
      let buffered = "";
      for await (const chunk of res.body) {
        buffered += decoder.decode(chunk, { stream: true });
        const lines = buffered.split("\n");
        buffered = lines.pop() ?? "";
        for (const line of lines) handle(line);
      }
      handle(buffered + decoder.decode());
      if (exitCode === undefined) throw new Error("the connection to the Worker ended before the command finished");
      return { exitCode, stdout, stderr };
    },
    async write(path, content) {
      const res = await call(file(path), { method: "PUT", body: new Uint8Array(content) });
      if (!res.ok) throw await failure(res, `couldn't write ${path}`);
    },
    async read(path) {
      const res = await call(file(path));
      if (res.status === 404) return null;
      if (!res.ok) throw await failure(res, `couldn't read ${path}`);
      return Buffer.from(await res.arrayBuffer());
    },
    async stop() {
      const res = await call(at, { method: "DELETE" });
      // 410: it had already stopped.
      if (!res.ok && res.status !== 410) throw await failure(res, "couldn't stop the container");
    },
  };
}

const USAGE = `Run coding agents in parallel in your own Vercel or Cloudflare account.

  bun run burst.ts --task "..." [--task "..."] [options]

  --task <text>      A task; repeat for more (required)
  --provider <name>  vercel (default) or cloudflare
  --repo <path>      The local repository (default: current directory)
  --base <branch>    Branch to start from, as it is on origin (default: main)
  --vcpus <n>        vCPUs per VM on Vercel (default: 2). On Cloudflare, the Worker's
                     INSTANCE_TYPE sets the size (default standard-2: 1 vCPU, 6 GiB).
  --minutes <n>      Time limit per VM (default: 30; Vercel Hobby allows up to 45)
  --concurrency <n>  VMs at once (default: 4)
  --setup <command>  Run before the agent, e.g. "bun install"
  --model <name>     Model for the agent
  --allow <domain>   Another domain the VM can reach; repeatable, "*.example.com" works
  --yes              Actually run. Without it, this only prints the plan and cost.

Vercel needs ANTHROPIC_API_KEY, and VERCEL_TOKEN, VERCEL_TEAM_ID and VERCEL_PROJECT_ID,
or VERCEL_OIDC_TOKEN.

Cloudflare needs BANTU_BURST_URL and BANTU_BURST_SECRET for the Worker you deployed from
deploy/cloudflare-sandbox. The API key is a secret on that Worker, so it isn't read here.

GITHUB_TOKEN is needed for private repos, and is used only to clone.`;

async function main(): Promise<number> {
  const { values } = parseArgs({
    options: {
      task: { type: "string", multiple: true },
      provider: { type: "string", default: "vercel" },
      repo: { type: "string", default: "." },
      base: { type: "string", default: "main" },
      vcpus: { type: "string" },
      minutes: { type: "string", default: "30" },
      concurrency: { type: "string", default: "4" },
      setup: { type: "string" },
      model: { type: "string" },
      allow: { type: "string", multiple: true },
      yes: { type: "boolean", default: false },
      help: { type: "boolean", default: false },
    },
  });
  if (values.help || !values.task?.length) {
    console.log(USAGE);
    return values.help ? 0 : 1;
  }
  const provider = values.provider;
  if (provider !== "vercel" && provider !== "cloudflare") {
    console.error(`--provider must be vercel or cloudflare, not ${provider}`);
    return 1;
  }
  if (provider === "cloudflare" && values.vcpus !== undefined) {
    console.log("--vcpus doesn't apply on Cloudflare; set INSTANCE_TYPE in the Worker's wrangler.jsonc instead.");
  }
  const o: Options = {
    repo: values.repo!,
    base: values.base!,
    tasks: values.task.map((prompt, index) => ({ index, prompt })),
    vcpus: provider === "cloudflare" ? CLOUDFLARE.instance.vcpus : Number(values.vcpus ?? "2"),
    minutes: Number(values.minutes),
    concurrency: Number(values.concurrency),
    setup: values.setup,
    model: values.model,
    allow: values.allow ?? [],
  };
  console.log(estimate(o, provider));
  if (!values.yes) {
    console.log("Nothing was started. Add --yes to run this.");
    return 0;
  }
  let makeBox: (request: BoxRequest) => Promise<Box>;
  let apiKey: string;
  if (provider === "cloudflare") {
    let worker: WorkerConfig;
    try {
      worker = workerFromEnv();
      workerBase(worker.url);
    } catch (err) {
      console.error(`${err instanceof Error ? err.message : String(err)}. Deploy deploy/cloudflare-sandbox first; see docs/CLOUDFLARE-BURST.md.`);
      return 1;
    }
    makeBox = (request) => cloudflareBox(request, worker);
    apiKey = WORKER_HELD_KEY;
  } else {
    const key = process.env.ANTHROPIC_API_KEY;
    if (!key) {
      console.error("ANTHROPIC_API_KEY isn't set. A Claude subscription can't sign in inside a VM, so this needs an API key.");
      return 1;
    }
    if (!process.env.VERCEL_OIDC_TOKEN && !(process.env.VERCEL_TOKEN && process.env.VERCEL_TEAM_ID && process.env.VERCEL_PROJECT_ID)) {
      console.error("Set VERCEL_TOKEN, VERCEL_TEAM_ID and VERCEL_PROJECT_ID, or VERCEL_OIDC_TOKEN (vercel env pull).");
      return 1;
    }
    makeBox = vercelBox;
    apiKey = key;
  }
  const outcomes = await runAll(o, makeBox, apiKey, (task, line) => console.log(`[${task.index + 1}] ${line}`));
  console.log("");
  for (const r of outcomes) {
    const name = `${r.task.index + 1}. ${r.task.prompt.split("\n")[0].slice(0, 60)}`;
    if (r.status === "changed") console.log(`${name}\n   ${r.commits} commit(s) on ${r.branch}${r.problem ? `\n   Stopped early: ${r.problem}` : ""}`);
    else if (r.status === "unchanged") console.log(`${name}\n   No changes`);
    else console.log(`${name}\n   Failed: ${r.error}`);
  }
  if (outcomes.some((r) => r.status === "changed")) {
    console.log("\nIn Bantu, use New workspace → From a branch to review them. Nothing was pushed.");
  }
  return outcomes.some((r) => r.status === "failed" || (r.status === "changed" && r.problem)) ? 1 : 0;
}

if (import.meta.main) process.exit(await main());
