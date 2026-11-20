import { useMemo, type CSSProperties } from "react";
import type { InboxItem, PermissionRequest, Repo, Session, Workspace, WorkspacePr } from "@bantu/protocol";
import { LANES, place, type Lane, type Placement } from "../lib/lanes.ts";

type Card = { workspace: Workspace; placement: Placement; title: string | undefined; pr: WorkspacePr | undefined };

const DOT: Record<Lane, string> = {
  needs_you: "bg-warn",
  working: "bg-accent",
  in_review: "bg-faint",
  ready: "bg-ok",
  idle: "bg-line",
};

/** Every workspace across every repo, in lanes worked out from what's actually happening. */
export function FleetBoard({
  repos,
  workspaces,
  sessionsByWorkspace,
  permissions,
  inbox,
  pullRequests,
  onOpen,
}: {
  repos: Record<string, Repo>;
  workspaces: Workspace[];
  sessionsByWorkspace: Record<string, Session[]>;
  permissions: PermissionRequest[];
  inbox: InboxItem[];
  pullRequests: Record<string, WorkspacePr>;
  onOpen: (workspaceId: string) => void;
}) {
  const lanes = useMemo(() => {
    const out: Record<Lane, Card[]> = { needs_you: [], working: [], in_review: [], ready: [], idle: [] };
    for (const workspace of workspaces) {
      if (workspace.state === "archived") continue;
      const sessions = sessionsByWorkspace[workspace.id] ?? [];
      const pr = pullRequests[workspace.id];
      const placement = place({
        workspace,
        sessions,
        permissions: permissions.filter((p) => p.workspace_id === workspace.id),
        questions: inbox.filter((i) => i.workspace_id === workspace.id && i.kind === "question"),
        pr,
      });
      const title = workspace.task ? `${workspace.task.key} ${workspace.task.title}` : [...sessions].reverse().find((s) => s.title !== "New chat")?.title;
      out[placement.lane].push({ workspace, placement, title, pr });
    }
    for (const list of Object.values(out)) list.sort((a, b) => b.workspace.created_at.localeCompare(a.workspace.created_at));
    return out;
  }, [workspaces, sessionsByWorkspace, permissions, inbox, pullRequests]);

  // Lanes with cards get the room; empty ones stay narrow but keep their place,
  // so a workspace moving across never makes the columns jump around.
  const columns = LANES.map(({ lane }) => (lanes[lane].length > 0 ? "minmax(13rem,1fr)" : "minmax(9.5rem,0.55fr)")).join(" ");
  const waiting = lanes.needs_you.length;
  const summary =
    waiting > 0
      ? `${waiting} ${waiting === 1 ? "workspace needs" : "workspaces need"} you`
      : lanes.working.length > 0
        ? `${lanes.working.length} ${lanes.working.length === 1 ? "agent" : "agents"} working, nothing waiting on you`
        : "Nothing is waiting on you";

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-12 items-center gap-3 border-b border-line px-5 max-md:px-3">
        <h2 className="font-medium">Board</h2>
        <p className="min-w-0 truncate text-[12.5px] text-muted" title="Every workspace, placed by what's happening: approvals, agents at work, CI and reviews.">
          {summary}
        </p>
      </div>
      {/* All five lanes fit side by side on a laptop; phones swipe between them. */}
      <div
        className="grid min-h-0 flex-1 grid-flow-col auto-cols-[85%] snap-x gap-3 overflow-x-auto p-3 md:grid-flow-row md:grid-cols-(--lanes) md:p-4 md:transition-[grid-template-columns] md:duration-300 md:ease-[var(--ease-out-ui)]"
        style={{ "--lanes": columns } as CSSProperties}
      >
        {LANES.map(({ lane, title, empty }) => (
          <section key={lane} aria-label={title} className="flex min-h-0 snap-start flex-col rounded-lg bg-panel/60">
            <h3 className="flex items-center gap-2 px-3 pt-3 pb-2 text-[12px] font-medium uppercase tracking-wide text-muted">
              <span aria-hidden className={`size-1.5 rounded-full ${DOT[lane]}`} />
              {title}
              <span className="tabular-nums text-faint">{lanes[lane].length}</span>
            </h3>
            <ul className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-2 pb-2">
              {lanes[lane].length === 0 && empty && <li className="px-1.5 py-2 text-[12px] text-faint">{empty}</li>}
              {lanes[lane].map(({ workspace, placement, title, pr }) => (
                <li key={workspace.id} className="enter-rise">
                  <button
                    type="button"
                    onClick={() => onOpen(workspace.id)}
                    className={`flex w-full flex-col gap-1 rounded-lg border bg-panel px-3 py-2.5 text-left shadow-sm shadow-black/20 ${
                      lane === "needs_you" ? "border-warn/35 hover:border-warn/60" : "border-line hover:border-faint"
                    }`}
                  >
                    <span className="flex items-baseline gap-2">
                      <span className="truncate font-medium">{workspace.name}</span>
                      <span className="ml-auto shrink-0 truncate text-[11.5px] text-faint">{repos[workspace.repo_id]?.name}</span>
                    </span>
                    {title && (
                      <span className="line-clamp-2 text-[12.5px] text-muted" title={title}>
                        {title}
                      </span>
                    )}
                    <span className={`text-[12px] ${lane === "needs_you" ? "text-warn" : "text-faint"}`}>{placement.reason}</span>
                    {pr && pr.pull_request.state === "OPEN" && <Checks pr={pr} />}
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}

function Checks({ pr }: { pr: WorkspacePr }) {
  const p = pr.pull_request;
  const total = p.checks_passing + p.checks_failing + p.checks_pending;
  if (total === 0) return null;
  return (
    <span className="flex items-center gap-2 text-[11.5px] tabular-nums text-faint" aria-label={`${p.checks_passing} checks passing, ${p.checks_failing} failing, ${p.checks_pending} running`}>
      {p.checks_passing > 0 && <span className="text-ok">✓ {p.checks_passing}</span>}
      {p.checks_failing > 0 && <span className="text-bad">✗ {p.checks_failing}</span>}
      {p.checks_pending > 0 && <span>… {p.checks_pending}</span>}
    </span>
  );
}
