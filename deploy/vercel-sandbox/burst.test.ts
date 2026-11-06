// The whole run, with real git and a local stand-in for the VM. Only the Vercel
// and Cloudflare APIs themselves aren't exercised here. For Cloudflare, the
// stand-in is the real Worker router from deploy/cloudflare-sandbox on a local
// port, with local folders in place of containers.

import { afterEach, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { CLONE_SCRIPT, type CreateInit, execStream, route, type SandboxApi, sandboxEnv } from "../cloudflare-sandbox/src/api.ts";
import {
  type Box,
  type BoxRequest,
  cloudflareBox,
  egressPolicy,
  estimate,
  httpsUrl,
  type Options,
  progressLines,
  runAll,
  slug,
  WORKER_HELD_KEY,
} from "./burst.ts";

const dirs: string[] = [];
const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const c of cleanups.splice(0)) c();
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

function sh(cwd: string, script: string) {
  const r = spawnSync("bash", ["-c", script], { cwd, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${script}: ${r.stderr}`);
  return r.stdout.trim();
}

/** A bare origin and a clone of it, like the user's checkout. */
function fixture() {
  const root = mkdtempSync(join(tmpdir(), "burst-test-"));
  dirs.push(root);
  sh(root, "git init -q --bare -b main origin.git && git clone -q origin.git repo 2>/dev/null");
  const repo = join(root, "repo");
  sh(repo, "git config user.name t && git config user.email t@t && git checkout -q -b main; echo hi > README.md && git add -A && git commit -qm init && git push -q origin main");
  return { root, repo, origin: join(root, "origin.git") };
}

const options = (repo: string, prompts: string[], agentCommand: string): Options => ({
  repo,
  base: "main",
  tasks: prompts.map((prompt, index) => ({ index, prompt })),
  vcpus: 2,
  minutes: 30,
  concurrency: 2,
  setup: undefined,
  model: undefined,
  allow: [],
  agentCommand,
});

type Created = { request: BoxRequest; stopped: boolean; scripts: string[] };

/** Clones into a temp folder and runs scripts there, like a VM would. */
function localBoxes(root: string, origin: string, created: Created[]) {
  return async (request: BoxRequest): Promise<Box> => {
    const record: Created = { request, stopped: false, scripts: [] };
    created.push(record);
    const dir = mkdtempSync(join(root, "vm-"));
    sh(dir, `git clone -q ${origin} . && git checkout -q ${request.revision}`);
    return {
      async run(script, opts) {
        record.scripts.push(script);
        const r = spawnSync("bash", ["-c", script], { cwd: dir, encoding: "utf8", env: { ...process.env, ...request.env } });
        if (r.stdout) opts?.onOutput?.(r.stdout);
        return { exitCode: r.status ?? 1, stdout: r.stdout ?? "", stderr: r.stderr ?? "" };
      },
      async write(path, content) {
        mkdirSync(join(dir, path, ".."), { recursive: true });
        writeFileSync(join(dir, path), content);
      },
      async read(path) {
        const file = join(dir, path);
        return existsSync(file) ? readFileSync(file) : null;
      },
      async stop() {
        record.stopped = true;
      },
    };
  };
}

// Reads the prompt on stdin like Claude does, and acts on it.
const FAKE_AGENT = `bash -c '
prompt=$(cat)
echo "{\\"type\\":\\"assistant\\",\\"message\\":{\\"content\\":[{\\"type\\":\\"tool_use\\",\\"name\\":\\"Bash\\"}]}}"
case "$prompt" in
  *commit*this*) echo a > committed.txt; git add -A; git -c user.name=a -c user.email=a@a commit -qm "from the agent" ;;
  *leave*draft*) echo b > draft.txt ;;
  *partial*) echo c > partial.txt; echo oops >&2; exit 4 ;;
  *crash*) echo boom >&2; exit 3 ;;
esac'`;

test("brings work back as local branches, and stops every VM", async () => {
  const { root, repo, origin } = fixture();
  const created: Created[] = [];
  const prompts = ["Please commit this change", "Just leave a draft", "Look around and do nothing", "crash now"];
  const seen: string[] = [];
  const results = await runAll(options(repo, prompts, FAKE_AGENT), localBoxes(root, origin, created), "sk-secret", (_t, line) => seen.push(line));

  expect(results.map((r) => r.status)).toEqual(["changed", "changed", "unchanged", "failed"]);
  expect(created.every((c) => c.stopped)).toBe(true);
  expect(seen).toContain("Bash");

  const first = results[0];
  if (first.status !== "changed") throw new Error("unreachable");
  expect(first.branch).toBe("bantu/burst-please-commit-this-change");
  expect(first.commits).toBe(1);
  expect(sh(repo, `git show ${first.branch}:committed.txt`)).toBe("a");

  // Work the agent left uncommitted is collected too.
  const second = results[1];
  if (second.status !== "changed") throw new Error("unreachable");
  expect(sh(repo, `git show ${second.branch}:draft.txt`)).toBe("b");

  const failed = results[3];
  if (failed.status !== "failed") throw new Error("unreachable");
  expect(failed.error).toContain("exit 3");
  expect(failed.error).toContain("boom");

  // The user's checkout and the remote are untouched.
  expect(sh(repo, "git rev-parse --abbrev-ref HEAD")).toBe("main");
  expect(existsSync(join(repo, "committed.txt"))).toBe(false);
  expect(sh(origin, "git for-each-ref --format='%(refname)' refs/heads")).toBe("refs/heads/main");
});

test("work from an agent that stopped early still comes back", async () => {
  const { root, repo, origin } = fixture();
  const results = await runAll(options(repo, ["do partial work"], FAKE_AGENT), localBoxes(root, origin, []), "k");
  const r = results[0];
  if (r.status !== "changed") throw new Error(JSON.stringify(r));
  expect(sh(repo, `git show ${r.branch}:partial.txt`)).toBe("c");
  expect(r.problem).toContain("exit 4");
  expect(r.problem).toContain("oops");
});

test("keeps the key out of the VM, and removes the clone's credentials", async () => {
  const { root, repo, origin } = fixture();
  const created: Created[] = [];
  await runAll(options(repo, ["look around"], FAKE_AGENT), localBoxes(root, origin, created), "sk-secret");
  const { request, scripts } = created[0];
  expect(JSON.stringify(request.env)).not.toContain("sk-secret");
  expect(request.env.ANTHROPIC_API_KEY).toBeTruthy();
  expect(request.policy.allow["api.anthropic.com"][0].transform[0].headers["x-api-key"]).toBe("sk-secret");
  expect(scripts[0]).toBe("git remote remove origin");
  expect(scripts.join("\n")).not.toContain("sk-secret");
});

test("a branch name that's taken gets a number", async () => {
  const { root, repo, origin } = fixture();
  sh(repo, "git branch bantu/burst-commit-this");
  const results = await runAll(options(repo, ["commit this"], FAKE_AGENT), localBoxes(root, origin, []), "k");
  const r = results[0];
  if (r.status !== "changed") throw new Error(JSON.stringify(r));
  expect(r.branch).toBe("bantu/burst-commit-this-2");
});

test("a failure to start a VM is reported, not thrown", async () => {
  const { repo } = fixture();
  const results = await runAll(
    options(repo, ["anything"], FAKE_AGENT),
    async () => {
      throw new Error("no Vercel credentials");
    },
    "k",
  );
  expect(results[0]).toMatchObject({ status: "failed", error: "no Vercel credentials" });
});

test("small helpers", () => {
  expect(httpsUrl("git@github.com:o/r.git")).toBe("https://github.com/o/r.git");
  expect(httpsUrl("ssh://git@github.com/o/r.git")).toBe("https://github.com/o/r.git");
  expect(httpsUrl("https://github.com/o/r.git")).toBe("https://github.com/o/r.git");
  expect(slug("Fix the login redirect!")).toBe("fix-the-login-redirect");
  expect(slug("修正")).toBe("task");
  expect(egressPolicy("k", ["*.pypi.org"]).allow["*.pypi.org"]).toEqual([]);
  expect(progressLines('{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit"}]}}\nnot json\n{"cut')).toEqual(["Edit"]);
  expect(estimate({ tasks: [{ index: 0, prompt: "x" }, { index: 1, prompt: "y" }], vcpus: 2, minutes: 30, concurrency: 4 })).toContain("≈ $0.26");
});

test("the Cloudflare estimate", () => {
  const tasks = (n: number) => Array.from({ length: n }, (_, index) => ({ index, prompt: "x" }));
  // 4 × 30 min on standard-2: 7200 vCPU-s, 43200 GiB-s, 86400 GB-s, all inside the allowance.
  const four = estimate({ tasks: tasks(4), vcpus: 1, minutes: 30, concurrency: 4 }, "cloudflare");
  expect(four).toContain("≈ $0.26 at Cloudflare's list price");
  expect(four).toContain("or $0.00 if");
  expect(four).toContain("standard-2");
  // 20 × 60 min: 72000 vCPU-s (49500 over), 432000 GiB-s (342000 over), 864000 GB-s (144000 over): $0.99 + $0.855 + $0.010.
  const twenty = estimate({ tasks: tasks(20), vcpus: 1, minutes: 60, concurrency: 4 }, "cloudflare");
  expect(twenty).toContain("≈ $2.58 at");
  expect(twenty).toContain("or $1.86 if");
});

type FakeContainer = { dir: string; env: Record<string, string>; alive: boolean; scripts: string[] };

/**
 * The real Worker router and auth, on a local port, with folders for containers. Clone URLs
 * must be https for the Worker, so the local origin is handed to it as one, and git maps it
 * back when the Worker's own clone script runs.
 */
function fakeWorker(root: string, origin: string, secret: string) {
  const containers = new Map<string, FakeContainer>();
  const creates: Record<string, unknown>[] = [];
  const stand_in = "https://example.invalid/origin.git";
  const sandbox = (id: string): SandboxApi => ({
    async exists() {
      return containers.get(id)?.alive ?? false;
    },
    async create(init: CreateInit) {
      const dir = mkdtempSync(join(root, "container-"));
      const r = spawnSync("bash", ["-c", CLONE_SCRIPT], {
        encoding: "utf8",
        env: {
          ...process.env,
          GIT_CONFIG_COUNT: "1",
          GIT_CONFIG_KEY_0: `url.${origin}.insteadOf`,
          GIT_CONFIG_VALUE_0: stand_in,
          BANTU_REPO_DIR: dir,
          BANTU_CLONE_URL: init.url,
          BANTU_REVISION: init.revision,
        },
      });
      if (r.status !== 0) throw new Error(r.stderr);
      containers.set(id, { dir, env: sandboxEnv(init.env), alive: true, scripts: [] });
    },
    async run(script) {
      const c = containers.get(id)!;
      c.scripts.push(script);
      const proc = Bun.spawn(["bash", "-c", script], { cwd: c.dir, env: { ...process.env, ...c.env, HOME: process.env.HOME ?? "/tmp" }, stdout: "pipe", stderr: "pipe" });
      return execStream({ stdout: proc.stdout, stderr: proc.stderr, exitCode: proc.exited, kill: () => proc.kill() });
    },
    async write(path, body) {
      await Bun.write(join(containers.get(id)!.dir, path), new Response(body));
    },
    async read(path) {
      const file = join(containers.get(id)!.dir, path);
      return existsSync(file) ? Bun.file(file).stream() : null;
    },
    async destroy() {
      const c = containers.get(id);
      if (c) c.alive = false;
    },
  });
  const server = Bun.serve({
    port: 0,
    async fetch(request) {
      let req = request;
      if (new URL(request.url).pathname === "/sandboxes" && request.method === "POST") {
        const body = (await request.json()) as Record<string, unknown>;
        creates.push(body);
        req = new Request(request.url, { method: "POST", headers: request.headers, body: JSON.stringify({ ...body, url: stand_in }) });
      }
      return route(req, { BURST_SECRET: secret }, sandbox);
    },
  });
  cleanups.push(() => server.stop(true));
  return { url: server.url.href, containers, creates };
}

test("cloudflareBox brings work back through the Worker, and stops every container", async () => {
  const { root, repo, origin } = fixture();
  const worker = fakeWorker(root, origin, "right-secret");
  const previous = process.env.GITHUB_TOKEN;
  process.env.GITHUB_TOKEN = "ghp_clone_only";
  cleanups.push(() => {
    if (previous === undefined) delete process.env.GITHUB_TOKEN;
    else process.env.GITHUB_TOKEN = previous;
  });

  const seen: string[] = [];
  const prompts = ["Please commit this change", "Just leave a draft", "Look around and do nothing", "crash now"];
  const results = await runAll(
    options(repo, prompts, FAKE_AGENT),
    (r) => cloudflareBox(r, { url: worker.url, secret: "right-secret" }),
    WORKER_HELD_KEY,
    (_t, line) => seen.push(line),
  );

  expect(results.map((r) => r.status)).toEqual(["changed", "changed", "unchanged", "failed"]);
  expect(seen).toContain("Bash");
  const first = results[0];
  if (first.status !== "changed") throw new Error(JSON.stringify(first));
  expect(sh(repo, `git show ${first.branch}:committed.txt`)).toBe("a");
  const second = results[1];
  if (second.status !== "changed") throw new Error(JSON.stringify(second));
  expect(sh(repo, `git show ${second.branch}:draft.txt`)).toBe("b");
  const failed = results[3];
  if (failed.status !== "failed") throw new Error("unreachable");
  expect(failed.error).toContain("exit 3");
  expect(failed.error).toContain("boom");

  const containers = [...worker.containers.values()];
  expect(containers).toHaveLength(4);
  expect(containers.every((c) => !c.alive)).toBe(true);
  expect(containers.every((c) => c.scripts[0] === "git remote remove origin")).toBe(true);

  // What crossed the wire: hostnames only, a placeholder key, and the clone token for the Worker.
  const body = worker.creates[0];
  expect(body.allow).toContain("api.anthropic.com");
  expect(body.allow).toContain("*.npmjs.org");
  expect(JSON.stringify(worker.creates)).not.toContain(WORKER_HELD_KEY);
  expect(body.token).toBe("ghp_clone_only");
  // The container's environment has neither the token nor a real key.
  for (const c of containers) {
    expect(JSON.stringify(c.env)).not.toContain("ghp_clone_only");
    expect(c.env.ANTHROPIC_API_KEY).toBe("injected-by-the-worker");
  }

  // The user's checkout and the remote are untouched.
  expect(sh(repo, "git rev-parse --abbrev-ref HEAD")).toBe("main");
  expect(sh(origin, "git for-each-ref --format='%(refname)' refs/heads")).toBe("refs/heads/main");
});

test("cloudflareBox with the wrong secret starts nothing", async () => {
  const { root, repo, origin } = fixture();
  const worker = fakeWorker(root, origin, "right-secret");
  const results = await runAll(options(repo, ["anything"], FAKE_AGENT), (r) => cloudflareBox(r, { url: worker.url, secret: "wrong" }), WORKER_HELD_KEY);
  expect(results[0]).toMatchObject({ status: "failed" });
  if (results[0].status !== "failed") throw new Error("unreachable");
  expect(results[0].error).toContain("rejected BANTU_BURST_SECRET (401)");
  expect(worker.containers.size).toBe(0);
});

test("cloudflareBox sends the secret only over https, or to this machine", async () => {
  const request = { url: "https://github.com/o/r.git", revision: "a".repeat(40), token: undefined, vcpus: 1, timeoutMs: 60_000, policy: egressPolicy(WORKER_HELD_KEY, []), env: {} };
  expect(cloudflareBox(request, { url: "http://burst.example.com", secret: "s" })).rejects.toThrow("must be an https:// URL");
});
