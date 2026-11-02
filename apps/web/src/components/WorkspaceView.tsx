import { useEffect, useState, type ReactNode } from "react";
import type { Command, CommandResponse, DiffComment, Repo, Session, Terminal, Workspace } from "@bantu/protocol";
import type { DaemonClient } from "../lib/api.ts";
import type { ChatEntry, LogLine } from "../lib/store.ts";
import { useWorkspaceGit } from "../lib/useWorkspaceGit.ts";
import { ChangesPanel } from "./changes/ChangesPanel.tsx";
import { ChatPanel } from "./chat/ChatPanel.tsx";
import { GitStatus } from "./GitStatus.tsx";
import { TerminalPanel } from "./terminal/TerminalPanel.tsx";
import { CopyButton } from "./CopyButton.tsx";
import { LogView } from "./LogView.tsx";
import { StateDot } from "./StateDot.tsx";

type Tab = "chat" | "changes" | "terminal" | "log";

export function WorkspaceView({
  workspace,
  repo,
  logs,
  sessions,
  chat,
  comments,
  terminals,
  client,
  run,
  onArchive,
}: {
  workspace: Workspace;
  repo: Repo | undefined;
  logs: LogLine[];
  sessions: Session[];
  chat: Record<string, ChatEntry[]>;
  /** This workspace's diff comments. */
  comments: DiffComment[];
  /** This workspace's terminals, oldest first. */
  terminals: Terminal[];
  client: DaemonClient;
  run: (command: Command) => Promise<CommandResponse | null>;
  onArchive: (id: string) => Promise<void>;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [tab, setTab] = useState<Tab>("chat");
  const [activeId, setActiveId] = useState<string | null>(null);
  const [terminalId, setTerminalId] = useState<string | null>(null);
  useEffect(() => setConfirming(false), [workspace.id]);

  const active = sessions.find((s) => s.id === activeId) ?? sessions[sessions.length - 1];
  const git = useWorkspaceGit(client, workspace, sessions);
  const drafts = comments.filter((c) => !c.sent_at).length;
  const ready = workspace.state === "ready";

  const canArchive = workspace.state === "ready" || workspace.state === "failed";

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-start justify-between gap-4 px-5 pt-4 pb-2">
        <div className="min-w-0">
          <p className="text-xs text-muted">
            {repo?.name ?? "Unknown repository"} · <span className="font-mono">{workspace.branch}</span>
          </p>
          <h2 className="mt-0.5 flex items-center gap-2 text-lg font-semibold tracking-tight">
            {workspace.name}
            <span className="flex items-center gap-1.5 rounded-full border border-line px-2 py-0.5 text-xs font-normal text-muted">
              <StateDot state={workspace.state} decorative />
              {workspace.state}
            </span>
          </h2>
        </div>
        <div className="flex items-center gap-3">
        {ready && (
          <button
            type="button"
            onClick={async () => {
              const res = await run({ type: "workspace.run", workspace_id: workspace.id });
              const t = res?.result as Terminal | undefined;
              if (t?.id) {
                setTerminalId(t.id);
                setTab("terminal");
              }
            }}
            title="Run this repo's run script in a terminal"
            className="rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:bg-raised hover:text-fg"
          >
            ▶ Run
          </button>
        )}
        {ready && (
          <GitStatus
            diff={git.diff}
            pr={git.pr}
            canCreatePr={!!active && active.status === "idle"}
            onCreatePr={async () => {
              if (!active) return;
              if (await run({ type: "workspace.create_pr", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
            }}
          />
        )}
        {canArchive &&
          (confirming ? (
            <div className="flex items-center gap-1.5">
              <span className="text-muted">Remove the worktree? The branch is kept.</span>
              <button
                type="button"
                disabled={busy}
                onClick={async () => {
                  setBusy(true);
                  try {
                    await onArchive(workspace.id);
                  } finally {
                    setBusy(false);
                    setConfirming(false);
                  }
                }}
                className="rounded-md bg-bad px-2.5 py-1 font-medium text-bg disabled:opacity-40"
              >
                Archive
              </button>
              <button type="button" onClick={() => setConfirming(false)} className="rounded-md px-2.5 py-1 text-muted hover:bg-raised">
                Cancel
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setConfirming(true)}
              className="rounded-md border border-line px-2.5 py-1 text-muted hover:bg-raised hover:text-fg"
            >
              Archive…
            </button>
          ))}
        </div>
      </header>

      {workspace.error && (
        <div className="mx-5 mb-2 rounded-md border border-bad/40 bg-bad/10 px-3 py-2 text-bad">{workspace.error}</div>
      )}

      <div className="flex gap-1 border-b border-line px-5" role="tablist" aria-label="Workspace">
        <TabButton active={tab === "chat"} onClick={() => setTab("chat")}>
          Chat
        </TabButton>
        <TabButton active={tab === "changes"} onClick={() => setTab("changes")}>
          Changes
          {git.diff && git.diff.files.length > 0 && <span className="ml-1.5 text-faint tabular-nums">{git.diff.files.length}</span>}
          {drafts > 0 && <span className="ml-1.5 rounded bg-warn/15 px-1 text-[11px] text-warn">{drafts}</span>}
        </TabButton>
        <TabButton active={tab === "terminal"} onClick={() => setTab("terminal")}>
          Terminal
          {terminals.some((t) => !t.exited) && <span className="ml-1.5 text-faint tabular-nums">{terminals.filter((t) => !t.exited).length}</span>}
        </TabButton>
        <TabButton active={tab === "log"} onClick={() => setTab("log")}>
          Setup log
        </TabButton>
      </div>

      {tab === "chat" ? (
        <ChatPanel workspace={workspace} sessions={sessions} active={active} onSelect={setActiveId} chat={chat} run={run} />
      ) : tab === "terminal" ? (
        <TerminalPanel
          workspace={workspace}
          terminals={terminals}
          activeId={terminalId}
          onSelect={setTerminalId}
          client={client}
          run={run}
        />
      ) : tab === "changes" ? (
        <ChangesPanel
          diff={git.diff}
          diffError={git.diffError}
          comments={comments}
          canSend={!!active && ready}
          onRefresh={git.refreshDiff}
          onAddComment={async (path, line, body) =>
            (await run({ type: "comment.add", workspace_id: workspace.id, path, line, body })) !== null
          }
          onDeleteComment={async (id) => {
            await run({ type: "comment.delete", comment_id: id });
          }}
          onSendComments={async () => {
            if (!active) return;
            if (await run({ type: "comments.send", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
          }}
        />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-4 p-5">
          <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 rounded-md border border-line bg-panel p-3">
            <Field label="Branch">
              <span className="font-mono">{workspace.branch}</span>
              <CopyButton text={workspace.branch} />
            </Field>
            <Field label="Base">
              <span className="font-mono">{workspace.base_ref}</span>
            </Field>
            <Field label="Path">
              <span className="truncate font-mono" title={workspace.path}>
                {workspace.path}
              </span>
              <CopyButton text={workspace.path} />
            </Field>
            <Field label="Ports">
              <span className="font-mono tabular-nums">
                {workspace.port_base}–{workspace.port_base + 9}
              </span>
              <span className="text-faint">BANTU_PORT</span>
            </Field>
            {workspace.archive_commit && (
              <Field label="Saved changes">
                <span className="font-mono">{workspace.archive_commit.slice(0, 12)}</span>
                <CopyButton text={workspace.archive_commit} />
              </Field>
            )}
            <Field label="Created">
              <span className="tabular-nums">{new Date(workspace.created_at).toLocaleString()}</span>
            </Field>
          </dl>
          <section className="flex min-h-0 flex-1 flex-col gap-1.5">
            <h3 className="text-xs font-medium uppercase tracking-wide text-muted">Log</h3>
            <LogView lines={logs} />
          </section>
        </div>
      )}
    </div>
  );
}

function TabButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={`-mb-px border-b-2 px-2.5 py-1.5 ${active ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg"}`}
    >
      {children}
    </button>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-muted">{label}</dt>
      <dd className="flex min-w-0 items-center gap-2">{children}</dd>
    </>
  );
}
