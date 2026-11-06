// The Cloudflare side of Bantu's burst runner: one Durable Object and Container per task.
// deploy/vercel-sandbox/burst.ts --provider cloudflare calls the API in api.ts.
// See docs/CLOUDFLARE-BURST.md.

import { Files, SandboxFileError } from "@cloudflare/sandbox";
import { DurableObject, WorkerEntrypoint } from "cloudflare:workers";

import { type CreateInit, CLONE_SCRIPT, execStream, REPO_DIR, route, type SandboxApi, sandboxEnv, TRUST_ENV, TRUST_SCRIPT } from "./api";
import { outboundDecision, type OutboundProps } from "./egress";

type Instance = "lite" | "basic" | "standard-1" | "standard-2" | "standard-3" | "standard-4";
const INSTANCES: Instance[] = ["lite", "basic", "standard-1", "standard-2", "standard-3", "standard-4"];

export interface Env {
  SANDBOX: DurableObjectNamespace<BurstSandbox>;
  /** Bearer secret the local script sends. `wrangler secret put BURST_SECRET`. */
  BURST_SECRET: string;
  /** Added to requests to api.anthropic.com on the way out. Never enters a container. */
  ANTHROPIC_API_KEY: string;
  /** A var in wrangler.jsonc; standard-2 when unset. */
  INSTANCE_TYPE?: string;
}

/** Stops an idle container even if the client vanished without deleting it. */
const INACTIVITY_MS = 10 * 60_000;
const ENV_KEY = "env";

interface BurstState extends DurableObjectState {
  readonly exports: Cloudflare.Exports & { readonly Outbound: LoopbackForExport<typeof Outbound> };
}

/** Every HTTP request on port 80 and HTTPS request on port 443 from a container lands here. */
export class Outbound extends WorkerEntrypoint<Env, OutboundProps> {
  override async fetch(request: Request): Promise<Response> {
    const next = outboundDecision(request, this.ctx.props, this.env.ANTHROPIC_API_KEY);
    return next instanceof Response ? next : fetch(next);
  }
}

export class BurstSandbox extends DurableObject<Env> implements SandboxApi {
  readonly #container: Container;
  readonly #files: Files;
  readonly #exports: BurstState["exports"];

  constructor(ctx: BurstState, env: Env) {
    super(ctx, env);
    const container = ctx.container;
    if (!container) throw new Error("This Durable Object has no container; check wrangler.jsonc");
    this.#container = container;
    this.#files = new Files(container);
    this.#exports = ctx.exports;
    // The timeout belongs to this instance of the object, not the container.
    if (container.running) void ctx.blockConcurrencyWhile(() => container.setInactivityTimeout(INACTIVITY_MS));
  }

  async exists(): Promise<boolean> {
    return this.#container.running;
  }

  async create(init: CreateInit): Promise<void> {
    if (this.#container.running) throw new Error("this sandbox is already running");
    const wanted = this.env.INSTANCE_TYPE ?? "standard-2";
    const instance = INSTANCES.find((i) => i === wanted);
    if (!instance) throw new Error(`INSTANCE_TYPE ${wanted} isn't one of ${INSTANCES.join(", ")}`);

    this.#container.start({
      image: this.#container.images.sandbox,
      instance,
      // Nothing leaves except HTTP and HTTPS, and those go through Outbound.
      enableInternet: false,
      labels: { app: "bantu-burst" },
    });
    // The GitHub token, if any, lives in these props only until the clone is done.
    await this.#intercept({ allow: init.allow, ...(init.token ? { cloneToken: init.token } : {}) });
    await this.#container.setInactivityTimeout(INACTIVITY_MS);
    // A hard limit, like the VM timeout on Vercel.
    await this.ctx.storage.setAlarm(Date.now() + init.timeoutMs);
    this.ctx.storage.kv.put(ENV_KEY, sandboxEnv(init.env));

    try {
      const trust = await this.#exec(TRUST_SCRIPT, "/", TRUST_ENV);
      if (trust.exitCode !== 0) throw new Error(`trusting the interception CA failed: ${trust.stderr.trim().slice(-500)}`);
      const clone = await this.#exec(CLONE_SCRIPT, "/", {
        ...TRUST_ENV,
        BANTU_REPO_DIR: REPO_DIR,
        BANTU_CLONE_URL: init.url,
        BANTU_REVISION: init.revision,
      });
      if (clone.exitCode !== 0) throw new Error(`the clone failed: ${clone.stderr.trim().slice(-500)}`);
    } finally {
      if (init.token) await this.#intercept({ allow: init.allow });
    }
  }

  async run(script: string): Promise<ReadableStream<Uint8Array>> {
    const proc = await this.#container.exec(["bash", "-c", script], {
      cwd: REPO_DIR,
      env: this.#env(),
      stdout: "pipe",
      stderr: "pipe",
    });
    return execStream({ stdout: proc.stdout, stderr: proc.stderr, exitCode: proc.exitCode, kill: () => proc.kill() });
  }

  async write(path: string, body: ReadableStream<Uint8Array>): Promise<void> {
    await this.#files.writeFile(path, body, { cwd: REPO_DIR });
  }

  async read(path: string): Promise<ReadableStream<Uint8Array> | null> {
    try {
      return (await this.#files.readFile(path, { cwd: REPO_DIR })).body;
    } catch (err) {
      if (SandboxFileError.is(err) && err.code === "ENOENT") return null;
      throw err;
    }
  }

  async destroy(): Promise<void> {
    await this.ctx.storage.deleteAlarm();
    this.ctx.storage.kv.delete(ENV_KEY);
    if (this.#container.running) await this.#container.destroy();
  }

  override async alarm(): Promise<void> {
    await this.destroy();
  }

  #env(): Record<string, string> {
    return this.ctx.storage.kv.get<Record<string, string>>(ENV_KEY) ?? sandboxEnv({});
  }

  /** Installing again replaces the props; in-flight connections pick up the new handler. */
  async #intercept(props: OutboundProps): Promise<void> {
    const outbound = this.#exports.Outbound({ props });
    await this.#container.interceptAllOutboundHttp(outbound);
    await this.#container.interceptOutboundHttps("*", outbound);
  }

  async #exec(script: string, cwd: string, env: Record<string, string>) {
    const proc = await this.#container.exec(["bash", "-c", script], { cwd, env });
    const out = await proc.output();
    const decoder = new TextDecoder();
    return { exitCode: out.exitCode, stdout: decoder.decode(out.stdout), stderr: decoder.decode(out.stderr) };
  }
}

export default {
  fetch(request, env): Promise<Response> {
    return route(request, env, (id) => env.SANDBOX.getByName(id) as unknown as SandboxApi);
  },
} satisfies ExportedHandler<Env>;
