#!/usr/bin/env bun
// Watches the Conductor and Pane changelogs and reports entries we haven't seen.
//
//   bun scripts/changelog-watch.ts                 # update snapshots, print new entries
//   bun scripts/changelog-watch.ts --open-issues   # also open one GitHub issue per new entry
//   bun scripts/changelog-watch.ts --dry-run       # print new entries, don't write snapshots
//
// Snapshots live in docs/competitive/snapshots/. A source whose snapshot doesn't
// exist yet is only seeded, so the first run never opens a flood of issues.
// No dependencies: needs Bun (or Node 18+ with a TS loader) and, for
// --open-issues, the `gh` CLI.

import { existsSync, mkdirSync, readFileSync, writeFileSync, appendFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";

type Entry = { id: string; title: string; url: string; date?: string };
type Source = { key: string; label: string; fetch: () => Promise<Entry[]> };

const ROOT = join(import.meta.dir ?? dirname(new URL(import.meta.url).pathname), "..");
const SNAPSHOT_DIR = join(ROOT, "docs/competitive/snapshots");
const args = new Set(process.argv.slice(2));
const openIssues = args.has("--open-issues");
const dryRun = args.has("--dry-run");

const UA = { "user-agent": "bantu-changelog-watch (+https://github.com/Sreenivas1323/bantu)" };

async function getText(url: string, headers: Record<string, string> = {}): Promise<string> {
  const res = await fetch(url, { headers: { ...UA, ...headers } });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.text();
}

// Conductor publishes a markdown changelog:
//   - [0.92.0: Remote control](/changelog/0.92.0-remote-control) - October 23, 2026
// Older entries may omit the date. Falls back to scraping the HTML page.
async function fetchConductorChangelog(): Promise<Entry[]> {
  const base = "https://www.conductor.build";
  const entries = new Map<string, Entry>();
  const md = await getText(`${base}/changelog.md`).catch(() => "");
  const re = /\[(\d+\.\d+\.\d+)(?::\s*([^\]]*))?\]\((\/changelog\/[^)\s]+)\)(?:\s*-\s*([A-Z][a-z]+ \d{1,2}, \d{4}))?/g;
  for (const m of md.matchAll(re)) {
    const [, version, title, path, date] = m;
    if (entries.has(version)) continue;
    entries.set(version, {
      id: version,
      title: (title ?? "").trim() || version,
      url: base + path,
      ...(date ? { date: toIsoDate(date) } : {}),
    });
  }
  if (entries.size === 0) {
    const html = await getText(`${base}/changelog`);
    for (const m of html.matchAll(/href="(\/changelog\/(\d+\.\d+\.\d+)-([^"]+))"/g)) {
      const [, path, version, slug] = m;
      if (!entries.has(version)) {
        entries.set(version, { id: version, title: slug.replace(/-/g, " "), url: base + path });
      }
    }
  }
  if (entries.size === 0) throw new Error("conductor changelog: no entries parsed (page format changed?)");
  return [...entries.values()];
}

// New docs pages are a strong signal of new features, so track llms.txt links too.
async function fetchConductorDocs(): Promise<Entry[]> {
  const txt = await getText("https://www.conductor.build/llms.txt");
  const entries = new Map<string, Entry>();
  for (const m of txt.matchAll(/\[([^\]]+)\]\((https:\/\/www\.conductor\.build\/[^)\s]+)\)/g)) {
    const [, title, url] = m;
    if (url.includes("/changelog/")) continue;
    if (!entries.has(url)) entries.set(url, { id: url, title: title.trim(), url });
  }
  if (entries.size === 0) throw new Error("conductor llms.txt: no links parsed");
  return [...entries.values()];
}

async function fetchPaneReleases(): Promise<Entry[]> {
  const headers: Record<string, string> = { accept: "application/vnd.github+json" };
  const token = process.env.GITHUB_TOKEN ?? process.env.GH_TOKEN;
  if (token) headers.authorization = `Bearer ${token}`;
  const entries: Entry[] = [];
  for (let page = 1; page <= 3; page++) {
    const body = await getText(
      `https://api.github.com/repos/greenfield-inc/Pane/releases?per_page=100&page=${page}`,
      headers,
    );
    const releases = JSON.parse(body) as Array<{
      tag_name: string;
      name: string | null;
      html_url: string;
      published_at: string | null;
      draft: boolean;
    }>;
    for (const r of releases) {
      if (r.draft) continue;
      entries.push({
        id: r.tag_name,
        title: (r.name ?? "").trim() || r.tag_name,
        url: r.html_url,
        ...(r.published_at ? { date: r.published_at.slice(0, 10) } : {}),
      });
    }
    if (releases.length < 100) break;
  }
  return entries;
}

const SOURCES: Source[] = [
  { key: "conductor", label: "Conductor", fetch: fetchConductorChangelog },
  { key: "conductor-docs", label: "Conductor docs", fetch: fetchConductorDocs },
  { key: "pane", label: "Pane", fetch: fetchPaneReleases },
];

function toIsoDate(s: string): string {
  const d = new Date(`${s} UTC`);
  return Number.isNaN(d.getTime()) ? s : d.toISOString().slice(0, 10);
}

function readSnapshot(path: string): Entry[] | null {
  if (!existsSync(path)) return null;
  return JSON.parse(readFileSync(path, "utf8")) as Entry[];
}

function sortEntries(entries: Entry[]): Entry[] {
  return [...entries].sort(
    (a, b) => (b.date ?? "").localeCompare(a.date ?? "") || b.id.localeCompare(a.id, undefined, { numeric: true }),
  );
}

function openIssue(source: Source, e: Entry): void {
  const title = `[${source.label} ${source.key === "conductor-docs" ? "new page" : e.id}] ${e.title}`.slice(0, 250);
  const body = [
    `**Source:** ${source.label}`,
    `**Entry:** [${e.title}](${e.url})${e.date ? ` (${e.date})` : ""}`,
    "",
    "Triage: does Bantu want this?",
    "- [ ] Read the entry and summarize the feature",
    "- [ ] Update `docs/competitive/parity.md` (have / planned / skip)",
    "- [ ] If planned, add an idea card to `docs/competitive/ideas.md` with our better version",
    "",
    "_Opened automatically by `scripts/changelog-watch.ts`._",
  ].join("\n");
  const r = spawnSync("gh", ["issue", "create", "--title", title, "--body", body, "--label", "competitor-update"], {
    stdio: ["ignore", "inherit", "inherit"],
  });
  if (r.status !== 0) throw new Error(`gh issue create failed for ${source.key}:${e.id}`);
}

async function main(): Promise<void> {
  mkdirSync(SNAPSHOT_DIR, { recursive: true });
  const report: Record<string, { seeded: boolean; total: number; new: Entry[]; error?: string }> = {};
  let failures = 0;

  for (const source of SOURCES) {
    const path = join(SNAPSHOT_DIR, `${source.key}.json`);
    try {
      const current = await source.fetch();
      const previous = readSnapshot(path);
      const seen = new Set((previous ?? []).map((e) => e.id));
      const fresh = previous === null ? [] : current.filter((e) => !seen.has(e.id));
      // Keep entries that disappeared upstream so they are never reported as new again.
      const merged = new Map<string, Entry>((previous ?? []).map((e) => [e.id, e]));
      for (const e of current) merged.set(e.id, e);
      if (!dryRun) writeFileSync(path, JSON.stringify(sortEntries([...merged.values()]), null, 2) + "\n");
      report[source.key] = { seeded: previous === null, total: merged.size, new: sortEntries(fresh) };
      if (openIssues && !dryRun) for (const e of [...fresh].reverse()) openIssue(source, e);
    } catch (err) {
      failures++;
      report[source.key] = { seeded: false, total: 0, new: [], error: String(err) };
    }
  }

  const out = JSON.stringify(report, null, 2);
  console.log(out);
  const newCount = Object.values(report).reduce((n, r) => n + r.new.length, 0);
  if (process.env.GITHUB_OUTPUT) {
    appendFileSync(process.env.GITHUB_OUTPUT, `new_count=${newCount}\nreport<<EOF\n${out}\nEOF\n`);
  }
  if (failures === SOURCES.length) process.exit(1);
}

await main();
