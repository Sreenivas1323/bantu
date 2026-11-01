# Competitive research

Bantu learns from two products that do something similar:

- **[Conductor](https://www.conductor.build)** is a proprietary Mac app. It's free for local use; a $50/mo Pro plan adds cloud, mobile and an API.
- **[Pane](https://github.com/greenfield-inc/Pane)** is AGPL-3.0, built on Electron, terminal-first and self-hosted.

**Rule: we copy features, never code or assets.** Everything here comes from public docs,
changelogs and release notes.

| File | What |
|---|---|
| [parity.md](parity.md) | Every known Conductor and Pane feature, with Bantu's status (have / planned / skip) and milestone |
| [ideas.md](ideas.md) | Idea cards: where we can do better than both |
| [snapshots/](snapshots/) | Machine-readable changelog snapshots used by the watcher |

## The watcher loop

```
Monday 05:00 UTC
  .github/workflows/changelog-watch.yml
    └─ bun scripts/changelog-watch.ts --open-issues
         ├─ Conductor: changelog.md (falls back to the /changelog HTML)  → snapshots/conductor.json
         ├─ Conductor: llms.txt docs pages (new pages = new features)    → snapshots/conductor-docs.json
         ├─ Pane: GitHub releases of greenfield-inc/Pane                 → snapshots/pane.json
         ├─ one `competitor-update` issue per new entry
         └─ commits the updated snapshots
            │
Weekly Claude routine (runs on the maintainer's Claude plan, not project infrastructure)
    ├─ reads open `competitor-update` issues and the linked pages
    ├─ updates parity.md (have / planned / skip) and adds idea cards to ideas.md
    ├─ opens a PR labelled `idea`, ranked by impact ÷ effort
    └─ comments on and closes the triaged issues
```

Run it locally:

```sh
bun scripts/changelog-watch.ts --dry-run   # show new entries, don't touch snapshots
bun scripts/changelog-watch.ts             # update snapshots, print new entries
```

A source with no snapshot file yet is only **seeded**, so the first run never opens a
flood of issues. Entries are never removed from snapshots, so an entry that disappears
upstream won't be reported as new again later.

## Snapshot as of 2026-11-01

| Product | Latest | Entries tracked |
|---|---|---|
| Conductor | 0.92.0 "Remote control" (2026-10-23) | 207 changelog entries, 12 docs pages |
| Pane | v2.4.212 (2026-10-31) | 300 most recent releases |

## Sources for the cost figures in ARCHITECTURE.md (checked 2026-11-01)

- Vercel Sandbox pricing: https://vercel.com/docs/sandbox/pricing
- Vercel Functions limits: https://vercel.com/docs/functions/limitations
- EC2 t4g prices: https://instances.vantage.sh/aws/ec2/t4g.small
- Public IPv4 pricing: https://aws.amazon.com/vpc/pricing/
- EBS pricing: https://aws.amazon.com/ebs/pricing
- Lightsail pricing: https://aws.amazon.com/lightsail/pricing/
- CloudFront pricing and WebSockets: https://aws.amazon.com/cloudfront/pricing/ ·
  https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/distribution-working-with.websockets.html
- CloudFront VPC origins: https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/private-content-vpc-origins.html
