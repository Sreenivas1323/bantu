import { useEffect, useMemo, useRef, useState } from "react";
import type { Command, Session, Workspace } from "@bantu/protocol";
import type { Connection } from "../lib/connection.ts";
import { useDaemon } from "../lib/useDaemon.ts";
import { conflictsFor } from "../lib/store.ts";
import { useNotifications } from "../lib/useNotifications.ts";
import { Sidebar } from "./Sidebar.tsx";
import { StatusBar } from "./StatusBar.tsx";
import { RepoSuggestions } from "./RepoSuggestions.tsx";
import type { DaemonClient } from "../lib/api.ts";
import { AccountsDialog } from "./AccountsDialog.tsx";
import { InboxDialog } from "./InboxDialog.tsx";
import { useAgents } from "../lib/useAgents.ts";
import { WorkspaceView } from "./WorkspaceView.tsx";

export function Shell({ conn, onDisconnect }: { conn: Connection; onDisconnect: (reason?: string) => void }) {
  const { state, status, send, loadHistory, client } = useDaemon(conn);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showArchived, setShowArchived] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { agents, refresh: refreshAgents } = useAgents(client);
  const [showAccounts, setShowAccounts] = useState(false);
  const [showInbox, setShowInbox] = useState(false);
  // Phones show one thing at a time: the workspace list slides over the page.
  const [navOpen, setNavOpen] = useState(true);

  useEffect(() => {
    if (status === "unauthorized") onDisconnect("The daemon rejected this token. Paste a fresh one.");
  }, [status, onDisconnect]);

  const repos = useMemo(() => Object.values(state.repos).sort((a, b) => a.name.localeCompare(b.name)), [state.repos]);
  const workspacesByRepo = useMemo(() => {
    const groups: Record<string, Workspace[]> = {};
    for (const w of Object.values(state.workspaces)) (groups[w.repo_id] ??= []).push(w);
    for (const [repoId, list] of Object.entries(groups)) {
      list.sort((a, b) => b.created_at.localeCompare(a.created_at));
      // Children sit right under the workspace that started them.
      const ids = new Set(list.map((w) => w.id));
      const nested: Workspace[] = [];
      const place = (w: Workspace) => {
        nested.push(w);
        for (const child of list) if (child.parent_id === w.id) place(child);
      };
      for (const w of list) if (!w.parent_id || !ids.has(w.parent_id)) place(w);
      groups[repoId] = nested;
    }
    return groups;
  }, [state.workspaces]);

  const sessionsByWorkspace = useMemo(() => {
    const groups: Record<string, Session[]> = {};
    for (const s of Object.values(state.sessions)) (groups[s.workspace_id] ??= []).push(s);
    for (const list of Object.values(groups)) list.sort((a, b) => a.created_at.localeCompare(b.created_at));
    return groups;
  }, [state.sessions]);

  // Backfill logs and transcripts the first time a workspace is opened.
  const loaded = useRef(new Set<string>());
  useEffect(() => {
    if (!selectedId || loaded.current.has(selectedId) || status !== "live") return;
    loaded.current.add(selectedId);
    loadHistory(selectedId).catch((err) => {
      loaded.current.delete(selectedId);
      setError(`Couldn't load history: ${err instanceof Error ? err.message : String(err)}`);
    });
  }, [selectedId, status, loadHistory]);

  const run = async (command: Command) => {
    setError(null);
    try {
      return await send(command);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      return null;
    }
  };

  // Workspaces where an agent is blocked on the user.
  const waitingWorkspaces = useMemo(
    () =>
      new Set([
        ...Object.values(state.permissions).map((p) => p.workspace_id),
        ...Object.values(state.inbox)
          .filter((i) => i.kind === "question")
          .map((i) => i.workspace_id),
      ]),
    [state.permissions, state.inbox],
  );
  const conflictedWorkspaces = useMemo(() => {
    const ids = new Set<string>();
    for (const list of Object.values(state.conflicts))
      for (const c of list) {
        ids.add(c.workspace_id);
        if (c.other_workspace_id) ids.add(c.other_workspace_id);
      }
    return ids;
  }, [state.conflicts]);
  useNotifications(state);

  const selected = selectedId ? state.workspaces[selectedId] : undefined;

  return (
    <div className="relative grid h-full grid-cols-1 grid-rows-[1fr_auto] md:grid-cols-[264px_minmax(0,1fr)]">
      {navOpen && (
        <button
          type="button"
          aria-label="Close the workspace list"
          onClick={() => setNavOpen(false)}
          className="absolute inset-0 z-20 bg-black/50 md:hidden"
        />
      )}
      <Sidebar
        open={navOpen}
        onClose={() => setNavOpen(false)}
        repos={repos}
        workspacesByRepo={workspacesByRepo}
        sessionsByWorkspace={sessionsByWorkspace}
        waitingWorkspaces={waitingWorkspaces}
        conflictedWorkspaces={conflictedWorkspaces}
        selectedId={selectedId}
        showArchived={showArchived}
        onToggleArchived={() => setShowArchived((v) => !v)}
        onSelect={(id) => {
          setSelectedId(id);
          setNavOpen(false);
        }}
        client={client}
        onAddRepo={async (path) => {
          await run({ type: "repo.add", path });
        }}
        onNewWorkspace={async (repoId, from) => {
          const res = await run({ type: "workspace.create", repo_id: repoId, name: null, base_ref: null, from: from ?? null });
          const created = res?.result as Workspace | undefined;
          if (created?.id) setSelectedId(created.id);
        }}
        onRemoveRepo={async (repoId) => {
          await run({ type: "repo.remove", repo_id: repoId });
        }}
      />
      <main className="flex min-h-0 min-w-0 flex-col overflow-hidden">
        <div className="flex items-center gap-2 border-b border-line bg-panel px-2 py-1.5 md:hidden">
          <button
            type="button"
            onClick={() => setNavOpen(true)}
            aria-label="Open the workspace list"
            className="flex size-9 items-center justify-center rounded-md text-lg text-muted hover:bg-raised hover:text-fg"
          >
            ☰
          </button>
          <span className="min-w-0 flex-1 truncate font-medium">{selected?.name ?? "Bantu"}</span>
          {waitingWorkspaces.size > 0 && (
            <button type="button" onClick={() => setShowInbox(true)} className="shrink-0 rounded-full bg-warn/15 px-2 py-0.5 text-[12px] font-medium text-warn">
              {waitingWorkspaces.size} waiting
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1">
        {selected ? (
          <WorkspaceView
            workspace={selected}
            repo={state.repos[selected.repo_id]}
            logs={state.logs[selected.id] ?? []}
            sessions={sessionsByWorkspace[selected.id] ?? []}
            chat={state.chat}
            streaming={state.streaming}
            comments={Object.values(state.comments).filter((c) => c.workspace_id === selected.id)}
            permissions={Object.values(state.permissions).filter((p) => p.workspace_id === selected.id)}
            questions={Object.values(state.inbox).filter((i) => i.workspace_id === selected.id && i.kind === "question")}
            conflicts={conflictsFor(state, selected.id)}
            agents={agents}
            onOpenAccounts={() => setShowAccounts(true)}
            allWorkspaces={state.workspaces}
            onSelectWorkspace={setSelectedId}
            loadouts={state.preferences.loadouts}
            spotlight={state.spotlights[selected.repo_id]}
            terminals={Object.values(state.terminals)
              .filter((t) => t.workspace_id === selected.id)
              .sort((a, b) => a.created_at.localeCompare(b.created_at))}
            client={client}
            run={run}
            onArchive={async (id) => {
              await run({ type: "workspace.archive", workspace_id: id });
            }}
          />
        ) : (
          <EmptyState
            hasRepos={repos.length > 0}
            client={client}
            onAdd={async (path) => {
              await run({ type: "repo.add", path });
            }}
          />
        )}
        </div>
      </main>
      {showAccounts && (
        <AccountsDialog
          agents={agents}
          terminals={state.terminals}
          preferences={state.preferences}
          client={client}
          run={run}
          onRefresh={refreshAgents}
          onClose={() => setShowAccounts(false)}
        />
      )}
      {showInbox && (
        <InboxDialog
          inbox={Object.values(state.inbox)}
          permissions={Object.values(state.permissions)}
          workspaces={state.workspaces}
          run={run}
          onSelectWorkspace={(id) => {
            setSelectedId(id);
            setShowInbox(false);
          }}
          onClose={() => setShowInbox(false)}
        />
      )}
      <StatusBar
        inboxCount={Object.keys(state.inbox).length + Object.keys(state.permissions).length}
        onOpenInbox={() => setShowInbox(true)}
        client={client}
        onError={setError}
        agents={agents}
        onOpenAccounts={() => setShowAccounts(true)}
        status={status}
        lastSeq={state.lastSeq}
        daemonVersion={state.daemonVersion}
        error={error}
        onDismissError={() => setError(null)}
        onDisconnect={() => onDisconnect()}
      />
    </div>
  );
}

function EmptyState({
  hasRepos,
  client,
  onAdd,
}: {
  hasRepos: boolean;
  client: DaemonClient;
  onAdd: (path: string) => Promise<void>;
}) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="w-full max-w-md text-center">
        <h2 className="text-base font-semibold">{hasRepos ? "Pick a workspace" : "Add your first repository"}</h2>
        <p className="mt-1.5 text-muted">
          {hasRepos
            ? "Each workspace is its own git worktree and branch, so agents can work in parallel without stepping on each other."
            : "Point Bantu at a git repository. New workspaces branch from its default branch."}
        </p>
        {!hasRepos && (
          <div className="mt-5 text-left">
            <p className="mb-1.5 px-2 text-[11.5px] uppercase tracking-wide text-faint">On this machine</p>
            <RepoSuggestions client={client} onAdd={onAdd} />
          </div>
        )}
      </div>
    </div>
  );
}
