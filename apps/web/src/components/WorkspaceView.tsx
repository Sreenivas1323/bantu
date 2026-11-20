import { useEffect, useState, type MouseEvent, type ReactNode } from "react";
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
import { Icon, IconButton, Menu, MenuItem, MenuSeparator } from "./Menu.tsx";
import { useMediaQuery, useStoredFlag } from "../lib/useMediaQuery.ts";

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

  // A ready workspace says what's happening in it, in the board's words and colours.
  const activity = !ready
    ? null
    : permissions.length > 0 || questions.length > 0
      ? { label: "needs you", dot: "bg-warn", tone: "border-warn/40 text-warn" }
      : sessions.some((s) => s.status === "running")
        ? { label: "working", dot: "bg-accent animate-pulse", tone: "border-accent/40 text-accent" }
        : null;

  const canArchive = workspace.state === "ready" || workspace.state === "failed";
  const childCount = Object.values(allWorkspaces).filter((w) => w.parent_id === workspace.id && w.state !== "archived").length;
  const hasChanges = (git.diff?.files.length ?? 0) > 0;
  const spotlit = spotlight?.workspace_id === workspace.id;

  // Wide windows put changes, checks and the terminal in a panel beside the
  // chat; narrower ones show everything as tabs.
  const wide = useMediaQuery("(min-width: 1280px)");
  const [inspectorOpen, setInspectorOpen] = useStoredFlag("bantu.inspector", true);
  const [terminalOpen, setTerminalOpen] = useStoredFlag("bantu.inspector.terminal", true);
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("changes");
  const split = wide && inspectorOpen;

  // The Board tab only exists while there are children to show.
  const tab = selectedTab === "board" && childCount === 0 ? "chat" : split && INSPECTOR.has(selectedTab as InspectorTab) ? "chat" : selectedTab;
  /** Brings a view forward, wherever it lives in the current layout. */
  const show = (t: Tab) => {
    if (split && t === "terminal") return setTerminalOpen(true);
    if (split && INSPECTOR.has(t as InspectorTab)) return setInspectorTab(t as InspectorTab);
    setTab(t);
  };

  const panel = (t: Tab): ReactNode => {
    switch (t) {
      case "chat":
        return (
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
              show("terminal");
            }}
            run={run}
          />
        );
      case "terminal":
        return <TerminalPanel workspace={workspace} terminals={terminals} activeId={terminalId} onSelect={setTerminalId} client={client} run={run} />;
      case "board":
        return (
          <BoardPanel
            workspace={workspace}
            client={client}
            run={run}
            activeSessionId={active?.id}
            onOpen={onSelectWorkspace}
            onAgentTookOver={() => setTab("chat")}
          />
        );
      case "ports":
        return <PortsPanel workspace={workspace} client={client} />;
      case "checks":
        return (
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
        );
      case "changes":
        return (
          <ChangesPanel
            diff={git.diff}
            diffError={git.diffError}
            comments={comments}
            canSend={!!active && ready}
            onRefresh={git.refreshDiff}
            onAddComment={async (path, line, body) => (await run({ type: "comment.add", workspace_id: workspace.id, path, line, body })) !== null}
            onDeleteComment={async (id) => {
              await run({ type: "comment.delete", comment_id: id });
            }}
            onSendComments={async () => {
              if (!active) return;
              if (await run({ type: "comments.send", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
            }}
          />
        );
      case "log":
        return (
          <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
            <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-2 rounded-lg border border-line p-3">
              <Field label="Branch">
                <span className="truncate font-mono">{workspace.branch}</span>
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
            <section className="flex min-h-48 flex-1 flex-col gap-1.5">
              <h3 className="text-[12px] font-medium text-faint">Setup log</h3>
              <LogView lines={logs} />
            </section>
          </div>
        );
    }
  };

  const changesCount = git.diff && git.diff.files.length > 0 ? git.diff.files.length : null;
  const tabLabel = (t: Tab): ReactNode => {
    switch (t) {
      case "chat":
        return "Chat";
      case "board":
        return (
          <>
            Board <Count>{childCount}</Count>
          </>
        );
      case "changes":
        return (
          <>
            Changes {changesCount && <Count>{changesCount}</Count>}
            {drafts > 0 && <span className="ml-1.5 rounded bg-warn/15 px-1 text-[11px] text-warn">{drafts}</span>}
          </>
        );
      case "checks":
        return (
          <>
            Checks
            {(git.pr?.checks_failing ?? 0) > 0 && <span className="ml-1.5 rounded bg-bad/15 px-1 text-[11px] text-bad">{git.pr?.checks_failing}</span>}
          </>
        );
      case "terminal":
        return (
          <>
            Terminal {terminals.some((t) => !t.exited) && <Count>{terminals.filter((t) => !t.exited).length}</Count>}
          </>
        );
      case "ports":
        return "Preview";
      case "log":
        return "Setup";
    }
  };

  const centerTabs: Tab[] = split ? (childCount > 0 ? ["chat", "board"] : []) : ["chat", ...(childCount > 0 ? (["board"] as Tab[]) : []), "changes", "checks", "terminal", "ports", "log"];

  return (
    <div className="flex h-full min-h-0 min-w-0">
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <header className="flex min-h-12 flex-wrap items-center gap-x-3 gap-y-1.5 border-b border-line px-4 py-2 max-md:px-3">
          <div className="flex min-w-0 flex-1 items-center gap-2 max-md:hidden">
            <span className="shrink-0 text-muted">{repo?.name ?? "Unknown repository"}</span>
            <span aria-hidden className="text-faint">/</span>
            <h2 className="shrink-0 font-semibold">{workspace.name}</h2>
            <span className="flex min-w-0 max-w-56 shrink items-center gap-1 rounded-md bg-raised px-1.5 py-0.5 font-mono text-[11.5px] text-muted" title={`Branch ${workspace.branch}`}>
              <Icon className="size-3">
                <circle cx="5" cy="4" r="1.5" />
                <circle cx="5" cy="12" r="1.5" />
                <circle cx="11" cy="6" r="1.5" />
                <path d="M5 5.5v5M11 7.5c0 2-2 2.5-6 3" />
              </Icon>
              <span className="truncate">{workspace.branch}</span>
            </span>
            {workspace.task && (
              <a href={workspace.task.url} target="_blank" rel="noreferrer" title={workspace.task.title} className="shrink-0 text-muted underline-offset-2 hover:text-fg hover:underline">
                {workspace.task.key}
              </a>
            )}
            {workspace.parent_id && allWorkspaces[workspace.parent_id] && (
              <button type="button" onClick={() => onSelectWorkspace(workspace.parent_id!)} className="shrink-0 text-[12.5px] text-muted underline-offset-2 hover:text-fg hover:underline">
                from {allWorkspaces[workspace.parent_id].name}
              </button>
            )}
            {activity ? (
              <span className={`flex shrink-0 items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs ${activity.tone}`}>
                <span aria-hidden className={`size-1.5 rounded-full ${activity.dot}`} />
                {activity.label}
              </span>
            ) : (
              !ready && (
                <span className="flex shrink-0 items-center gap-1.5 rounded-full border border-line px-2 py-0.5 text-xs text-muted">
                  <StateDot state={workspace.state} decorative />
                  {workspace.state}
                </span>
              )
            )}
            {workspace.review_state && (
              <span className={`shrink-0 rounded-full border px-2 py-0.5 text-xs ${REVIEW[workspace.review_state].tone}`}>{REVIEW[workspace.review_state].label}</span>
            )}
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-1 max-md:w-full">
            {confirming ? (
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
              <>
                {ready && (
                  <HeaderButton
                    title="Run this repo's run script in a terminal"
                    onClick={async () => {
                      const res = await run({ type: "workspace.run", workspace_id: workspace.id });
                      const t = res?.result as Terminal | undefined;
                      if (t?.id) {
                        setTerminalId(t.id);
                        show("terminal");
                      }
                    }}
                  >
                    <Icon className="size-3.5">
                      <path d="M5 3.5v9l7-4.5z" />
                    </Icon>
                    Run
                  </HeaderButton>
                )}
                {ready && hasChanges && (
                  <HeaderButton
                    title="Start a read-only chat that reviews these changes with the repo's reviewer agent (Shift-click: review with Codex)"
                    onClick={async (e) => {
                      // The repo's reviewer ([agents] reviewer, else Claude); Shift-click asks Codex for a second opinion.
                      const agent = e.shiftKey ? "codex" : null;
                      const res = await run({ type: "workspace.review", workspace_id: workspace.id, agent });
                      const s = res?.result as Session | undefined;
                      if (s?.id) {
                        setActiveId(s.id);
                        setTab("chat");
                      }
                    }}
                  >
                    <Icon className="size-3.5">
                      <path d="M1.8 8s2.3-4.5 6.2-4.5S14.2 8 14.2 8s-2.3 4.5-6.2 4.5S1.8 8 1.8 8z" />
                      <circle cx="8" cy="8" r="1.8" />
                    </Icon>
                    Review
                  </HeaderButton>
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
                <Menu label="More for this workspace">
                  {(close) => (
                    <>
                      {ready && (
                        <MenuItem
                          onSelect={() => {
                            close();
                            void run(spotlit ? { type: "repo.spotlight_stop", repo_id: workspace.repo_id } : { type: "workspace.spotlight", workspace_id: workspace.id });
                          }}
                        >
                          <span className="flex-1">{spotlit ? "Stop spotlight" : "Spotlight in the main checkout"}</span>
                          {spotlit && <span className="size-1.5 rounded-full bg-accent" />}
                        </MenuItem>
                      )}
                      {ready && !!active && hasChanges && (
                        <MenuItem
                          onSelect={async () => {
                            close();
                            if (await run({ type: "workspace.rename_branch", workspace_id: workspace.id, session_id: active.id })) setTab("chat");
                          }}
                        >
                          Rename the branch to fit the work
                        </MenuItem>
                      )}
                      <MenuItem
                        onSelect={() => {
                          close();
                          show("log");
                        }}
                      >
                        Workspace details and setup log
                      </MenuItem>
                      {canArchive && (
                        <>
                          <MenuSeparator />
                          <MenuItem
                            danger
                            onSelect={() => {
                              close();
                              setConfirming(true);
                            }}
                          >
                            Archive workspace…
                          </MenuItem>
                        </>
                      )}
                    </>
                  )}
                </Menu>
                {wide && (
                  <IconButton label={inspectorOpen ? "Hide the side panel" : "Show changes, checks and terminal"} pressed={inspectorOpen} onClick={() => setInspectorOpen(!inspectorOpen)}>
                    <rect x="2" y="3" width="12" height="10" rx="1.5" />
                    <path d="M10 3v10" />
                  </IconButton>
                )}
              </>
            )}
          </div>
        </header>

        {workspace.error && <div className="mx-4 mt-2 rounded-md border border-bad/40 bg-bad/10 px-3 py-2 text-bad">{workspace.error}</div>}

        {spotlight && (
          <p role="status" className="border-b border-accent/30 bg-accent/5 px-4 py-1.5 text-[12.5px] text-accent">
            {spotlit ? (
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
        {centerTabs.length > 0 && (
          <div className="scroll-quiet flex shrink-0 gap-1 overflow-x-auto border-b border-line px-4 max-md:px-2" role="tablist" aria-label="Workspace">
            {centerTabs.map((t) => (
              <TabButton key={t} active={tab === t} onClick={() => setTab(t)}>
                {tabLabel(t)}
              </TabButton>
            ))}
          </div>
        )}
        {panel(tab)}
      </div>

      {split && (
        <aside aria-label="Changes, checks and terminal" className="flex w-[clamp(380px,33vw,640px)] min-h-0 shrink-0 flex-col border-l border-line bg-bg">
          <div className="scroll-quiet flex min-h-12 shrink-0 items-end gap-1 overflow-x-auto border-b border-line px-3" role="tablist" aria-label="Side panel">
            {(["changes", "checks", "ports", "log"] as InspectorTab[]).map((t) => (
              <TabButton key={t} active={inspectorTab === t} onClick={() => setInspectorTab(t)}>
                {tabLabel(t)}
              </TabButton>
            ))}
          </div>
          <div className="flex min-h-0 flex-1 flex-col">{panel(inspectorTab)}</div>
          <section aria-label="Terminal" className={`flex shrink-0 flex-col border-t border-line ${terminalOpen ? "h-[42%] min-h-48" : ""}`}>
            <button
              type="button"
              onClick={() => setTerminalOpen(!terminalOpen)}
              aria-expanded={terminalOpen}
              className="flex shrink-0 items-center gap-1.5 px-3 py-2 text-left text-muted hover:text-fg"
            >
              <Icon className={`size-3 transition-transform duration-150 ease-out ${terminalOpen ? "" : "-rotate-90"}`}>
                <path d="M4 6l4 4 4-4" />
              </Icon>
              <span className="font-medium">{tabLabel("terminal")}</span>
            </button>
            {terminalOpen && panel("terminal")}
          </section>
        </aside>
      )}
    </div>
  );
}

type InspectorTab = "changes" | "checks" | "ports" | "log";
const INSPECTOR = new Set<InspectorTab>(["changes", "checks", "ports", "log"]);

function Count({ children }: { children: ReactNode }) {
  return <span className="ml-1 text-faint tabular-nums">{children}</span>;
}

function HeaderButton({ title, onClick, children }: { title: string; onClick: (e: MouseEvent<HTMLButtonElement>) => void; children: ReactNode }) {
  return (
    <button type="button" title={title} onClick={onClick} className="flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12.5px] text-muted hover:bg-raised hover:text-fg">
      {children}
    </button>
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
      className={`-mb-px shrink-0 border-b-2 px-2 py-2.5 ${active ? "border-fg text-fg" : "border-transparent text-muted hover:text-fg"}`}
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
