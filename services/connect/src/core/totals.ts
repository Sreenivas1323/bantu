// Team totals for the dashboard. Only sums across the org are ever read out, and nothing is
// shown until at least MIN_MEMBERS people have linked a Bantu. A period that fewer than
// MIN_MEMBERS people sent counts for is hidden too, so a total can't stand in for a person.

import type { Db } from "./db.ts";
import { COUNT_FIELDS, type Counts } from "./daemons.ts";

export const MIN_MEMBERS = 3;
export const WINDOWS = [7, 30] as const;

export type TotalsWindow = { days: number; since: string; contributors: number; counts: Counts | null };
export type TeamTotals = { linkedMembers: number; visible: boolean; windows: TotalsWindow[] };

const DAY_MS = 24 * 60 * 60 * 1000;

export async function linkedMemberCount(db: Db, orgId: string): Promise<number> {
  const row = await db.get<{ n: number }>(
    `SELECT COUNT(DISTINCT d.user_id) AS n FROM daemons d
     JOIN members m ON m.org_id = d.org_id AND m.user_id = d.user_id
     WHERE d.org_id = ? AND d.revoked_at IS NULL`,
    orgId,
  );
  return row?.n ?? 0;
}

export async function teamTotals(db: Db, now: number, orgId: string): Promise<TeamTotals> {
  const linkedMembers = await linkedMemberCount(db, orgId);
  if (linkedMembers < MIN_MEMBERS) return { linkedMembers, visible: false, windows: [] };
  const windows: TotalsWindow[] = [];
  for (const days of WINDOWS) {
    // The window includes today (UTC), so 7 days is today and the 6 before it.
    const since = new Date(now - (days - 1) * DAY_MS).toISOString().slice(0, 10);
    const row = await db.get<Counts & { contributors: number }>(
      `SELECT COUNT(DISTINCT d.user_id) AS contributors,
         ${COUNT_FIELDS.map((f) => `COALESCE(SUM(t.${f}), 0) AS ${f}`).join(", ")}
       FROM daily_totals t JOIN daemons d ON d.id = t.daemon_id
       WHERE t.org_id = ? AND t.day >= ?`,
      orgId,
      since,
    );
    const contributors = row?.contributors ?? 0;
    let counts: Counts | null = null;
    if (row && contributors >= MIN_MEMBERS) {
      counts = {} as Counts;
      for (const f of COUNT_FIELDS) counts[f] = Number(row[f]);
    }
    windows.push({ days, since, contributors, counts });
  }
  return { linkedMembers, visible: true, windows };
}
