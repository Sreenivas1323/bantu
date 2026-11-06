import { useEffect, useState, type ReactNode } from "react";
import type {
  AgentStatus,
  Command,
  CommandResponse,
  DiffComment,
  InboxItem,
  Loadout,
  PermissionRequest,
  Repo,
  ReviewState,
  Session,
  Spotlight,
  Terminal,
  Workspace,
} from "@bantu/protocol";
import type { DaemonClient } from "../lib/api.ts";
import type { ChatEntry, DaemonState, LogLine } from "../lib/store.ts";
import type { conflictsFor } from "../lib/store.ts";
import { ConflictBanner } from "./ConflictBanner.tsx";
import { useWorkspaceGit } from "../lib/useWorkspaceGit.ts";
import { ChangesPanel } from "./changes/ChangesPanel.tsx";
import { ChatPanel } from "./chat/ChatPanel.tsx";
import { BoardPanel } from "./BoardPanel.tsx";
import { ChecksPanel } from "./checks/ChecksPanel.tsx";
import { GitStatus } from "./GitStatus.tsx";
import { TerminalPanel } from "./terminal/TerminalPanel.tsx";
import { CopyButton } from "./CopyButton.tsx";
import { LogView } from "./LogView.tsx";
import { PortsPanel } from "./PortsPanel.tsx";
import { StateDot } from "./StateDot.tsx";

type Tab = "chat" | "board" | "changes" | "checks" | "terminal" | "ports" | "log";

export function WorkspaceView({
  workspace,
  repo,
  logs,
  sessions,
  chat,
  streaming,
  comments,
  terminals,
  permissions,
  questions,
  conflicts,
  agents,
  onOpenAccounts,
  allWorkspaces,
  onSelectWorkspace,
  loadouts,
  spotlight,
  client,
  run,
  onArchive,
}: {
  workspace: Workspace;
  repo: Repo | undefined;
  logs: LogLine[];
  sessions: Session[];
  chat: Record<string, ChatEntry[]>;
  streaming: DaemonState["streaming"];
  /** This workspace's diff comments. */
  comments: DiffComment[];
  /** This workspace's terminals, oldest first. */
  terminals: Terminal[];
  permissions: PermissionRequest[];
  /** Questions from this workspace's agents. */
  questions: InboxItem[];
  conflicts: ReturnType<typeof conflictsFor>;
  agents: AgentStatus[] | null;
  onOpenAccounts: () => void;
  allWorkspaces: Record<string, Workspace>;
  onSelectWorkspace: (id: string) => void;
  loadouts: Loadout[];
  /** What's in this repo's main checkout, if anything. */
  spotlight: Spotlight | undefined;
  client: DaemonClient;
  run: (command: Command) => Promise<CommandResponse | null>;
  onArchive: (id: string) => Promise<void>;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [selectedTab, setTab] = useState<Tab>("chat");
  const [activeId, setActiveId] = useState<string | null>(null);
  const [terminalId, setTerminalId] = useState<string | null>(null);
  useEffect(() => setConfirming(false), [workspace.id]);

  const active = sessions.find((s) => s.id === activeId) ?? sessions[sessions.length - 1];
  const git = useWorkspaceGit(client, workspace, sessions);
  const drafts = comments.filter((c) => !c.sent_at).length;
  const ready = workspace.state === "ready";

  const canArchive = workspace.state === "ready" || workspace.state === "failed";
  const childCount = Object.values(allWorkspaces).filter((w) => w.parent_id === workspace.id && w.state !== "archived").length;
  // The Board tab only exists while there are children to show.
  const tab = selectedTab === "board" && childCount === 0 ? "chat" : selectedTab;

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <header className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 px-5 pt-4 pb-2 max-md:px-3 max-md:pt-2">
        <div className="min-w-0 max-md:hidden">
          <p className="truncate text-xs text-muted">
            {repo?.name ?? "Unknown repository"} · <span className="font-mono">{workspace.branch}</span>
            {workspace.parent_id && allWorkspaces[workspace.parent_id] && (
              <>
                {" · started by "}
                <button type="button" onClick={() => onSelectWorkspace(workspace.parent_id!)} className="underline-offset-2 hover:text-fg hover:underline">
                  {allWorkspaces[workspace.parent_id].name}
                </button>
              </>
            )}
          </p>
          <h2 className="mt-0.5 flex items-center gap-2 text-lg font-semibold tracking-tight">
            {workspace.name}
            <span className="flex items-center gap-1.5 rounded-full border border-line px-2 py-0.5 text-xs font-normal text-muted">
              <StateDot state={workspace.state} decorative />
              {workspace.state}
            </span>
            {workspace.review_state && (
              <span className={`rounded-full border px-2 py-0.5 text-xs font-normal ${REVIEW[workspace.review_state].tone}`}>{REVIEW[workspace.review_state].label}</span>
            )}
          </h2>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2 max-md:justify-start">
        {ready && (
          <button
            type="button"
            aria-pressed={spotlight?.workspace_id === workspace.id}
            onClick={() =>
              void run(
                spotlight?.workspace_id === workspace.id
                  ? { type: "repo.spotlight_stop", repo_id: workspace.repo_id }
                  : { type: "workspace.spotlight", workspace_id: workspace.id },
              )
            }
            title={
              spotlight?.workspace_id === workspace.id
                ? `Return the main checkout to ${spotlight.restore_to}`
                : "Try this work in the repo's main checkout, where your own servers and .env files live"
            }
            className={`rounded-md border px-2.5 py-1 text-[12px] hover:bg-raised ${
              spotlight?.workspace_id === workspace.id ? "border-accent/60 text-accent" : "border-line text-muted hover:text-fg"
            }`}
          >
            {spotlight?.workspace_id === workspace.id ? "Stop spotlight" : "Spotlight"}
          </button>
        )}
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
        {ready && !!active && (git.diff?.files.length ?? 0) > 0 && (
          <button
            type="button"
            onClick={async () => {
              if (await run({ type: "workspace.rename_branch", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
            }}
            title="Ask the agent to rename the branch to describe the work"
            className="rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:bg-raised hover:text-fg"
          >
            Rename branch
          </button>
        )}
        {ready && (git.diff?.files.length ?? 0) > 0 && (
          <button
            type="button"
            onClick={async (e) => {
              // Shift-click asks Codex for a second opinion.
              const agent = e.shiftKey ? "codex" : "claude";
              const res = await run({ type: "workspace.review", workspace_id: workspace.id, agent });
              const s = res?.result as Session | undefined;
              if (s?.id) {
                setActiveId(s.id);
                setTab("chat");
              }
            }}
            title="Start a read-only chat that reviews these changes (Shift-click: review with Codex)"
            className="rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:bg-raised hover:text-fg"
          >
            Review
          </button>
        )}
        {ready && (
          <GitStatus
            diff={git.diff}
            pr={git.pr}
            canCreatePr={!!repo?.remote_url && !!active && active.status === "idle"}
            createPrBlocked={repo && !repo.remote_url ? "This repository has no origin remote" : undefined}
            onCreatePr={async () => {
              if (!active) return;
              if (await run({ type: "workspace.create_pr", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
            }}
            onFixChecks={async () => {
              if (!active) return;
              if (await run({ type: "workspace.fix_checks", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
            }}
          />
        )}
        {canArchive &&
          (confirming ? (
            <div className="flex items-center gap-1.5">
              <span className="text-muted max-md:hidden">Remove the worktree? The branch is kept.</span>
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

      {spotlight && (
        <p role="status" className="border-b border-accent/30 bg-accent/5 px-5 py-1.5 text-[12.5px] text-accent">
          {spotlight.workspace_id === workspace.id ? (
            <>
              The main checkout is showing this workspace at <span className="font-mono">{spotlight.commit.slice(0, 8)}</span>, and updates after each agent turn.
            </>
          ) : (
            <>
              The main checkout is showing{" "}
              <button type="button" onClick={() => onSelectWorkspace(spotlight.workspace_id)} className="font-medium underline-offset-2 hover:underline">
                {allWorkspaces[spotlight.workspace_id]?.name ?? "another workspace"}
              </button>
              . Spotlighting this one switches it here.
            </>
          )}
        </p>
      )}
      <ConflictBanner
        conflicts={conflicts}
        workspaces={allWorkspaces}
        onSelect={onSelectWorkspace}
        onResolve={
          ready && active
            ? async () => {
                if (await run({ type: "workspace.resolve_conflicts", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
              }
            : undefined
        }
      />
      <div className="scroll-quiet flex shrink-0 gap-1 overflow-x-auto border-b border-line px-5 max-md:px-2" role="tablist" aria-label="Workspace">
        <TabButton active={tab === "chat"} onClick={() => setTab("chat")}>
          Chat
        </TabButton>
        {childCount > 0 && (
          <TabButton active={tab === "board"} onClick={() => setTab("board")}>
            Board
            <span className="ml-1.5 text-faint tabular-nums">{childCount}</span>
          </TabButton>
        )}
        <TabButton active={tab === "changes"} onClick={() => setTab("changes")}>
          Changes
          {git.diff && git.diff.files.length > 0 && <span className="ml-1.5 text-faint tabular-nums">{git.diff.files.length}</span>}
          {drafts > 0 && <span className="ml-1.5 rounded bg-warn/15 px-1 text-[11px] text-warn">{drafts}</span>}
        </TabButton>
        <TabButton active={tab === "checks"} onClick={() => setTab("checks")}>
          Checks
          {(git.pr?.checks_failing ?? 0) > 0 && <span className="ml-1.5 rounded bg-bad/15 px-1 text-[11px] text-bad">{git.pr?.checks_failing}</span>}
        </TabButton>
        <TabButton active={tab === "terminal"} onClick={() => setTab("terminal")}>
          Terminal
          {terminals.some((t) => !t.exited) && <span className="ml-1.5 text-faint tabular-nums">{terminals.filter((t) => !t.exited).length}</span>}
        </TabButton>
        <TabButton active={tab === "ports"} onClick={() => setTab("ports")}>
          Preview
        </TabButton>
        <TabButton active={tab === "log"} onClick={() => setTab("log")}>
          Setup log
        </TabButton>
      </div>

      {tab === "chat" ? (
        <ChatPanel
          workspace={workspace}
          sessions={sessions}
          active={active}
          onSelect={setActiveId}
          chat={chat}
          streaming={streaming}
          permissions={permissions}
          questions={questions}
          agents={agents}
          onOpenAccounts={onOpenAccounts}
          terminals={terminals}
          loadouts={loadouts}
          onShowTerminal={(id) => {
            setTerminalId(id);
            setTab("terminal");
          }}
          run={run}
        />
      ) : tab === "terminal" ? (
        <TerminalPanel
          workspace={workspace}
          terminals={terminals}
          activeId={terminalId}
          onSelect={setTerminalId}
          client={client}
          run={run}
        />
      ) : tab === "board" ? (
        <BoardPanel
          workspace={workspace}
          client={client}
          run={run}
          activeSessionId={active?.id}
          onOpen={onSelectWorkspace}
          onAgentTookOver={() => setTab("chat")}
        />
      ) : tab === "ports" ? (
        <PortsPanel workspace={workspace} client={client} />
      ) : tab === "checks" ? (
        <ChecksPanel
          workspaceId={workspace.id}
          client={client}
          canSend={!!active && ready}
          onFixChecks={async () => {
            if (!active) return;
            if (await run({ type: "workspace.fix_checks", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
          }}
          onAddressComments={async () => {
            if (!active) return;
            if (await run({ type: "workspace.address_comments", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
          }}
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
          <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-2 rounded-md border border-line bg-panel p-3">
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

const REVIEW: Record<ReviewState, { label: string; tone: string }> = {
  reviewing: { label: "In review", tone: "border-line text-muted" },
  changes_requested: { label: "Changes requested", tone: "border-warn/50 text-warn" },
  approved: { label: "Ready", tone: "border-ok/50 text-ok" },
};

function TabButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={`-mb-px shrink-0 border-b-2 px-2.5 py-1.5 max-md:py-2.5 ${active ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg"}`}
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
