// The HTTP API that deploy/vercel-sandbox/burst.ts calls, with no Cloudflare runtime imports so
// it can be tested with Bun. index.ts plugs the Durable Object in.
//
//   POST   /sandboxes                 {url, revision, token?, allow, env, timeoutMs} → 201 {id}
//   POST   /sandboxes/:id/run         {script} → 200 NDJSON: {"stdout":…} {"stderr":…} … {"exitCode":n}
//   PUT    /sandboxes/:id/files?path= raw bytes → 204
//   GET    /sandboxes/:id/files?path= → 200 raw bytes, or 404 when the file is missing
//   DELETE /sandboxes/:id             → 204
//
// Every request needs `Authorization: Bearer <BURST_SECRET>`. A sandbox that doesn't exist
// (never created, destroyed, or past its time limit) answers 410.

/** Where the repository is cloned. Relative paths in the API are relative to it. */
export const REPO_DIR = "/workspace/repo";
/** Claude Code needs some key to start; the Worker replaces it on the way out. */
export const PLACEHOLDER_KEY = "injected-by-the-worker";
/** The ephemeral CA that HTTPS interception signs with. It exists only at runtime. */
export const CA_PATH = "/etc/cloudflare/certs/cloudflare-containers-ca.crt";
/** exec() doesn't inherit the start() environment, so every command gets these. */
export const TRUST_ENV: Record<string, string> = {
  NODE_EXTRA_CA_CERTS: CA_PATH,
  GIT_SSL_CAINFO: CA_PATH,
  CURL_CA_BUNDLE: CA_PATH,
  SSL_CERT_FILE: CA_PATH,
};

/** Adds the interception CA to the system store, for tools that ignore the variables above. */
export const TRUST_SCRIPT = `set -eu
for _ in $(seq 1 30); do [ -s ${CA_PATH} ] && break; sleep 0.5; done
if [ ! -s ${CA_PATH} ]; then echo "the Containers CA never appeared at ${CA_PATH}" >&2; exit 1; fi
if command -v update-ca-certificates >/dev/null; then
  cp ${CA_PATH} /usr/local/share/ca-certificates/cloudflare-containers-ca.crt
  update-ca-certificates >/dev/null 2>&1
fi`;

/**
 * Clones one revision, shallowly. Inputs come from the environment so nothing is interpolated
 * into the script. The Worker adds GitHub credentials outside the container, so the URL and
 * the git config hold none.
 */
export const CLONE_SCRIPT = `set -eu
mkdir -p "$BANTU_REPO_DIR" && cd "$BANTU_REPO_DIR"
git init -q
git config user.name "Bantu burst"
git config user.email bantu@localhost
git remote add origin "$BANTU_CLONE_URL"
git fetch -q --depth=1 origin "$BANTU_REVISION"
git checkout -q --detach FETCH_HEAD`;

export type CreateInit = {
  url: string;
  revision: string;
  /** Used by the Worker for the clone only; never given to the container. */
  token?: string;
  /** Hostname patterns the sandbox may reach, e.g. `github.com` or `*.npmjs.org`. */
  allow: string[];
  env: Record<string, string>;
  timeoutMs: number;
};

/** What the router needs from one sandbox. The Durable Object implements it. */
export interface SandboxApi {
  exists(): Promise<boolean>;
  create(init: CreateInit): Promise<void>;
  run(script: string): Promise<ReadableStream<Uint8Array>>;
  write(path: string, body: ReadableStream<Uint8Array>): Promise<void>;
  /** null when the file doesn't exist. */
  read(path: string): Promise<ReadableStream<Uint8Array> | null>;
  destroy(): Promise<void>;
}

export type RouteEnv = { BURST_SECRET?: string };

/** The environment every command runs with. The client can't override the key or the trust paths. */
export function sandboxEnv(client: Record<string, string>): Record<string, string> {
  const env: Record<string, string> = { HOME: "/root", ...client };
  delete env.ANTHROPIC_AUTH_TOKEN;
  return {
    ...env,
    ...TRUST_ENV,
    ANTHROPIC_API_KEY: PLACEHOLDER_KEY,
    // The container runs as root, where Claude Code's bypassPermissions mode needs this.
    IS_SANDBOX: "1",
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
  };
}

/** Compares secrets in time that doesn't depend on where they differ, or on their lengths. */
export async function sameSecret(given: string, expected: string): Promise<boolean> {
  const encoder = new TextEncoder();
  const [a, b] = await Promise.all(
    [given, expected].map(async (s) => new Uint8Array(await crypto.subtle.digest("SHA-256", encoder.encode(s)))),
  );
  let diff = 0;
  for (let i = 0; i < a.length; i += 1) diff |= a[i] ^ b[i];
  return diff === 0;
}

const ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const REVISION = /^[0-9a-f]{40}([0-9a-f]{24})?$/;
const HOST_PATTERN = /^(\*\.)?[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)*$/;
const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
const MAX_TIMEOUT_MS = 24 * 60 * 60_000;

class BadRequest extends Error {}

function text(status: number, body: string): Response {
  return new Response(`${body}\n`, { status, headers: { "content-type": "text/plain; charset=utf-8" } });
}

/** Checks a create body by hand, so the Worker needs no schema library. */
export function parseCreate(body: unknown): CreateInit {
  if (typeof body !== "object" || body === null) throw new BadRequest("the body must be a JSON object");
  const b = body as Record<string, unknown>;
  if (typeof b.url !== "string" || !b.url.startsWith("https://")) throw new BadRequest("url must be an https:// clone URL");
  try {
    if (new URL(b.url).username || new URL(b.url).password) throw new BadRequest("put credentials in token, not in the URL");
  } catch (err) {
    if (err instanceof BadRequest) throw err;
    throw new BadRequest("url isn't a valid URL");
  }
  if (typeof b.revision !== "string" || !REVISION.test(b.revision)) throw new BadRequest("revision must be a full commit SHA");
  if (b.token !== undefined && (typeof b.token !== "string" || !b.token)) throw new BadRequest("token must be a non-empty string");
  if (!Array.isArray(b.allow) || !b.allow.every((h) => typeof h === "string" && HOST_PATTERN.test(h.toLowerCase()))) {
    throw new BadRequest('allow must be a list of hostnames such as "github.com" or "*.npmjs.org"');
  }
  if (typeof b.env !== "object" || b.env === null || Array.isArray(b.env)) throw new BadRequest("env must be an object");
  const env: Record<string, string> = {};
  for (const [k, v] of Object.entries(b.env)) {
    if (!ENV_NAME.test(k) || typeof v !== "string") throw new BadRequest(`env.${k} must be a string with a plain name`);
    env[k] = v;
  }
  if (typeof b.timeoutMs !== "number" || !Number.isFinite(b.timeoutMs) || b.timeoutMs < 60_000 || b.timeoutMs > MAX_TIMEOUT_MS) {
    throw new BadRequest("timeoutMs must be between one minute and 24 hours");
  }
  return {
    url: b.url,
    revision: b.revision,
    ...(b.token ? { token: b.token as string } : {}),
    allow: (b.allow as string[]).map((h) => h.toLowerCase()),
    env,
    timeoutMs: b.timeoutMs,
  };
}

function filePath(url: URL): string {
  const path = url.searchParams.get("path");
  if (!path || path.includes("\0")) throw new BadRequest("path is required");
  return path;
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** Routes one request. `sandbox` returns the stub for an id; `newId` is replaceable for tests. */
export async function route(
  request: Request,
  env: RouteEnv,
  sandbox: (id: string) => SandboxApi,
  newId: () => string = () => crypto.randomUUID(),
): Promise<Response> {
  // Refuse everything rather than run unauthenticated.
  if (!env.BURST_SECRET) return text(500, "BURST_SECRET isn't set on this Worker");
  const auth = request.headers.get("authorization") ?? "";
  const given = auth.startsWith("Bearer ") ? auth.slice("Bearer ".length) : "";
  if (!(await sameSecret(given, env.BURST_SECRET))) return text(401, "wrong or missing bearer secret");

  const url = new URL(request.url);
  const parts = url.pathname.split("/").filter(Boolean);
  try {
    if (parts.length === 1 && parts[0] === "sandboxes") {
      if (request.method !== "POST") return text(405, "use POST");
      const init = parseCreate(await request.json().catch(() => undefined));
      const id = newId();
      const box = sandbox(id);
      try {
        await box.create(init);
      } catch (err) {
        await box.destroy().catch(() => {});
        return text(502, `couldn't start the sandbox: ${message(err)}`);
      }
      return Response.json({ id }, { status: 201 });
    }

    if (parts[0] !== "sandboxes" || !parts[1] || parts.length > 3) return text(404, "no such route");
    const id = parts[1];
    if (!ID.test(id)) return text(404, "no such sandbox");
    const box = sandbox(id);
    const action = `${request.method} ${parts[2] ?? ""}`;

    if (action === "DELETE ") {
      await box.destroy();
      return new Response(null, { status: 204 });
    }
    if (!["POST run", "PUT files", "GET files"].includes(action)) return text(parts[2] ? 405 : 404, "no such route");
    if (!(await box.exists())) return text(410, "this sandbox doesn't exist or has stopped");

    if (action === "POST run") {
      const body = (await request.json().catch(() => undefined)) as { script?: unknown } | undefined;
      if (typeof body?.script !== "string" || !body.script) throw new BadRequest("script must be a non-empty string");
      return new Response(await box.run(body.script), { headers: { "content-type": "application/x-ndjson" } });
    }
    if (action === "PUT files") {
      const path = filePath(url);
      await box.write(path, request.body ?? new Response("").body!);
      return new Response(null, { status: 204 });
    }
    const stream = await box.read(filePath(url));
    if (!stream) return text(404, "no such file");
    return new Response(stream, { headers: { "content-type": "application/octet-stream" } });
  } catch (err) {
    if (err instanceof BadRequest) return text(400, err.message);
    return text(500, message(err));
  }
}

/** The parts of a running process that execStream reads. Bun.spawn and Container.exec both fit. */
export type StreamingProcess = {
  stdout: ReadableStream<Uint8Array> | null;
  stderr: ReadableStream<Uint8Array> | null;
  exitCode: Promise<number>;
  kill(): void;
};

/**
 * Turns a process into the run endpoint's NDJSON. A `{}` line goes out every 15 seconds so
 * nothing between here and the client treats a quiet agent as a dead connection.
 */
export function execStream(proc: StreamingProcess, heartbeatMs = 15_000): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  let finished = false;
  return new ReadableStream<Uint8Array>({
    async start(controller) {
      const send = (value: object) => {
        if (!finished) controller.enqueue(encoder.encode(`${JSON.stringify(value)}\n`));
      };
      const beat = setInterval(() => send({}), heartbeatMs);
      const pump = async (stream: ReadableStream<Uint8Array> | null, key: "stdout" | "stderr") => {
        if (!stream) return;
        const reader = stream.getReader();
        const decoder = new TextDecoder();
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          const s = decoder.decode(value, { stream: true });
          if (s) send({ [key]: s });
        }
        const rest = decoder.decode();
        if (rest) send({ [key]: rest });
      };
      try {
        await Promise.all([pump(proc.stdout, "stdout"), pump(proc.stderr, "stderr")]);
        send({ exitCode: await proc.exitCode });
      } catch (err) {
        send({ error: message(err) });
      } finally {
        clearInterval(beat);
        finished = true;
        controller.close();
      }
    },
    cancel() {
      finished = true;
      proc.kill();
    },
  });
}
