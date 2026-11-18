import { expect, test } from "bun:test";
import type { Event, Org, StateSnapshot, Tracker } from "@bantu/protocol";
import { claudeGuidance, orgTrackers, syncLine } from "./org.ts";
import { emptyState, reducer } from "./store.ts";

const org: Org = {
  id: "org_acme",
  name: "Acme",
  slug: "acme",
  member: { login: "ana", role: "member" },
  connector: "https://connect.acme.dev",
  joined_at: "2026-11-18T09:00:00.000Z",
  last_synced_at: "2026-11-18T10:00:00.000Z",
  last_error: null,
  claude: { mode: "team", plan: "Claude Team", env_keys: [] },
};
const tracker = (id: string, org_id: string | null) => ({ id, org_id }) as Tracker;

test("team mode names the plan and asks for the work account", () => {
  const g = claudeGuidance("team", "Claude Team");
  expect(g.title).toBe("A seat on Claude Team");
  expect(g.body).toContain("work account");
  expect(claudeGuidance("api_key", null).body).toContain("don't need to sign in");
});

test("only the org's trackers count as managed", () => {
  const all = [tracker("a", "org_acme"), tracker("b", null)];
  expect(orgTrackers(all, org).map((t) => t.id)).toEqual(["a"]);
  expect(orgTrackers(all, null)).toEqual([]);
});

test("sync errors win over the last sync time", () => {
  expect(syncLine({ ...org, last_error: "couldn't reach it" })).toEqual({ text: "couldn't reach it", failed: true });
  expect(syncLine({ ...org, last_synced_at: null }).text).toBe("Not synced yet");
});

test("org.updated replaces the org and records why it ended", () => {
  const snapshot: StateSnapshot = {
    protocol_version: 1,
    daemon_version: "0",
    last_seq: 1,
    repos: [],
    workspaces: [],
    sessions: [],
    comments: [],
    terminals: [],
    permissions: [],
    inbox: [],
    conflicts: [],
    spotlights: [],
    loops: [],
    trackers: [],
    pull_requests: [],
    preferences: emptyState.preferences,
    org: null,
    org_ended: "You left",
  };
  let state = reducer(emptyState, { type: "snapshot", snapshot });
  expect(state.orgEnded).toBe("You left");
  const event = (seq: number, data: Event["data"]): Event => ({ seq, ts: "", data });
  state = reducer(state, { type: "event", event: event(2, { type: "org.updated", org, ended: null }) });
  expect(state.org?.name).toBe("Acme");
  state = reducer(state, { type: "event", event: event(3, { type: "org.updated", org: null, ended: "Acme removed this machine" }) });
  expect(state.org).toBeNull();
  expect(state.orgEnded).toBe("Acme removed this machine");
});
