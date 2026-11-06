// The Worker's routing, auth and egress rules, with a mocked sandbox in place of the Durable
// Object and Container. The Cloudflare runtime itself isn't exercised here.

import { expect, test } from "bun:test";

import { type CreateInit, execStream, PLACEHOLDER_KEY, route, type SandboxApi, sameSecret, sandboxEnv } from "../src/api.ts";
import { hostAllowed, outboundDecision } from "../src/egress.ts";

const SECRET = "s3cret-for-tests";
const ID = "0b9d2c1e-1111-4222-8333-444455556666";
const SHA = "a".repeat(40);

type Call = [string, ...unknown[]];

function mockSandbox(opts: { running?: boolean; failCreate?: boolean; files?: Record<string, string> } = {}) {
  const calls: Call[] = [];
  let running = opts.running ?? true;
  const files = new Map(Object.entries(opts.files ?? {}));
  const api: SandboxApi = {
    async exists() {
      return running;
    },
    async create(init) {
      calls.push(["create", init]);
      if (opts.failCreate) throw new Error("clone exploded");
      running = true;
    },
    async run(script) {
      calls.push(["run", script]);
      return new Response('{"stdout":"hi\\n"}\n{"exitCode":0}\n').body!;
    },
    async write(path, body) {
      files.set(path, await new Response(body).text());
      calls.push(["write", path]);
    },
    async read(path) {
      const f = files.get(path);
      return f === undefined ? null : new Response(f).body!;
    },
    async destroy() {
      calls.push(["destroy"]);
      running = false;
    },
  };
  return { api, calls, files };
}

function req(method: string, path: string, body?: BodyInit, secret: string | null = SECRET) {
  const headers: Record<string, string> = {};
  if (secret !== null) headers.authorization = `Bearer ${secret}`;
  return new Request(`https://burst.example${path}`, { method, headers, body });
}

const createBody = (extra: Partial<CreateInit> = {}) =>
  JSON.stringify({ url: "https://github.com/o/r.git", revision: SHA, allow: ["github.com", "*.npmjs.org"], env: { A: "1" }, timeoutMs: 600_000, ...extra });

test("every request needs the bearer secret", async () => {
  const { api, calls } = mockSandbox();
  const env = { BURST_SECRET: SECRET };
  expect((await route(req("POST", "/sandboxes", createBody(), null), env, () => api)).status).toBe(401);
  expect((await route(req("POST", "/sandboxes", createBody(), "wrong"), env, () => api)).status).toBe(401);
  expect((await route(req("POST", "/sandboxes", createBody(), `${SECRET}x`), env, () => api)).status).toBe(401);
  expect((await route(req("DELETE", `/sandboxes/${ID}`, undefined, "wrong"), env, () => api)).status).toBe(401);
  const basic = new Request("https://burst.example/sandboxes", { method: "POST", headers: { authorization: `Basic ${SECRET}` }, body: createBody() });
  expect((await route(basic, env, () => api)).status).toBe(401);
  expect(calls).toEqual([]);
});

test("with no BURST_SECRET set, nothing runs", async () => {
  const { api, calls } = mockSandbox();
  const r = await route(req("POST", "/sandboxes", createBody(), ""), {}, () => api);
  expect(r.status).toBe(500);
  expect(await r.text()).toContain("BURST_SECRET");
  expect(calls).toEqual([]);
});

test("sameSecret", async () => {
  expect(await sameSecret("abc", "abc")).toBe(true);
  expect(await sameSecret("abc", "abd")).toBe(false);
  expect(await sameSecret("", "abc")).toBe(false);
});

test("create validates its body and returns an id", async () => {
  const { api, calls } = mockSandbox();
  const env = { BURST_SECRET: SECRET };
  const ok = await route(req("POST", "/sandboxes", createBody({ token: "ghp_x" })), env, () => api, () => ID);
  expect(ok.status).toBe(201);
  expect(await ok.json()).toEqual({ id: ID });
  expect(calls[0][0]).toBe("create");
  expect((calls[0][1] as CreateInit).token).toBe("ghp_x");

  for (const bad of [
    createBody({ revision: "main" }),
    createBody({ url: "http://github.com/o/r.git" }),
    createBody({ url: "https://user:pw@github.com/o/r.git" }),
    createBody({ allow: ["evil.com/path"] }),
    createBody({ timeoutMs: 5 }),
    createBody({ env: { "BAD NAME": "x" } }),
    "not json",
  ]) {
    expect((await route(req("POST", "/sandboxes", bad), env, () => api)).status).toBe(400);
  }
});

test("a failed create is cleaned up", async () => {
  const { api, calls } = mockSandbox({ running: false, failCreate: true });
  const r = await route(req("POST", "/sandboxes", createBody()), { BURST_SECRET: SECRET }, () => api, () => ID);
  expect(r.status).toBe(502);
  expect(await r.text()).toContain("clone exploded");
  expect(calls.map((c) => c[0])).toEqual(["create", "destroy"]);
});

test("run, write, read and destroy reach the right sandbox", async () => {
  const { api, calls, files } = mockSandbox({ files: { "a.txt": "A" } });
  const seen: string[] = [];
  const env = { BURST_SECRET: SECRET };
  const pick = (id: string) => {
    seen.push(id);
    return api;
  };

  const run = await route(req("POST", `/sandboxes/${ID}/run`, JSON.stringify({ script: "echo hi" })), env, pick);
  expect(run.headers.get("content-type")).toBe("application/x-ndjson");
  expect(await run.text()).toContain('"exitCode":0');

  expect((await route(req("PUT", `/sandboxes/${ID}/files?path=.git/p.txt`, "prompt"), env, pick)).status).toBe(204);
  expect(files.get(".git/p.txt")).toBe("prompt");

  const read = await route(req("GET", `/sandboxes/${ID}/files?path=a.txt`), env, pick);
  expect(await read.text()).toBe("A");
  expect((await route(req("GET", `/sandboxes/${ID}/files?path=nope`), env, pick)).status).toBe(404);
  expect((await route(req("GET", `/sandboxes/${ID}/files`), env, pick)).status).toBe(400);
  expect((await route(req("POST", `/sandboxes/${ID}/run`, JSON.stringify({})), env, pick)).status).toBe(400);

  expect((await route(req("DELETE", `/sandboxes/${ID}`), env, pick)).status).toBe(204);
  expect(seen.every((id) => id === ID)).toBe(true);
  expect(calls.map((c) => c[0])).toEqual(["run", "write", "destroy"]);

  // Once it's gone, it's gone.
  expect((await route(req("POST", `/sandboxes/${ID}/run`, JSON.stringify({ script: "x" })), env, pick)).status).toBe(410);
});

test("unknown routes and ids", async () => {
  const { api } = mockSandbox();
  const env = { BURST_SECRET: SECRET };
  expect((await route(req("GET", "/"), env, () => api)).status).toBe(404);
  expect((await route(req("GET", "/sandboxes"), env, () => api)).status).toBe(405);
  expect((await route(req("POST", "/sandboxes/not-an-id/run", "{}"), env, () => api)).status).toBe(404);
  expect((await route(req("PATCH", `/sandboxes/${ID}/run`), env, () => api)).status).toBe(405);
});

test("the sandbox environment always gets the placeholder key", () => {
  const env = sandboxEnv({ ANTHROPIC_API_KEY: "sk-real", ANTHROPIC_AUTH_TOKEN: "t", FOO: "bar" });
  expect(env.ANTHROPIC_API_KEY).toBe(PLACEHOLDER_KEY);
  expect(env.ANTHROPIC_AUTH_TOKEN).toBeUndefined();
  expect(env.FOO).toBe("bar");
  expect(env.NODE_EXTRA_CA_CERTS).toContain("cloudflare-containers-ca.crt");
});

test("execStream reports output and the exit code as NDJSON", async () => {
  const proc = Bun.spawn(["bash", "-c", "echo out; echo err >&2; exit 7"], { stdout: "pipe", stderr: "pipe" });
  const body = await new Response(execStream({ stdout: proc.stdout, stderr: proc.stderr, exitCode: proc.exited, kill: () => proc.kill() })).text();
  const lines = body.trim().split("\n").map((l) => JSON.parse(l));
  expect(lines).toContainEqual({ stdout: "out\n" });
  expect(lines).toContainEqual({ stderr: "err\n" });
  expect(lines.at(-1)).toEqual({ exitCode: 7 });
});

test("the model key is added for api.anthropic.com over HTTPS only", () => {
  const props = { allow: ["github.com"] };
  const out = outboundDecision(
    new Request("https://api.anthropic.com/v1/messages", { method: "POST", headers: { "x-api-key": PLACEHOLDER_KEY, authorization: "Bearer junk" }, body: "{}" }),
    props,
    "sk-real",
  );
  if (!(out instanceof Request)) throw new Error("expected a request");
  expect(out.headers.get("x-api-key")).toBe("sk-real");
  expect(out.headers.get("authorization")).toBeNull();

  const plain = outboundDecision(new Request("http://api.anthropic.com/v1/messages"), props, "sk-real");
  expect(plain).toBeInstanceOf(Response);
  expect((plain as Response).status).toBe(403);
  expect(((outboundDecision(new Request("https://api.anthropic.com/"), props, undefined)) as Response).status).toBe(500);
});

test("everything else is allowlisted, and never gets the key", () => {
  const props = { allow: ["github.com", "*.npmjs.org"] };
  const denied = outboundDecision(new Request("https://evil.example/steal"), props, "sk-real");
  expect((denied as Response).status).toBe(403);

  const npm = outboundDecision(new Request("https://registry.npmjs.org/x", { headers: { "x-api-key": "p" } }), props, "sk-real");
  if (!(npm instanceof Request)) throw new Error("expected a request");
  expect(npm.headers.get("x-api-key")).toBe("p");

  expect(hostAllowed("registry.npmjs.org", props.allow)).toBe(true);
  expect(hostAllowed("npmjs.org", props.allow)).toBe(false);
  expect(hostAllowed("evilnpmjs.org", props.allow)).toBe(false);
  expect(hostAllowed("GitHub.com.", props.allow)).toBe(true);
  expect(hostAllowed("api.github.com", props.allow)).toBe(false);
});

test("the GitHub token is added only while cloning, only for github.com over HTTPS", () => {
  const cloning = { allow: ["github.com", "*.github.com"], cloneToken: "ghp_x" };
  const gh = outboundDecision(new Request("https://github.com/o/r.git/info/refs"), cloning, "k");
  if (!(gh instanceof Request)) throw new Error("expected a request");
  expect(gh.headers.get("authorization")).toBe(`Basic ${btoa("x-access-token:ghp_x")}`);

  const other = outboundDecision(new Request("https://api.github.com/user"), cloning, "k") as Request;
  expect(other.headers.get("authorization")).toBeNull();
  const plain = outboundDecision(new Request("http://github.com/o/r.git"), cloning, "k") as Request;
  expect(plain.headers.get("authorization")).toBeNull();

  const after = outboundDecision(new Request("https://github.com/o/r.git/info/refs"), { allow: cloning.allow }, "k") as Request;
  expect(after.headers.get("authorization")).toBeNull();
});
