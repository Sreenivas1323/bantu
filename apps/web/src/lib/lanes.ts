import type { InboxItem, PermissionRequest, Session, Workspace, WorkspacePr } from "@bantu/protocol";

/** The board's columns, in the order a person should look at them. */
export type Lane = "needs_you" | "working" | "in_review" | "ready" | "idle";

export const LANES: { lane: Lane; title: string; empty: string }[] = [
  { lane: "needs_you", title: "Needs you", empty: "Nothing is waiting on you." },
  { lane: "working", title: "Working", empty: "No agent is working right now." },
  { lane: "in_review", title: "In review", empty: "No pull requests waiting on CI or reviews." },
  { lane: "ready", title: "Ready to merge", empty: "Nothing is ready to merge yet." },
  { lane: "idle", title: "Idle", empty: "" },
];

export type Placement = { lane: Lane; reason: string };

export type Facts = {
  workspace: Workspace;
  sessions: Session[];
  permissions: PermissionRequest[];
  questions: InboxItem[];
  pr: WorkspacePr | undefined;
};

/**
 * Where a workspace belongs, worked out from facts rather than set by hand:
 * anything waiting on a person first, then live agents, then where its pull
 * request stands.
 */
export function place({ workspace, sessions, permissions, questions, pr }: Facts): Placement {
  if (workspace.state === "failed") return { lane: "needs_you", reason: "Setup failed" };
  if (permissions.length > 0) return { lane: "needs_you", reason: permissions.length === 1 ? "Waiting for your approval" : `${permissions.length} approvals waiting` };
  if (questions.length > 0) return { lane: "needs_you", reason: "Has a question for you" };
  if (pr?.needs_you) return { lane: "needs_you", reason: `PR #${pr.pull_request.number} still isn't ready after ${pr.follow_ups} follow-ups` };

  if (workspace.state === "creating") return { lane: "working", reason: "Setting up" };
  if (sessions.some((s) => s.status === "running")) {
    if (workspace.review_state === "reviewing") return { lane: "working", reason: "Another agent is reviewing" };
    return { lane: "working", reason: pr && pr.follow_ups > 0 ? `Following up on PR #${pr.pull_request.number}` : "Agent working" };
  }

  const p = pr?.pull_request;
  if (p && p.state === "OPEN") {
    const checks = p.checks_failing > 0 ? "failing" : p.checks_pending > 0 ? "pending" : "passing";
    if (p.is_draft) return { lane: "in_review", reason: `Draft PR #${p.number}` };
    if (pr.mergeable === false) return { lane: "in_review", reason: `PR #${p.number} has conflicts` };
    if (checks === "failing") return { lane: "in_review", reason: `CI failing on PR #${p.number}` };
    if (checks === "pending") return { lane: "in_review", reason: `CI running on PR #${p.number}` };
    if (p.review_decision === "CHANGES_REQUESTED") return { lane: "in_review", reason: `Changes requested on PR #${p.number}` };
    if (p.review_decision === "REVIEW_REQUIRED") return { lane: "in_review", reason: `PR #${p.number} waiting for review` };
    return { lane: "ready", reason: p.review_decision === "APPROVED" ? `PR #${p.number} approved, CI passing` : `PR #${p.number}, CI passing` };
  }

  if (workspace.review_state === "changes_requested") return { lane: "idle", reason: "Review asked for changes" };
  if (workspace.review_state === "approved") return { lane: "idle", reason: "Reviewed and ready for a PR" };
  if (p?.state === "MERGED") return { lane: "idle", reason: `PR #${p.number} merged` };
  if (p?.state === "CLOSED") return { lane: "idle", reason: `PR #${p.number} closed` };
  return { lane: "idle", reason: sessions.length > 0 ? "Agent finished" : "No chats yet" };
}
