import { useCallback, useEffect, useState } from "react";
import type { ChildStatus, ChildWorkspace, Command, CommandResponse, IntegrationResult, Workspace } from "@bantu/protocol";
import type { DaemonClient } from "../lib/api.ts";

const POLL_MS = 3000;

const STATUS: Record<ChildStatus, { label: string; dot: string }> = {
  starting: { label: "Starting", dot: "bg-faint" },
  working: { label: "Working", dot: "bg-accent animate-pulse" },
  waiting: { label: "Needs you", dot: "bg-warn" },
  idle: { label: "Finished", dot: "bg-ok" },
  failed: { label: "Failed", dot: "bg-bad" },
};

/** A lead workspace's children, live, with one click to merge their work. */
export function BoardPanel({
  workspace,
  client,
  run,
  activeSessionId,
  onOpen,
  onAgentTookOver,
}: {
  workspace: Workspace;
  client: DaemonClient;
  run: (command: Command) => Promise<CommandResponse | null>;
  /** The chat that takes over if a merge conflicts. */
  activeSessionId: string | undefined;
  onOpen: (workspaceId: string) => void;
  onAgentTookOver: () => void;
}) {
  const [children, setChildren] = useState<ChildWorkspace[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<IntegrationResult | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setChildren(await client.children(workspace.id));
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [client, workspace.id]);

  useEffect(() => {
    void load();
    const timer = setInterval(load, POLL_MS);
    return () => clearInterval(timer);
  }, [load]);

  // Merging takes commits, so uncommitted work has to wait.
  const finished = children?.filter((c) => c.status === "idle" && c.ahead > 0 && c.uncommitted === 0).length ?? 0;

  const integrate = async () => {
    setBusy(true);
    setResult(null);
    try {
      const res = await run({ type: "workspace.integrate", workspace_id: workspace.id, session_id: activeSessionId ?? null });
      const outcome = res?.result as IntegrationResult | undefined;
      if (outcome) {
        setResult(outcome);
        if (outcome.sent_to_agent) onAgentTookOver();
      }
      await load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex max-w-3xl flex-col gap-3 px-5 py-4 max-md:px-3">
        <div className="flex items-center gap-3">
          <p className="min-w-0 flex-1 text-muted">
            Workspaces this one's agent started. Merging brings their commits into <span className="font-mono">{workspace.branch}</span>, locally.
          </p>
          <button
            type="button"
            disabled={busy || finished === 0}
            onClick={() => void integrate()}
            title="Merge finished workspaces into this branch. Nothing is pushed."
            className="shrink-0 rounded-md bg-accent px-3 py-1.5 text-[12px] font-medium text-bg disabled:opacity-40"
          >
            {busy ? "Merging…" : `Merge finished${finished > 0 ? ` (${finished})` : ""}`}
          </button>
        </div>
        {error && <p className="rounded-md border border-bad/40 bg-bad/10 px-3 py-2 text-bad">{error}</p>}
        {result && <Outcome result={result} />}
        {children?.length === 0 && <p className="rounded-md border border-line bg-panel px-3 py-6 text-center text-muted">This workspace hasn't started any others.</p>}
        <ul className="flex flex-col gap-2">
          {children?.map((c) => (
            <li key={c.workspace.id} className="rounded-md border border-line bg-panel p-3">
              <div className="flex items-center gap-2">
                <span aria-hidden className={`size-2 shrink-0 rounded-full ${STATUS[c.status].dot}`} />
                <button type="button" onClick={() => onOpen(c.workspace.id)} className="min-w-0 truncate font-medium underline-offset-2 hover:underline">
                  {c.workspace.name}
                </button>
                <span className="shrink-0 text-[12px] text-muted">{STATUS[c.status].label}</span>
                {c.conflicted && <span className="shrink-0 text-[12px] text-warn">⚠ Conflicts</span>}
                <span className="flex-1" />
                <span className="shrink-0 text-[12px] tabular-nums text-muted">
                  {c.files} file{c.files === 1 ? "" : "s"} <span className="text-ok">+{c.additions}</span> <span className="text-bad">−{c.deletions}</span>
                </span>
              </div>
              {c.title && <p className="mt-1 truncate text-[12.5px]" title={c.title}>{c.title}</p>}
              <p className="mt-0.5 text-[12px] tabular-nums text-faint">
                <span className="font-mono">{c.workspace.branch}</span> · {c.ahead} commit{c.ahead === 1 ? "" : "s"}
                {c.uncommitted > 0 && <span className="text-warn"> · {c.uncommitted} uncommitted, so it can't merge yet</span>}
              </p>
              {c.last_reply && <p className="mt-1.5 line-clamp-3 whitespace-pre-wrap [overflow-wrap:anywhere] text-[12.5px] text-muted">{c.last_reply}</p>}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function Outcome({ result }: { result: IntegrationResult }) {
  return (
    <div role="status" className="rounded-md border border-line bg-panel px-3 py-2 text-[12.5px]">
      {result.merged.length > 0 && <p className="text-ok">Merged {result.merged.join(", ")}.</p>}
      {result.conflict && (
        <p className="text-warn">
          {result.conflict} conflicts with this branch, so that merge was undone.
          {result.sent_to_agent ? " The agent is taking it from here." : ""}
        </p>
      )}
      {result.skipped.map((s) => (
        <p key={s.workspace} className="text-muted">
          Skipped {s.workspace}: {s.reason}.
        </p>
      ))}
      {result.merged.length === 0 && !result.conflict && result.skipped.length === 0 && <p className="text-muted">Nothing to merge.</p>}
    </div>
  );
}
