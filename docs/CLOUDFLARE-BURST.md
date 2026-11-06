# Burst runs on Cloudflare Containers

[`deploy/vercel-sandbox/burst.ts`](../deploy/vercel-sandbox/burst.ts) can run its tasks in
Cloudflare Containers instead of Vercel Sandbox. You deploy a small Worker from
[`deploy/cloudflare-sandbox`](../deploy/cloudflare-sandbox) into **your** Cloudflare account,
and the script calls it. Each task gets its own Durable Object and container, and the work
comes back as local branches, the same way as on Vercel ([VERCEL.md](VERCEL.md)).

> Status: none of this has run against Cloudflare's API yet. The Worker typechecks against
> `@cloudflare/sandbox` 1.0.3 and `@cloudflare/workers-types`, its routing, auth and egress
> rules are unit-tested with a mocked sandbox, and the full burst run is tested with real git
> against the Worker's router on a local port, with local folders standing in for containers.
> The Docker image hasn't been built.

## Setup

You need a Workers Paid plan ($5/month), which Containers require, and Docker running locally
so `wrangler deploy` can build the image.

```sh
cd deploy/cloudflare-sandbox
bun install
bunx wrangler@^4.148.0 login

# Any long random string. The script sends it as a bearer token.
openssl rand -hex 32 | tee /dev/stderr | bunx wrangler@^4.148.0 secret put BURST_SECRET
# Your Anthropic API key. It stays in the Worker.
bunx wrangler@^4.148.0 secret put ANTHROPIC_API_KEY

bun run deploy   # bunx wrangler deploy: builds the Dockerfile and pushes it
```

`wrangler deploy` prints the Worker's URL, such as `https://bantu-burst.<you>.workers.dev`.
`wrangler.jsonc` lists both secrets under `secrets.required`, so deploy refuses to go ahead
until they're set.

What gets deployed:

- `src/index.ts`: the Worker, the `BurstSandbox` Durable Object that starts and drives one
  container, and the `Outbound` entrypoint that every request from a container goes through.
- `src/api.ts`: the HTTP API and its auth. `src/egress.ts`: the egress rules.
- `Dockerfile`: `node:26-bookworm-slim` with bash, git, ripgrep and Claude Code 2.1.318, plus
  `sandbox-shim` copied from `cloudflare/sandbox:1.0.3`, as the SDK's Claude Code example does.
  Keep that tag equal to the `@cloudflare/sandbox` version in `package.json`.
- `wrangler.jsonc`: the container with `scheduling_policy: "durable_object"`, the `SANDBOX`
  Durable Object binding, and the class declared under `exports` with SQLite storage. That's
  how the SDK's 1.0 examples declare it; wrangler calls `migrations` the legacy form, and the
  two can't be combined. `max_instances` is 10. `INSTANCE_TYPE` is `standard-2` (1 vCPU,
  6 GiB, 12 GB disk); change it there to `standard-3` or `standard-4` for bigger tasks.

`@cloudflare/sandbox` 1.0 is the current stable line (npm `latest` is 1.0.3; 0.12.x gets
maintenance releases). In 1.0 your own Durable Object starts the container through
`ctx.container`; the package only adds file operations. Posts about the 0.x `getSandbox()` API
don't apply.

## Usage

```sh
export BANTU_BURST_URL=https://bantu-burst.<you>.workers.dev
export BANTU_BURST_SECRET=…        # the BURST_SECRET you set
export GITHUB_TOKEN=…              # only for private repositories

cd deploy/vercel-sandbox
# Prints the plan and the most it can cost. Nothing starts without --yes.
bun run burst.ts --provider cloudflare --repo ~/code/app --task "Add a health check"
bun run burst.ts --provider cloudflare --repo ~/code/app --task "Add a health check" --yes
```

The other flags work as on Vercel, except `--vcpus`: the Worker's `INSTANCE_TYPE` sets the
size. `ANTHROPIC_API_KEY` isn't read on this machine for Cloudflare runs. The script refuses a
`BANTU_BURST_URL` that isn't `https://`, so the secret isn't sent in the clear.

The API, all requiring `Authorization: Bearer <BURST_SECRET>`:

| Request | Does |
| --- | --- |
| `POST /sandboxes` `{url, revision, token?, allow, env, timeoutMs}` | Starts a container, clones `url` at `revision` (shallow) into `/workspace/repo`, returns `{id}` |
| `POST /sandboxes/:id/run` `{script}` | Runs `bash -c script` in the repo; streams NDJSON `{"stdout"}` / `{"stderr"}` lines, `{}` heartbeats every 15 s, then `{"exitCode"}` |
| `PUT /sandboxes/:id/files?path=` | Writes the raw body to a file |
| `GET /sandboxes/:id/files?path=` | Reads a file; 404 if it's missing |
| `DELETE /sandboxes/:id` | Destroys the container |

A sandbox that has stopped answers 410. Each container is destroyed by a Durable Object alarm at
`timeoutMs` (from `--minutes`), and by Cloudflare after 10 minutes with no requests, so one
left behind by an interrupted script doesn't run for long.

## How your credentials are protected

The goal is the same as on Vercel: the container holds no secret worth stealing.

**The API key never enters the container.** The container starts with `enableInternet: false`,
and the Durable Object then routes every HTTP request (port 80) and every HTTPS request (port
443, any hostname) to the `Outbound` entrypoint in the Worker:

```ts
container.start({ image, instance, enableInternet: false, labels });
await container.interceptAllOutboundHttp(outbound);
await container.interceptOutboundHttps("*", outbound);
```

`Outbound` sets `x-api-key` from the Worker's `ANTHROPIC_API_KEY` secret on HTTPS requests to
`api.anthropic.com` (and drops any `Authorization` header), refuses plain HTTP to that host, and
answers 403 for any host not on the allowlist. Inside the container, `ANTHROPIC_API_KEY` is the
placeholder `injected-by-the-worker`, and the Worker sets that itself, so a client can't put the
real key in the container's environment.

Where this comes from:

- The types in `@cloudflare/workers-types` 5.20261106.1, on the `Container` interface:
  `interceptOutboundHttp(addr: string, binding: Fetcher)`, `interceptAllOutboundHttp(binding:
  Fetcher)`, `interceptOutboundHttps(addr: string, binding: Fetcher)`, and `enableInternet?:
  boolean` in `ContainerStartupOptions`.
- Cloudflare's [network docs](https://developers.cloudflare.com/sandbox/network/): "A Container
  that starts with `enableInternet: false` reaches only the hostnames you intercept." and "Keep
  the token in your Worker instead … your Worker intercepts each request and adds the token."
- The [outbound traffic docs](https://developers.cloudflare.com/containers/configuration/outbound-traffic/):
  HTTPS interception re-signs traffic with an ephemeral CA at
  `/etc/cloudflare/certs/cloudflare-containers-ca.crt`, created at runtime; with
  `enableInternet: false`, ports other than 80 and 443 are denied and DNS only answers for
  intercepted names.
- The SDK's own Claude Code example (`examples/coding-agents` in cloudflare/sandbox-sdk) uses
  the same three calls, a placeholder `ANTHROPIC_API_KEY`, and the CA variables below.

Because traffic is re-signed, each command runs with `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`,
`CURL_CA_BUNDLE` and `GIT_SSL_CAINFO` pointing at that CA, and the Worker adds it to the system
store with `update-ca-certificates` before cloning. A tool with its own CA list that ignores all
of these will fail TLS rather than get around the Worker.

**GitHub token.** `GITHUB_TOKEN` goes from your machine to the Worker in the create request. The
Worker puts it in the `Outbound` props for the clone only, and adds it as an `Authorization`
header on HTTPS requests to `github.com`. Once the clone is done the intercepts are installed
again without it. It's never in the container's environment, git config or remote URL, and
`burst.ts` removes the `origin` remote before the agent starts.

**Limited network.** The allowlist is the same as on Vercel: Anthropic, GitHub, npm and anything
you add with `--allow`. Only hostnames are sent to the Worker. Unlike Vercel's TLS-name
matching, every HTTP and HTTPS request on Cloudflare goes through Worker code, and other ports
don't leave the container at all.

**No push access.** Work comes back as a git bundle read through the API.

What this doesn't protect against:

- The agent can *use* the key, through `api.anthropic.com`, for as long as the container runs.
  It can't read it. The same is true on Vercel.
- Anyone with `BURST_SECRET` can start containers on your account and spend your API key. Treat
  it like the key itself. The Worker compares it in constant time (SHA-256 of both sides, then a
  full-length XOR), and refuses every request if it isn't set.
- Allowlisted hosts can still receive data the agent sends them.

What hasn't been checked against the real platform: that `interceptOutboundHttps("*", …)`
behaves as documented when called through `ctx.container` directly, that the CA is in place by
the time the trust step runs (it waits up to 15 seconds, then fails the create), that Claude
Code's native binary trusts the CA through these variables (Cloudflare's example relies on it),
and that streaming a ReadableStream from the Durable Object through the Worker holds up for a
30-minute command.

Because the container is the boundary, the agent runs with `bypassPermissions`. The container
runs as root, so the Worker sets `IS_SANDBOX=1`, which Claude Code needs for that.

## Cost

Cloudflare Containers on Workers Paid bill CPU while it's busy, and memory and disk while the
container runs:

| | Price | Included each month |
| --- | --- | --- |
| CPU | $0.000020 per vCPU-second | 375 vCPU-minutes |
| Memory | $0.0000025 per GiB-second | 25 GiB-hours |
| Disk | $0.00000007 per GB-second | 200 GB-hours |

Plus $5/month for Workers Paid. The script's estimate assumes `standard-2` and every container
busy for its whole `--minutes`, so it's an upper bound: 4 tasks at 30 minutes come to about
$0.26 at list price, or $0 if the month's allowance is unused. Requests and Durable Object
duration are billed under Workers Paid too, and the estimate leaves them out. Model usage is
billed to the API key in the Worker.
