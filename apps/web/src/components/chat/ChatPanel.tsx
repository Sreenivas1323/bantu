import { useEffect, useState } from "react";
import type { Command, CommandResponse, Session, Workspace } from "@bantu/protocol";
import type { ChatEntry } from "../../lib/store.ts";
import { Composer } from "./Composer.tsx";
import { Transcript } from "./Transcript.tsx";

type Run = (command: Command) => Promise<CommandResponse | null>;

export function ChatPanel({
  workspace,
  sessions,
  active,
  onSelect: setActiveId,
  chat,
  run,
}: {
  workspace: Workspace;
  /** This workspace's sessions, oldest first. */
  sessions: Session[];
  /** The chat being shown; the workspace view owns this so PR and review actions target it. */
  active: Session | undefined;
  onSelect: (sessionId: string) => void;
  chat: Record<string, ChatEntry[]>;
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
  const newChat = async () => {
    const res = await run({ type: "session.create", workspace_id: workspace.id, model: null, permission_mode: null });
    const created = res?.result as Session | undefined;
    if (created?.id) setPendingNew(created.id);
  };

  if (!active) {
    return (
      <div className="flex flex-1 items-center justify-center p-8 text-center text-muted">
        {ready ? (
          <div>
            <p>No chat in this workspace yet.</p>
            <button type="button" onClick={() => void newChat()} className="mt-3 rounded-md bg-accent px-3 py-1.5 font-medium text-bg">
              Start a chat
            </button>
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
                {s.status === "running" && <span aria-label="running" className="size-1.5 shrink-0 animate-pulse rounded-full bg-accent" />}
                <span className="truncate">{s.title}</span>
              </button>
            ))}
          {ready && (
            <button
              type="button"
              onClick={() => void newChat()}
              className="ml-1 shrink-0 rounded px-2 py-1 text-muted hover:bg-raised hover:text-fg"
              title="New chat in this workspace"
            >
              + New chat
            </button>
          )}
        </div>
      )}
      <Transcript
        entries={chat[active.id] ?? []}
        session={active}
        workspacePath={workspace.path}
        onRewind={async (turn) => {
          await run({ type: "session.rewind", session_id: active.id, turn });
        }}
      />
      <Composer
        session={active}
        disabledReason={ready ? null : `Workspace is ${workspace.state}`}
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
