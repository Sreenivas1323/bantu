import { expect, test } from "bun:test";
import type { PullRequest, Session, Workspace, WorkspacePr } from "@bantu/protocol";
import { place, type Facts } from "./lanes.ts";

const ws = (over: Partial<Workspace> = {}) => ({ id: "w", state: "ready", review_state: null, ...over }) as Workspace;
const session = (status: Session["status"]) => ({ id: "s", status }) as Session;
const pr = (p: Partial<PullRequest>, over: Partial<WorkspacePr> = {}): WorkspacePr => ({
  workspace_id: "w",
  pull_request: { number: 4, url: "", title: "", state: "OPEN", is_draft: false, review_decision: null, checks_passing: 2, checks_failing: 0, checks_pending: 0, ...p },
  mergeable: true,
  head_sha: "a",
  updated_at: null,
  follow_ups: 0,
  needs_you: false,
  ...over,
});
const facts = (over: Partial<Facts>): Facts => ({ workspace: ws(), sessions: [], permissions: [], questions: [], pr: undefined, ...over });

test("anything waiting on a person comes first", () => {
  expect(place(facts({ sessions: [session("running")], permissions: [{} as never] })).lane).toBe("needs_you");
  expect(place(facts({ questions: [{} as never] })).reason).toBe("Has a question for you");
  expect(place(facts({ workspace: ws({ state: "failed" }) })).lane).toBe("needs_you");
  expect(place(facts({ pr: pr({}, { needs_you: true, follow_ups: 3 }) })).reason).toContain("after 3 follow-ups");
});

test("a working agent beats its pull request's state", () => {
  expect(place(facts({ sessions: [session("running")], pr: pr({ checks_failing: 1 }) })).lane).toBe("working");
  expect(place(facts({ sessions: [session("running")], pr: pr({}, { follow_ups: 1 }) })).reason).toBe("Following up on PR #4");
  expect(place(facts({ workspace: ws({ state: "creating" }) })).lane).toBe("working");
});

test("pull requests are in review until everything is green", () => {
  expect(place(facts({ pr: pr({ checks_failing: 1 }) })).reason).toBe("CI failing on PR #4");
  expect(place(facts({ pr: pr({ checks_pending: 1 }) })).reason).toBe("CI running on PR #4");
  expect(place(facts({ pr: pr({}, { mergeable: false }) })).reason).toBe("PR #4 has conflicts");
  expect(place(facts({ pr: pr({ is_draft: true }) })).lane).toBe("in_review");
  expect(place(facts({ pr: pr({ review_decision: "CHANGES_REQUESTED" }) })).lane).toBe("in_review");
  expect(place(facts({ pr: pr({ review_decision: "REVIEW_REQUIRED" }) })).lane).toBe("in_review");
  expect(place(facts({ pr: pr({ review_decision: "APPROVED" }) }))).toEqual({ lane: "ready", reason: "PR #4 approved, CI passing" });
  expect(place(facts({ pr: pr({}) })).lane).toBe("ready");
  // GitHub hasn't worked out mergeability yet: not a reason to hold it back.
  expect(place(facts({ pr: pr({}, { mergeable: null }) })).lane).toBe("ready");
});

test("everything else is idle, with a reason", () => {
  expect(place(facts({ pr: pr({ state: "MERGED" }) }))).toEqual({ lane: "idle", reason: "PR #4 merged" });
  expect(place(facts({ workspace: ws({ review_state: "approved" }) })).reason).toBe("Reviewed and ready for a PR");
  expect(place(facts({ sessions: [session("idle")] })).reason).toBe("Agent finished");
});
