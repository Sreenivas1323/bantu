import { useState } from "react";
import type { PullRequest, WorkspaceDiff } from "@bantu/protocol";

/** Branch position, uncommitted count, and the PR (or a button to create one). */
export function GitStatus({
  diff,
  pr,
  canCreatePr,
  createPrBlocked,
  onCreatePr,
}: {
  diff: WorkspaceDiff | null;
  pr: PullRequest | null;
  canCreatePr: boolean;
  /** Why Create PR is unavailable, shown as its tooltip. */
  createPrBlocked?: string;
  onCreatePr: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  return (
    <div className="flex items-center gap-2 text-[12px]">
      {diff && (
        <span className="flex items-center gap-2 tabular-nums text-muted" title={`Compared with ${diff.base_ref}`}>
          <span title="Commits ahead">↑{diff.ahead}</span>
          <span title="Commits behind">↓{diff.behind}</span>
          {diff.uncommitted > 0 && <span className="text-warn">{diff.uncommitted} uncommitted</span>}
        </span>
      )}
      {pr ? (
        <a
          href={pr.url}
          target="_blank"
          rel="noreferrer"
          className="flex items-center gap-1.5 rounded-md border border-line px-2 py-1 hover:bg-raised"
          title={pr.title}
        >
          <span className={prTone(pr)}>{pr.is_draft ? "Draft" : titleCase(pr.state)}</span>
          <span>#{pr.number}</span>
          <Checks pr={pr} />
        </a>
      ) : (
        <button
          type="button"
          disabled={!canCreatePr || busy}
          onClick={async () => {
            setBusy(true);
            try {
              await onCreatePr();
            } finally {
              setBusy(false);
            }
          }}
          title={
            canCreatePr
              ? "Ask the agent to commit, push and open a pull request"
              : (createPrBlocked ?? "Needs a ready workspace with an idle chat")
          }
          className="rounded-md border border-line px-2.5 py-1 text-muted hover:bg-raised hover:text-fg disabled:opacity-40"
        >
          Create PR
        </button>
      )}
    </div>
  );
}

function Checks({ pr }: { pr: PullRequest }) {
  const total = pr.checks_passing + pr.checks_failing + pr.checks_pending;
  if (total === 0) return null;
  if (pr.checks_failing > 0) return <span className="text-bad">✕ {pr.checks_failing} failing</span>;
  if (pr.checks_pending > 0) return <span className="text-warn">● {pr.checks_pending} running</span>;
  return <span className="text-ok">✓ checks</span>;
}

function prTone(pr: PullRequest) {
  if (pr.state === "MERGED") return "text-[oklch(0.72_0.15_300)]";
  if (pr.state === "CLOSED") return "text-bad";
  return pr.is_draft ? "text-muted" : "text-ok";
}

function titleCase(s: string) {
  return s.charAt(0) + s.slice(1).toLowerCase();
}
