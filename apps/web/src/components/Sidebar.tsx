import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
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
  onOpenLoops: (repoId: string) => void;
  onOpenTasks: (repoId: string) => void;
  onOpenImport: () => void;
  onShowBoard: () => void;
  boardSelected: boolean;
};

export function Sidebar(props: Props) {
  const { repos, workspacesByRepo, selectedId, showArchived } = props;
  return (
    <aside
      className={`absolute inset-y-0 left-0 z-30 flex w-[min(19rem,85vw)] min-h-0 flex-col border-r border-line bg-panel transition-transform will-change-transform md:static md:z-auto md:row-span-1 md:w-auto md:translate-x-0 md:transition-none ${
        props.open ? "translate-x-0 duration-[280ms] ease-[var(--ease-out-ui)]" : "-translate-x-full duration-200 ease-[var(--ease-exit)]"
      }`}
    >
      <div className="flex items-center justify-between px-3 py-2.5">
        <span className="flex items-center gap-2 font-semibold tracking-tight">
          <img src="./icon.svg" alt="" width={22} height={22} className="-my-1" />
          Bantu
        </span>
        <button type="button" onClick={props.onClose} aria-label="Close the workspace list" className="flex size-8 items-center justify-center rounded-md text-muted hover:bg-raised hover:text-fg md:hidden">
          ✕
        </button>
      </div>
      <AddRepo onAdd={props.onAddRepo} client={props.client} />
      {repos.length > 0 && (
        <button
          type="button"
          onClick={props.onShowBoard}
          aria-current={props.boardSelected ? "page" : undefined}
          className={`mx-1.5 mt-2 flex items-center gap-2 rounded-md px-2 py-1.5 text-left ${props.boardSelected ? "bg-raised" : "hover:bg-raised/60"}`}
        >
          <span aria-hidden className="flex gap-0.5">
            <span className="size-1.5 rounded-full bg-warn" />
            <span className="size-1.5 rounded-full bg-accent" />
            <span className="size-1.5 rounded-full bg-ok" />
          </span>
          Board
        </button>
      )}
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
            onOpenLoops={props.onOpenLoops}
            onOpenTasks={props.onOpenTasks}
          />
        ))}
      </nav>
      <div className="flex items-center gap-2 border-t border-line px-3 py-2 text-muted">
        <label className="flex flex-1 items-center gap-2">
          <input type="checkbox" checked={showArchived} onChange={props.onToggleArchived} />
          Show archived
        </label>
        <button type="button" onClick={props.onOpenImport} className="rounded px-1.5 py-0.5 text-xs hover:bg-raised hover:text-fg" title="Bring workspaces and chats over from Conductor or Pane">
          Import…
        </button>
      </div>
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
  onOpenLoops,
  onOpenTasks,
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
  onOpenLoops: (repoId: string) => void;
  onOpenTasks: (repoId: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [showFrom, setShowFrom] = useState(false);
  return (
    <section className="mt-2">
      <div className="flex items-center gap-0.5 py-1 pr-0.5 pl-2">
        <span
          className="min-w-0 flex-1 truncate text-xs font-medium uppercase tracking-wide text-muted"
          title={repo.github_account ? `${repo.root_path}\nGitHub: ${repo.github_account}` : repo.root_path}
        >
          {repo.name}
        </span>
        <IconButton label="Tasks from GitHub Issues, Jira and ClickUp" onClick={() => onOpenTasks(repo.id)}>
          <path d="M3 4.5l1.2 1.2L6.5 3.4M3 10.5l1.2 1.2 2.3-2.3M8.5 4.5H13M8.5 10.5H13" />
        </IconButton>
        <IconButton label="Loops: agents that start on a schedule or from GitHub" onClick={() => onOpenLoops(repo.id)}>
          <path d="M12.5 6.5A4.6 4.6 0 0 0 4 4.8M3.5 9.5A4.6 4.6 0 0 0 12 11.2M4 2.5v2.5h2.5M12 13.5V11H9.5" />
        </IconButton>
        <IconButton
          label="New workspace"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await onNewWorkspace(repo.id);
            } finally {
              setBusy(false);
            }
          }}
        >
          <path d="M8 3.5v9M3.5 8h9" />
        </IconButton>
        <RepoMenu
          onNewFrom={() => setShowFrom(true)}
          onRemove={() => {
            if (confirm(`Remove ${repo.name} from Bantu? Files on disk are not touched.`)) void onRemoveRepo(repo.id);
          }}
        />
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
              className={`flex w-full items-center gap-2 rounded-md py-1.5 pr-2 text-left ${w.parent_id ? "pl-5" : "pl-2"} ${
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
                  {w.review_state === "approved" && (
                    <span role="img" aria-label="reviewed and ready" title="Automatic review approved this work" className="text-[11px] text-ok">
                      ✓
                    </span>
                  )}
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
        <button type="submit" disabled={busy || !path.trim()} className="pressable rounded-md bg-accent px-2.5 py-1 font-medium text-bg disabled:opacity-40">
          {busy ? "Adding…" : "Add"}
        </button>
        <button type="button" onClick={() => setOpen(false)} className="rounded-md px-2.5 py-1 text-muted hover:bg-raised">
          Cancel
        </button>
      </div>
    </form>
  );
}

/** A small square button for the repo header; the label doubles as its tooltip. */
function IconButton({ label, onClick, disabled, children }: { label: string; onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      title={label}
      className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted hover:bg-raised hover:text-fg disabled:opacity-40 max-md:size-9"
    >
      <svg aria-hidden viewBox="0 0 16 16" className="size-3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        {children}
      </svg>
    </button>
  );
}

/** The repo's less frequent actions, out of the way until asked for. */
function RepoMenu({ onNewFrom, onRemove }: { onNewFrom: () => void; onRemove: () => void }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        root.current?.querySelector<HTMLButtonElement>("button")?.focus();
      }
    };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", escape);
    // Keyboard users land on the first item.
    root.current?.querySelector<HTMLButtonElement>("[role=menuitem]")?.focus();
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);
  const item = "block w-full rounded px-2.5 py-1.5 text-left hover:bg-raised focus-visible:bg-raised";
  return (
    <div ref={root} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-label="More for this repository"
        aria-haspopup="menu"
        aria-expanded={open}
        title="More"
        className={`flex size-7 items-center justify-center rounded-md text-muted hover:bg-raised hover:text-fg max-md:size-9 ${open ? "bg-raised text-fg" : ""}`}
      >
        <svg aria-hidden viewBox="0 0 16 16" className="size-3.5" fill="currentColor">
          <circle cx="3.5" cy="8" r="1.2" />
          <circle cx="8" cy="8" r="1.2" />
          <circle cx="12.5" cy="8" r="1.2" />
        </svg>
      </button>
      {open && (
        <div
          role="menu"
          className="menu-in absolute top-full right-0 z-40 mt-1 w-60 origin-top-right rounded-lg border border-line bg-raised/95 p-1 text-[12.5px] shadow-xl shadow-black/40 backdrop-blur-sm"
        >
          <button
            type="button"
            role="menuitem"
            className={item}
            onClick={() => {
              setOpen(false);
              onNewFrom();
            }}
          >
            New from a branch, PR or issue…
          </button>
          <div className="my-1 h-px bg-line" />
          <button
            type="button"
            role="menuitem"
            className={`${item} text-bad`}
            onClick={() => {
              setOpen(false);
              onRemove();
            }}
          >
            Remove repository…
          </button>
        </div>
      )}
    </div>
  );
}
