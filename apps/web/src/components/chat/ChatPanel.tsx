import { useEffect, useState } from "react";
import type { AgentKind, AgentStatus, Command, CommandResponse, PermissionRequest, Session, Terminal, Workspace } from "@bantu/protocol";
import type { ChatEntry, DaemonState } from "../../lib/store.ts";
import { agentLabel, AGENT_KINDS } from "../../lib/useAgents.ts";
import { Composer } from "./Composer.tsx";
import { PermissionCard } from "./PermissionCard.tsx";
import { Transcript } from "./Transcript.tsx";

type Run = (command: Command) => Promise<CommandResponse | null>;

export function ChatPanel({
  workspace,
  sessions,
  active,
  onSelect: setActiveId,
  chat,
  streaming,
  permissions,
  agents,
  onOpenAccounts,
  terminals,
  onShowTerminal,
  run,
}: {
  workspace: Workspace;
  /** This workspace's sessions, oldest first. */
  sessions: Session[];
  /** The chat being shown; the workspace view owns this so PR and review actions target it. */
  active: Session | undefined;
  onSelect: (sessionId: string) => void;
  chat: Record<string, ChatEntry[]>;
  streaming: DaemonState["streaming"];
  agents: AgentStatus[] | null;
  onOpenAccounts: () => void;
  /** Pending permission requests for this workspace. */
  permissions: PermissionRequest[];
  /** This workspace's terminals, to find one continuing the active chat. */
  terminals: Terminal[];
  onShowTerminal: (terminalId: string) => void;
  run: Run;
}) {
  // Jump to a chat the user just created once the daemon reports it.
  const [pendingNew, setPendingNew] = useState<string | null>(null);
  useEffect(() => {
    if (pendingNew && sessions.some((s) => s.id === pendingNew)) {
      setActiveId(pendingNew);
      setPendingNew(null);
    }
  }, [pendingNew, sessions]);

  const ready = workspace.state === "ready";
  const newChat = async (agent: AgentKind) => {
    const res = await run({ type: "session.create", workspace_id: workspace.id, agent, model: null, permission_mode: null });
    const created = res?.result as Session | undefined;
    if (created?.id) setPendingNew(created.id);
  };

  const liveTerminal = active && terminals.find((t) => t.session_id === active.id && !t.exited);
  const canUseTerminal = active && (active.agent === "claude" || active.agent === "codex");
  const openTerminal = async () => {
    if (!active) return;
    const res = await run({ type: "session.open_terminal", session_id: active.id, cols: null, rows: null });
    const t = res?.result as Terminal | undefined;
    if (t?.id) onShowTerminal(t.id);
  };

  if (!active) {
    return (
      <div className="flex flex-1 items-center justify-center p-8 text-center text-muted">
        {ready ? (
          <div>
            <p>No chat in this workspace yet.</p>
            <div className="mt-3 flex justify-center gap-2">
              <button type="button" onClick={() => void newChat("claude")} className="rounded-md bg-accent px-3 py-1.5 font-medium text-bg">
                Start a Claude chat
              </button>
              {AGENT_KINDS.filter((a) => a !== "claude" && isInstalled(agents, a)).map((a) => (
                <button key={a} type="button" onClick={() => void newChat(a)} className="rounded-md border border-line px-3 py-1.5 text-muted hover:text-fg">
                  {agentLabel(a)}
                </button>
              ))}
            </div>
          </div>
        ) : (
          <p>The chat opens once the workspace is ready.</p>
        )}
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {(sessions.length > 1 || ready) && (
        <div className="flex items-center gap-0.5 overflow-x-auto border-b border-line px-3" role="tablist" aria-label="Chats">
          {sessions.length > 1 &&
            sessions.map((s) => (
              <button
                key={s.id}
                type="button"
                role="tab"
                aria-selected={s.id === active.id}
                onClick={() => setActiveId(s.id)}
                className={`flex max-w-48 shrink-0 items-center gap-1.5 border-b-2 px-2.5 py-1.5 ${
                  s.id === active.id ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg"
                }`}
                title={s.title}
              >
                {permissions.some((p) => p.session_id === s.id) ? (
                  <span aria-label="needs approval" className="size-1.5 shrink-0 rounded-full bg-warn" />
                ) : (
                  s.status === "running" && <span aria-label="running" className="size-1.5 shrink-0 animate-pulse rounded-full bg-accent" />
                )}
                {s.agent !== "claude" && <span className="shrink-0 text-[10.5px] uppercase tracking-wide text-faint">{agentLabel(s.agent)}</span>}
                <span className="truncate">{s.title}</span>
              </button>
            ))}
          {ready && (
            <span className="ml-1 flex shrink-0 items-center gap-0.5 text-muted">
              <span className="px-1">+ New chat:</span>
              {AGENT_KINDS.filter((a) => a === "claude" || isInstalled(agents, a)).map((agent) => (
                <button
                  key={agent}
                  type="button"
                  onClick={() => void newChat(agent)}
                  className="rounded px-2 py-1 hover:bg-raised hover:text-fg"
                  title={`New ${agentLabel(agent)} chat in this workspace (uses your own login)`}
                >
                  {agentLabel(agent)}
                </button>
              ))}
            </span>
          )}
          {ready && canUseTerminal && (
            <button
              type="button"
              disabled={active.status === "running"}
              onClick={() => void openTerminal()}
              className="ml-auto shrink-0 rounded px-2 py-1 text-muted hover:bg-raised hover:text-fg disabled:opacity-40"
              title="Continue this conversation in the agent's own terminal UI"
            >
              Open in terminal
            </button>
          )}
        </div>
      )}
      <Transcript
        entries={chat[active.id] ?? []}
        session={active}
        streaming={streaming[active.id]}
        workspacePath={workspace.path}
        onRewind={async (turn) => {
          await run({ type: "session.rewind", session_id: active.id, turn });
        }}
      />
      {liveTerminal && (
        <div className="mx-auto w-full max-w-3xl px-5 pb-2">
          <button
            type="button"
            onClick={() => onShowTerminal(liveTerminal.id)}
            className="w-full rounded-md border border-line bg-panel px-3 py-2 text-left text-[12.5px] text-muted hover:text-fg"
          >
            This conversation is open in a terminal. Go to the terminal →
          </button>
        </div>
      )}
      <AgentSetupNotice status={agents?.find((a) => a.agent === active.agent)} onOpenAccounts={onOpenAccounts} />
      {permissions.some((p) => p.session_id === active.id) && (
        <div className="mx-auto flex w-full max-w-3xl flex-col gap-2 px-5 pb-2">
          {permissions
            .filter((p) => p.session_id === active.id)
            .map((p) => (
              <PermissionCard
                key={p.id}
                request={p}
                workspacePath={workspace.path}
                onRespond={async (allow, message) => {
                  await run({ type: "permission.respond", request_id: p.id, allow, message });
                }}
              />
            ))}
        </div>
      )}
      <Composer
        session={active}
        disabledReason={
          !ready ? `Workspace is ${workspace.state}` : liveTerminal ? "This chat is open in a terminal. Close it to continue here." : null
        }
        onSend={async (text) => (await run({ type: "session.send", session_id: active.id, text })) !== null}
        onStop={async () => {
          await run({ type: "session.cancel", session_id: active.id });
        }}
        onUpdate={async (change) => {
          await run({
            type: "session.update",
            session_id: active.id,
            title: null,
            model: change.model ?? null,
            permission_mode: change.permission_mode ?? null,
          });
        }}
      />
    </div>
  );
}

/** Codex stays visible while status is loading, as it was before more agents existed. */
function isInstalled(agents: AgentStatus[] | null, agent: AgentKind) {
  const status = agents?.find((a) => a.agent === agent);
  return status ? status.installed : agent === "codex";
}

/** Shown when the chat's agent can't run: not installed or not signed in. */
function AgentSetupNotice({ status, onOpenAccounts }: { status: AgentStatus | undefined; onOpenAccounts: () => void }) {
  if (!status || (status.installed && status.logged_in)) return null;
  const name = status.agent === "claude" ? "Claude Code" : agentLabel(status.agent);
  return (
    <div className="mx-auto flex w-full max-w-3xl items-center gap-3 px-5 pb-2">
      <p className="flex-1 rounded-md border border-warn/40 bg-warn/5 px-3 py-2 text-[12.5px] text-warn">
        {status.installed ? `${name} isn't signed in on this machine.` : `${name} isn't installed on this machine.`}{" "}
        <button type="button" onClick={onOpenAccounts} className="font-medium underline underline-offset-2">
          {status.installed ? "Sign in" : "How to install"}
        </button>
      </p>
    </div>
  );
}
