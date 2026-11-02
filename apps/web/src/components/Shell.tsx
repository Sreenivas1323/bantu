import { useEffect, useMemo, useRef, useState } from "react";
import type { Command, Session, Workspace } from "@bantu/protocol";
import type { Connection } from "../lib/connection.ts";
import { useDaemon } from "../lib/useDaemon.ts";
import { useNotifications } from "../lib/useNotifications.ts";
import { Sidebar } from "./Sidebar.tsx";
import { StatusBar } from "./StatusBar.tsx";
import { WorkspaceView } from "./WorkspaceView.tsx";

export function Shell({ conn, onDisconnect }: { conn: Connection; onDisconnect: (reason?: string) => void }) {
  const { state, status, send, loadHistory, client } = useDaemon(conn);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showArchived, setShowArchived] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (status === "unauthorized") onDisconnect("The daemon rejected this token. Paste a fresh one.");
  }, [status, onDisconnect]);

  const repos = useMemo(() => Object.values(state.repos).sort((a, b) => a.name.localeCompare(b.name)), [state.repos]);
  const workspacesByRepo = useMemo(() => {
    const groups: Record<string, Workspace[]> = {};
    for (const w of Object.values(state.workspaces)) (groups[w.repo_id] ??= []).push(w);
    for (const list of Object.values(groups)) list.sort((a, b) => b.created_at.localeCompare(a.created_at));
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
    () => new Set(Object.values(state.permissions).map((p) => p.workspace_id)),
    [state.permissions],
  );
  useNotifications(state);

  const selected = selectedId ? state.workspaces[selectedId] : undefined;

  return (
    <div className="grid h-full grid-cols-[264px_1fr] grid-rows-[1fr_auto]">
      <Sidebar
        repos={repos}
        workspacesByRepo={workspacesByRepo}
        sessionsByWorkspace={sessionsByWorkspace}
        waitingWorkspaces={waitingWorkspaces}
        selectedId={selectedId}
        showArchived={showArchived}
        onToggleArchived={() => setShowArchived((v) => !v)}
        onSelect={setSelectedId}
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
      <main className="min-h-0 min-w-0 overflow-hidden">
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
          <EmptyState hasRepos={repos.length > 0} />
        )}
      </main>
      <StatusBar
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

function EmptyState({ hasRepos }: { hasRepos: boolean }) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="max-w-sm text-center">
        <h2 className="text-base font-semibold">{hasRepos ? "Pick a workspace" : "Add your first repository"}</h2>
        <p className="mt-1.5 text-muted">
          {hasRepos
            ? "Each workspace is its own git worktree and branch, so agents can work in parallel without stepping on each other."
            : "Point Bantu at a local git repository. New workspaces branch from its default branch."}
        </p>
      </div>
    </div>
  );
}
