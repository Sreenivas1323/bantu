import { useState, type FormEvent, type ReactNode } from "react";
import type { AgentStatus, Repo, Session, Workspace, WorkspaceSource } from "@bantu/protocol";
import { NewFromForm } from "./NewFromForm.tsx";
import { RepoSuggestions } from "./RepoSuggestions.tsx";
import type { DaemonClient } from "../lib/api.ts";
import type { ConnStatus } from "../lib/useDaemon.ts";
import { StateDot } from "./StateDot.tsx";
import { Icon, IconButton, Menu, MenuItem, MenuSeparator } from "./Menu.tsx";
import { SidebarFooter } from "./SidebarFooter.tsx";

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
  /** Questions, notices and approvals waiting for the user. */
  inboxCount: number;
  onOpenInbox: () => void;
  status: ConnStatus;
  agents: AgentStatus[] | null;
  onOpenAccounts: () => void;
  onDisconnect: () => void;
  daemonVersion: string | null;
  onError: (message: string) => void;
};

const CONNECTION: Partial<Record<ConnStatus, { label: string; tone: string }>> = {
  connecting: { label: "Connecting…", tone: "bg-warn" },
  reconnecting: { label: "Reconnecting…", tone: "bg-warn animate-pulse" },
  unauthorized: { label: "Token rejected", tone: "bg-bad" },
  signin: { label: "Sign-in expired", tone: "bg-warn" },
};

/** The latest named chat says more about the work than the branch does. */
function titleOf(w: Workspace, sessions: Session[]) {
  if (w.task) return `${w.task.key} ${w.task.title}`;
  return [...sessions].reverse().find((s) => s.title !== "New chat")?.title ?? w.name;
}

export function Sidebar(props: Props) {
  const { repos, workspacesByRepo, selectedId, showArchived } = props;
  const [query, setQuery] = useState("");
  const [adding, setAdding] = useState(false);
  const q = query.trim().toLowerCase();
  const matches = (w: Workspace) => `${w.name} ${w.branch} ${titleOf(w, props.sessionsByWorkspace[w.id] ?? [])}`.toLowerCase().includes(q);
  const connection = CONNECTION[props.status];

  return (
    <aside
      className={`absolute inset-y-0 left-0 z-30 flex w-[min(19rem,85vw)] min-h-0 flex-col border-r border-line bg-panel transition-transform will-change-transform md:static md:z-auto md:w-auto md:translate-x-0 md:transition-none ${
        props.open ? "translate-x-0 duration-[280ms] ease-[var(--ease-out-ui)]" : "-translate-x-full duration-200 ease-[var(--ease-exit)]"
      }`}
    >
      <div className="flex h-12 shrink-0 items-center gap-2 px-3.5">
        <img src="./icon.svg" alt="" width={22} height={22} />
        <span className="font-semibold tracking-tight">Bantu</span>
        {connection && (
          <span className="flex min-w-0 items-center gap-1.5 truncate text-[12px] text-muted" role="status">
            <span className={`size-1.5 shrink-0 rounded-full ${connection.tone}`} />
            {connection.label}
          </span>
        )}
        <span className="flex-1" />
        <button type="button" onClick={props.onClose} aria-label="Close the workspace list" className="flex size-9 items-center justify-center rounded-md text-muted hover:bg-raised hover:text-fg md:hidden">
          <Icon className="size-3.5">
            <path d="M4 4l8 8M12 4l-8 8" />
          </Icon>
        </button>
      </div>

      <nav aria-label="App" className="flex flex-col gap-px px-2 pb-2">
        <NavItem
          label="Board"
          current={props.boardSelected}
          onClick={props.onShowBoard}
          icon={<path d="M2.5 3.5h3v9h-3zM6.5 3.5h3v6h-3zM10.5 3.5h3v4h-3z" />}
          count={props.waitingWorkspaces.size || undefined}
        />
        <NavItem
          label="Inbox"
          onClick={props.onOpenInbox}
          icon={<path d="M2.5 9.5l1.6-5.2a1 1 0 0 1 1-.8h5.8a1 1 0 0 1 1 .8l1.6 5.2M2.5 9.5v3h11v-3H10a2 2 0 0 1-4 0z" />}
          count={props.inboxCount || undefined}
        />
        <label className="mt-1 flex items-center gap-2.5 rounded-md border border-transparent px-2 py-1 text-muted focus-within:border-line focus-within:bg-bg hover:bg-raised/60 focus-within:hover:bg-bg">
          <Icon className="size-4">
            <circle cx="7" cy="7" r="4.2" />
            <path d="M10.2 10.2l3.3 3.3" />
          </Icon>
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setQuery("")}
            placeholder="Search workspaces"
            aria-label="Search workspaces"
            className="min-w-0 flex-1 bg-transparent py-0.5 text-[16px] text-fg placeholder:text-muted focus:outline-none md:text-[13.5px]"
          />
        </label>
      </nav>

      <div className="flex items-center justify-between pt-2 pr-2 pb-1 pl-4">
        <h2 className="text-[12px] font-medium text-faint">Repositories</h2>
        <IconButton label="Add a repository" onClick={() => setAdding((v) => !v)} pressed={adding}>
          <path d="M8 3.5v9M3.5 8h9" />
        </IconButton>
      </div>
      {adding && <AddRepo onAdd={props.onAddRepo} client={props.client} onDone={() => setAdding(false)} />}

      <nav aria-label="Workspaces" className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        {repos.length === 0 && !adding && (
          <button type="button" onClick={() => setAdding(true)} className="mx-2 mt-1 text-left text-muted hover:text-fg">
            Add your first repository →
          </button>
        )}
        {repos.map((repo) => {
          const all = (workspacesByRepo[repo.id] ?? []).filter((w) => showArchived || w.state !== "archived");
          const shown = q ? all.filter(matches) : all;
          if (q && shown.length === 0) return null;
          return (
            <RepoGroup
              key={repo.id}
              repo={repo}
              count={all.filter((w) => w.state !== "archived").length}
              workspaces={shown}
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
          );
        })}
        {q && !repos.some((r) => (workspacesByRepo[r.id] ?? []).some(matches)) && (
          <p className="px-2 py-3 text-muted">No workspace matches “{query.trim()}”.</p>
        )}
      </nav>

      <SidebarFooter
        agents={props.agents}
        onOpenAccounts={props.onOpenAccounts}
        showArchived={showArchived}
        onToggleArchived={props.onToggleArchived}
        onOpenImport={props.onOpenImport}
        onDisconnect={props.onDisconnect}
        daemonVersion={props.daemonVersion}
        client={props.client}
        onError={props.onError}
      />
    </aside>
  );
}

function NavItem({ label, icon, onClick, current, count }: { label: string; icon: ReactNode; onClick: () => void; current?: boolean; count?: number }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={current ? "page" : undefined}
      className={`flex items-center gap-2.5 rounded-md px-2 py-1.5 text-left ${current ? "bg-raised text-fg" : "text-fg/90 hover:bg-raised/60"}`}
    >
      <Icon className="size-4 text-muted">{icon}</Icon>
      <span className="flex-1">{label}</span>
      {count !== undefined && (
        <span className="min-w-5 rounded-full bg-warn/15 px-1.5 text-center text-[11.5px] font-medium text-warn tabular-nums">{count}</span>
      )}
    </button>
  );
}

function RepoGroup({
  repo,
  count,
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
  /** Live workspaces, whatever the search shows. */
  count: number;
  workspaces: Workspace[];
  sessionsByWorkspace: Record<string, Session[]>;
  waitingWorkspaces: Set<string>;
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
  const [collapsed, setCollapsed] = useState(false);
  return (
    <section className="mt-1.5">
      {/* The extra actions wait until the row is hovered or focused; touch screens always show them. */}
      <div className="group flex items-center gap-0.5 rounded-md py-0.5 pr-0.5 pl-1">
        <button
          type="button"
          onClick={() => setCollapsed((v) => !v)}
          aria-expanded={!collapsed}
          title={repo.github_account ? `${repo.root_path}\nGitHub: ${repo.github_account}` : repo.root_path}
          className="flex min-w-0 flex-1 items-center gap-1.5 rounded px-1 py-1 text-left"
        >
          <Icon className={`size-3 text-faint transition-transform duration-150 ease-out ${collapsed ? "-rotate-90" : ""}`}>
            <path d="M4 6l4 4 4-4" />
          </Icon>
          <span className="truncate font-medium">{repo.name}</span>
          {count > 0 && <span className="shrink-0 text-[12px] text-faint tabular-nums">{count}</span>}
        </button>
        <div className="flex items-center gap-0.5 opacity-0 transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100 max-md:opacity-100 [@media(hover:none)]:opacity-100">
          <IconButton label="Tasks from GitHub Issues, Jira and ClickUp" onClick={() => onOpenTasks(repo.id)}>
            <path d="M3 4.5l1.2 1.2L6.5 3.4M3 10.5l1.2 1.2 2.3-2.3M8.5 4.5H13M8.5 10.5H13" />
          </IconButton>
          <IconButton label="Loops: agents that start on a schedule or from GitHub" onClick={() => onOpenLoops(repo.id)}>
            <path d="M12.5 6.5A4.6 4.6 0 0 0 4 4.8M3.5 9.5A4.6 4.6 0 0 0 12 11.2M4 2.5v2.5h2.5M12 13.5V11H9.5" />
          </IconButton>
          <Menu label={`More for ${repo.name}`}>
            {(close) => (
              <>
                <MenuItem
                  onSelect={() => {
                    close();
                    setShowFrom(true);
                  }}
                >
                  New from a branch, PR or issue…
                </MenuItem>
                <MenuSeparator />
                <MenuItem
                  danger
                  onSelect={() => {
                    close();
                    if (confirm(`Remove ${repo.name} from Bantu? Files on disk are not touched.`)) void onRemoveRepo(repo.id);
                  }}
                >
                  Remove repository…
                </MenuItem>
              </>
            )}
          </Menu>
        </div>
        {/* New is the common action, so it stays visible. */}
        <IconButton
          label={`New workspace in ${repo.name}`}
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            setCollapsed(false);
            try {
              await onNewWorkspace(repo.id);
            } finally {
              setBusy(false);
            }
          }}
        >
          <path d="M8 3.5v9M3.5 8h9" />
        </IconButton>
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
      {!collapsed && (
        <ul className="mt-px flex flex-col gap-px">
          {workspaces.map((w) => {
            const sessions = sessionsByWorkspace[w.id] ?? [];
            const running = sessions.some((s) => s.status === "running");
            const selected = w.id === selectedId;
            return (
              <li key={w.id}>
                <button
                  type="button"
                  onClick={() => onSelect(w.id)}
                  aria-current={selected ? "page" : undefined}
                  title={`${w.name} · ${w.branch}`}
                  className={`flex w-full items-center gap-2.5 rounded-md py-1.5 pr-2 text-left ${w.parent_id ? "pl-8" : "pl-[1.4rem]"} ${
                    selected ? "bg-raised text-fg" : "text-fg/85 hover:bg-raised/60"
                  }`}
                >
                  {waitingWorkspaces.has(w.id) ? (
                    <span role="img" aria-label="needs you" className="size-2 shrink-0 rounded-full bg-warn" />
                  ) : running ? (
                    <span role="img" aria-label="agent working" className="relative inline-flex size-2 shrink-0">
                      <span className="absolute inline-flex size-full animate-ping rounded-full bg-accent opacity-60" />
                      <span className="relative inline-flex size-2 rounded-full bg-accent" />
                    </span>
                  ) : w.state === "ready" ? (
                    <span aria-hidden className="size-2 shrink-0 rounded-full border border-faint" />
                  ) : (
                    <StateDot state={w.state} />
                  )}
                  <span className={`min-w-0 flex-1 truncate ${w.state === "archived" ? "text-faint" : ""}`}>{titleOf(w, sessions)}</span>
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
                </button>
              </li>
            );
          })}
          {workspaces.length === 0 && <li className="py-1 pl-[1.4rem] text-[12.5px] text-faint">No workspaces yet</li>}
        </ul>
      )}
    </section>
  );
}

function AddRepo({ onAdd, client, onDone }: { onAdd: (path: string) => Promise<void>; client: DaemonClient; onDone: () => void }) {
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!path.trim()) return;
    setBusy(true);
    try {
      await onAdd(path.trim());
      setPath("");
      onDone();
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="reveal space-y-1.5 px-3 pb-2">
      <input
        value={path}
        onChange={(e) => setPath(e.target.value)}
        onKeyDown={(e) => e.key === "Escape" && onDone()}
        placeholder="~/code/app, owner/repo or a git URL"
        spellCheck={false}
        autoFocus
        className="w-full rounded-md border border-line bg-bg px-2 py-1.5 font-mono text-[16px] placeholder:text-faint sm:text-[12px]"
      />
      <RepoSuggestions
        client={client}
        filter={path}
        limit={6}
        onAdd={async (p) => {
          await onAdd(p);
          onDone();
        }}
      />
      <div className="flex gap-1.5">
        <button type="submit" disabled={busy || !path.trim()} className="pressable rounded-md bg-accent px-2.5 py-1 font-medium text-bg disabled:opacity-40">
          {busy ? "Adding…" : "Add"}
        </button>
        <button type="button" onClick={onDone} className="rounded-md px-2.5 py-1 text-muted hover:bg-raised">
          Cancel
        </button>
      </div>
    </form>
  );
}
