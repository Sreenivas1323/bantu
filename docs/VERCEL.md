# Burst runs on Vercel Sandbox

Sometimes you want ten agents at once, briefly. [`deploy/vercel-sandbox/burst.ts`](../deploy/vercel-sandbox/burst.ts)
runs each task in its own Firecracker microVM in **your** Vercel account, and brings the work
back as local branches to review in Bantu. You pay Vercel and your model provider directly.

```sh
cd deploy/vercel-sandbox && bun install

# Prints the plan and the most it can cost. Nothing starts without --yes.
bun run burst.ts --repo ~/code/app --task "Add a health check" --task "Fix the login redirect"

export ANTHROPIC_API_KEY=… VERCEL_TOKEN=… VERCEL_TEAM_ID=… VERCEL_PROJECT_ID=…
bun run burst.ts --repo ~/code/app --task "Add a health check" --task "Fix the login redirect" --yes
```

Each task ends up on a branch such as `bantu/burst-add-health-check`. In Bantu, use **New
workspace → From a branch** to review it. Nothing is pushed.

Tasks start from `--base` (default `main`) as it is on `origin`, so push anything they depend
on first. `--setup "bun install"` runs before the agent. See `--help` for the rest.

## What you need

- **An Anthropic API key.** A Claude subscription's sign-in can't be used inside a VM.
- **Vercel credentials**: `VERCEL_TOKEN`, `VERCEL_TEAM_ID` and `VERCEL_PROJECT_ID`, or
  `VERCEL_OIDC_TOKEN` from `vercel env pull`.
- **`GITHUB_TOKEN`** for private repositories, used only to clone.

## How your credentials are protected

An agent reading untrusted content could try to send secrets somewhere, so the VM holds as
few as possible.

- **The API key stays outside the VM.** The VM's firewall adds it to requests to
  `api.anthropic.com`. Inside the VM, `ANTHROPIC_API_KEY` is a placeholder.
- **No push access.** Commits come back as a git bundle that this script reads from the VM.
  The clone's `origin` remote is removed before the agent starts.
- **Limited network.** The VM can reach Anthropic, GitHub and npm. Add others with `--allow`,
  for example `--allow "*.pypi.org"`. Domains are matched on TLS traffic only, so treat this as
  a limit on ordinary tools rather than a hard wall.
- **Throwaway.** VMs are `persistent: false`, so their disks are deleted when they stop.

Because the VM is the boundary, the agent runs with `bypassPermissions` and doesn't stop to ask.

## Cost and limits

Vercel's Hobby plan includes 5 vCPU-hours free, with sessions up to 45 minutes and 10 VMs at
once. Pro is about $0.13 per vCPU-hour, plus $0.02 per GB-hour of memory, with sessions up to 24
hours. Every VM stops when its task ends. If you interrupt the script, running VMs stop at their
`--minutes` limit.

> Status: the full run is tested with real git and a local stand-in for the VM, and the script
> typechecks against the real SDK. It hasn't run against Vercel's API yet.
