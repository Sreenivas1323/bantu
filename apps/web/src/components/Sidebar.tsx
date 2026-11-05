import { useState, type FormEvent } from "react";
import type { Repo, Session, Workspace, WorkspaceSource } from "@bantu/protocol";
import { NewFromForm } from "./NewFromForm.tsx";
import { RepoSuggestions } from "./RepoSuggestions.tsx";
import type { DaemonClient } from "../lib/api.ts";
import { StateDot } from "./StateDot.tsx";

type Props = {
  /** On phones the list is a drawer; on wider screens it's always shown. */
  open: boolean;
  onClose: () => void;
  repos: Repo[];
  workspacesByRepo: Record<string, Workspace[]>;
  sessionsByWorkspace: Record<string, Session[]>;
  /** Workspaces with an agent waiting for approval. */
  waitingWorkspaces: Set<string>;
  /** Workspaces the conflict radar flagged. */
  conflictedWorkspaces: Set<string>;
  selectedId: string | null;
  showArchived: boolean;
  onToggleArchived: () => void;
  onSelect: (workspaceId: string) => void;
  onAddRepo: (path: string) => Promise<void>;
  client: DaemonClient;
  onNewWorkspace: (repoId: string, from?: WorkspaceSource) => Promise<void>;
  onRemoveRepo: (repoId: string) => Promise<void>;
};

export function Sidebar(props: Props) {
  const { repos, workspacesByRepo, selectedId, showArchived } = props;
  return (
    <aside
      className={`absolute inset-y-0 left-0 z-30 flex w-[min(19rem,85vw)] min-h-0 flex-col border-r border-line bg-panel transition-transform duration-200 ease-out motion-reduce:transition-none md:static md:z-auto md:row-span-1 md:w-auto md:translate-x-0 md:transition-none ${
        props.open ? "translate-x-0" : "-translate-x-full"
      }`}
    >
      <div className="flex items-center justify-between px-3 py-2.5">
        <span className="font-semibold tracking-tight">Bantu</span>
        <button type="button" onClick={props.onClose} aria-label="Close the workspace list" className="flex size-8 items-center justify-center rounded-md text-muted hover:bg-raised hover:text-fg md:hidden">
          ✕
        </button>
      </div>
      <AddRepo onAdd={props.onAddRepo} client={props.client} />
      <nav className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-3">
        {repos.length === 0 && <p className="px-2 py-3 text-faint">No repositories yet.</p>}
        {repos.map((repo) => (
          <RepoGroup
            key={repo.id}
            repo={repo}
            workspaces={(workspacesByRepo[repo.id] ?? []).filter((w) => showArchived || w.state !== "archived")}
            sessionsByWorkspace={props.sessionsByWorkspace}
            waitingWorkspaces={props.waitingWorkspaces}
            conflictedWorkspaces={props.conflictedWorkspaces}
            selectedId={selectedId}
            onSelect={props.onSelect}
            onNewWorkspace={props.onNewWorkspace}
            onRemoveRepo={props.onRemoveRepo}
          />
        ))}
      </nav>
      <label className="flex items-center gap-2 border-t border-line px-3 py-2 text-muted">
        <input type="checkbox" checked={showArchived} onChange={props.onToggleArchived} />
        Show archived
      </label>
    </aside>
  );
}

function RepoGroup({
  repo,
  workspaces,
  sessionsByWorkspace,
  waitingWorkspaces,
  conflictedWorkspaces,
  selectedId,
  onSelect,
  onNewWorkspace,
  onRemoveRepo,
}: {
  repo: Repo;
  workspaces: Workspace[];
  sessionsByWorkspace: Record<string, Session[]>;
  /** Workspaces with an agent waiting for approval. */
  waitingWorkspaces: Set<string>;
  /** Workspaces the conflict radar flagged. */
  conflictedWorkspaces: Set<string>;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onNewWorkspace: (repoId: string, from?: WorkspaceSource) => Promise<void>;
  onRemoveRepo: (repoId: string) => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [showFrom, setShowFrom] = useState(false);
  return (
    <section className="mt-2">
      <div className="group flex items-center gap-1 px-2 py-1">
        <span className="min-w-0 flex-1 truncate text-xs font-medium uppercase tracking-wide text-muted" title={repo.root_path}>
          {repo.name}
        </span>
        <button
          type="button"
          onClick={() => {
            if (confirm(`Remove ${repo.name} from Bantu? Files on disk are not touched.`)) void onRemoveRepo(repo.id);
          }}
          className="rounded px-1 text-xs text-faint opacity-0 hover:text-bad group-hover:opacity-100 focus-visible:opacity-100 max-md:opacity-100"
          title="Remove repository"
        >
          Remove
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await onNewWorkspace(repo.id);
            } finally {
              setBusy(false);
            }
          }}
          className="rounded px-1.5 py-0.5 text-xs text-muted hover:bg-raised hover:text-fg disabled:opacity-40"
          title="New workspace"
        >
          + New
        </button>
        <button
          type="button"
          onClick={() => setShowFrom((v) => !v)}
          aria-expanded={showFrom}
          className="rounded px-1 py-0.5 text-xs text-muted hover:bg-raised hover:text-fg"
          title="New workspace from a branch, pull request or issue"
        >
          …
        </button>
      </div>
      {showFrom && (
        <NewFromForm
          onCancel={() => setShowFrom(false)}
          onCreate={async (from) => {
            await onNewWorkspace(repo.id, from);
            setShowFrom(false);
          }}
        />
      )}
      <ul>
        {workspaces.map((w) => {
          const sessions = sessionsByWorkspace[w.id] ?? [];
          const running = sessions.some((s) => s.status === "running");
          // The latest named chat says more about the work than the branch does.
          const title = [...sessions].reverse().find((s) => s.title !== "New chat")?.title;
          return (
          <li key={w.id}>
            <button
              type="button"
              onClick={() => onSelect(w.id)}
              className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left ${
                w.id === selectedId ? "bg-raised" : "hover:bg-raised/60"
              }`}
            >
              {waitingWorkspaces.has(w.id) ? (
                <span role="img" aria-label="needs your approval" title="Needs your approval" className="size-2 shrink-0 rounded-full bg-warn" />
              ) : running ? (
                <span role="img" aria-label="agent working" title="Agent working" className="relative inline-flex size-2 shrink-0">
                  <span className="absolute inline-flex size-full animate-ping rounded-full bg-accent opacity-60" />
                  <span className="relative inline-flex size-2 rounded-full bg-accent" />
                </span>
              ) : (
                <StateDot state={w.state} />
              )}
              <span className="min-w-0 flex-1">
                <span className={`flex items-center gap-1 truncate ${w.state === "archived" ? "text-faint" : ""}`}>
                  {w.name}
                  {conflictedWorkspaces.has(w.id) && (
                    <span role="img" aria-label="will conflict" title="Will conflict with another workspace or its base" className="text-[11px] text-warn">
                      ⚠
                    </span>
                  )}
                </span>
                {title ? (
                  <span className="block truncate text-[11.5px] text-muted" title={title}>
                    {title}
                  </span>
                ) : (
                  <span className="block truncate font-mono text-[11px] text-faint">{w.branch}</span>
                )}
              </span>
            </button>
          </li>
          );
        })}
        {workspaces.length === 0 && <li className="px-2 py-1 text-faint">No workspaces</li>}
      </ul>
    </section>
  );
}

function AddRepo({ onAdd, client }: { onAdd: (path: string) => Promise<void>; client: DaemonClient }) {
  const [open, setOpen] = useState(false);
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);

  if (!open) {
    return (
      <div className="px-3 pb-2">
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="w-full rounded-md border border-line px-2 py-1.5 text-left text-muted hover:bg-raised hover:text-fg"
        >
          + Add repository
        </button>
      </div>
    );
  }

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!path.trim()) return;
    setBusy(true);
    try {
      await onAdd(path.trim());
      setPath("");
      setOpen(false);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="space-y-1.5 px-3 pb-2">
      <input
        value={path}
        onChange={(e) => setPath(e.target.value)}
        onKeyDown={(e) => e.key === "Escape" && setOpen(false)}
        placeholder="~/code/app, owner/repo or a git URL"
        spellCheck={false}
        autoFocus
        className="w-full rounded-md border border-line bg-bg px-2 py-1.5 font-mono text-[16px] sm:text-[12px] placeholder:text-faint"
      />
      <RepoSuggestions
        client={client}
        filter={path}
        limit={6}
        onAdd={async (p) => {
          await onAdd(p);
          setOpen(false);
        }}
      />
      <div className="flex gap-1.5">
        <button type="submit" disabled={busy || !path.trim()} className="rounded-md bg-accent px-2.5 py-1 font-medium text-bg disabled:opacity-40">
          {busy ? "Adding…" : "Add"}
        </button>
        <button type="button" onClick={() => setOpen(false)} className="rounded-md px-2.5 py-1 text-muted hover:bg-raised">
          Cancel
        </button>
      </div>
    </form>
  );
}
