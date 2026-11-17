import { useEffect, useRef, useState, type FormEvent } from "react";
import type { Command, CommandResponse, Repo, Task, Tracker, TrackerKind } from "@bantu/protocol";
import type { DaemonClient } from "../lib/api.ts";
import { afterExit } from "../lib/motion.ts";

const KINDS: { kind: TrackerKind; label: string }[] = [
  { kind: "github", label: "GitHub Issues" },
  { kind: "jira", label: "Jira" },
  { kind: "clickup", label: "ClickUp" },
];

/** A repo's tasks from its trackers, and a way to start an agent on one. */
export function TasksDialog({
  repo,
  trackers,
  tasksVersion,
  client,
  run,
  onSelectWorkspace,
  onClose,
}: {
  repo: Repo;
  trackers: Tracker[];
  /** Changes whenever the daemon says tasks changed. */
  tasksVersion: number;
  client: DaemonClient;
  run: (command: Command) => Promise<CommandResponse | null>;
  onSelectWorkspace: (id: string) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [mineOnly, setMineOnly] = useState(true);
  const [starting, setStarting] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    dialog.current?.showModal();
  }, []);

  useEffect(() => {
    let cancelled = false;
    client.tasks().then(
      (all) => !cancelled && setTasks(all),
      () => !cancelled && setTasks([]),
    );
    return () => {
      cancelled = true;
    };
  }, [client, tasksVersion]);

  const ids = new Set(trackers.map((t) => t.id));
  const byId = Object.fromEntries(trackers.map((t) => [t.id, t]));
  const mine = tasks?.filter((t) => ids.has(t.tracker_id)) ?? [];
  const shown = mineOnly ? mine.filter((t) => t.mine) : mine;

  const start = async (task: Task) => {
    if (task.workspace_id) {
      onSelectWorkspace(task.workspace_id);
      return;
    }
    setStarting(`${task.tracker_id}/${task.id}`);
    try {
      const res = await run({ type: "task.start", tracker_id: task.tracker_id, task_id: task.id });
      const ws = res?.result as { id: string } | undefined;
      if (ws) onSelectWorkspace(ws.id);
    } finally {
      setStarting(null);
    }
  };

  return (
    <dialog
      ref={dialog}
      onClose={() => afterExit(onClose)}
      className="m-auto max-h-[85dvh] w-[min(720px,calc(100vw-2rem))] rounded-lg border border-line bg-panel p-0 text-fg backdrop:bg-black/60"
    >
      <div className="flex items-center justify-between border-b border-line px-4 py-3">
        <h2 className="font-medium">Tasks for {repo.name}</h2>
        <button type="button" onClick={() => dialog.current?.close()} className="px-1 text-muted hover:text-fg" aria-label="Close">
          ✕
        </button>
      </div>
      <div className="flex flex-col gap-3 p-4">
        {trackers.length === 0 && !adding && (
          <p className="text-[12.5px] text-muted">
            Connect GitHub Issues, Jira or ClickUp to work on tasks from there. Starting one gives an agent its own workspace, and it posts progress and the
            pull request back to the task.
          </p>
        )}
        {trackers.length > 0 && (
          <ul className="flex flex-wrap gap-2 text-[12px]">
            {trackers.map((t) => (
              <li key={t.id} className="flex items-center gap-1.5 rounded-md border border-line px-2 py-1">
                <span aria-hidden className={`size-1.5 rounded-full ${t.last_error ? "bg-bad" : "bg-ok"}`} />
                <span title={t.last_error ?? (t.last_synced_at ? `Synced ${new Date(t.last_synced_at).toLocaleTimeString()}` : undefined)}>{t.name}</span>
                <button
                  type="button"
                  onClick={() => {
                    if (confirm(`Disconnect ${t.name}? Workspaces already started from it are kept.`)) void run({ type: "tracker.remove", tracker_id: t.id });
                  }}
                  className="text-faint hover:text-bad"
                  aria-label={`Disconnect ${t.name}`}
                >
                  ✕
                </button>
              </li>
            ))}
            <li>
              <button type="button" onClick={() => setAdding((v) => !v)} className="rounded-md px-2 py-1 text-muted hover:bg-raised hover:text-fg">
                + Connect
              </button>
            </li>
          </ul>
        )}
        {trackers.some((t) => t.last_error) && (
          <p className="text-[12px] text-bad">
            {trackers
              .filter((t) => t.last_error)
              .map((t) => `${t.name}: ${t.last_error}`)
              .join(" · ")}
          </p>
        )}
        {(adding || trackers.length === 0) && <ConnectTracker repoId={repo.id} run={run} onDone={() => setAdding(false)} />}

        {trackers.length > 0 && (
          <>
            <div className="flex items-center gap-3 text-[12.5px]">
              <label className="flex items-center gap-1.5 text-muted">
                <input type="checkbox" checked={mineOnly} onChange={(e) => setMineOnly(e.target.checked)} />
                Assigned to me
              </label>
              <span className="flex-1" />
              <button
                type="button"
                disabled={refreshing}
                onClick={async () => {
                  setRefreshing(true);
                  try {
                    await run({ type: "tasks.refresh", tracker_id: null });
                  } finally {
                    setRefreshing(false);
                  }
                }}
                className="rounded-md border border-line px-2 py-0.5 text-muted hover:bg-raised hover:text-fg disabled:opacity-50"
              >
                {refreshing ? "Refreshing…" : "Refresh"}
              </button>
            </div>
            {tasks === null && <p className="text-muted">Loading…</p>}
            {tasks !== null && shown.length === 0 && (
              <p className="rounded-md border border-line px-3 py-6 text-center text-muted">
                {mineOnly && mine.length > 0 ? "Nothing open is assigned to you." : "No open tasks."}
              </p>
            )}
            <ul className="flex flex-col divide-y divide-line rounded-md border border-line">
              {shown.map((t) => {
                const key = `${t.tracker_id}/${t.id}`;
                return (
                  <li key={key} className="flex items-start gap-3 px-3 py-2.5">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline gap-2">
                        <a href={t.url} target="_blank" rel="noreferrer" className="shrink-0 font-mono text-[12px] text-muted hover:text-fg hover:underline">
                          {t.key}
                        </a>
                        <span className="truncate font-medium" title={t.title}>
                          {t.title}
                        </span>
                      </div>
                      <p className="mt-0.5 text-[12px] text-faint">
                        {byId[t.tracker_id]?.name} · {t.status}
                        {t.assignee && ` · ${t.assignee}`}
                      </p>
                    </div>
                    <button
                      type="button"
                      disabled={starting === key}
                      onClick={() => void start(t)}
                      className={`shrink-0 rounded-md px-2.5 py-1 text-[12px] font-medium disabled:opacity-50 ${
                        t.workspace_id ? "border border-line text-muted hover:bg-raised hover:text-fg" : "pressable bg-accent text-bg"
                      }`}
                    >
                      {t.workspace_id ? "Open" : starting === key ? "Starting…" : "Start"}
                    </button>
                  </li>
                );
              })}
            </ul>
          </>
        )}
      </div>
    </dialog>
  );
}

function ConnectTracker({ repoId, run, onDone }: { repoId: string; run: (command: Command) => Promise<CommandResponse | null>; onDone: () => void }) {
  const [kind, setKind] = useState<TrackerKind>("github");
  const [site, setSite] = useState("");
  const [scope, setScope] = useState("");
  const [account, setAccount] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);

  const field = "w-full rounded-md border border-line bg-bg px-2 py-1 text-[16px] sm:text-[13px]";
  const label = "flex flex-col gap-1 text-[12.5px] text-muted";
  const ready = kind === "github" || (kind === "jira" ? site && account && token && scope : token && scope);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    setBusy(true);
    try {
      const res = await run({
        type: "tracker.add",
        repo_id: repoId,
        kind,
        site: kind === "jira" ? site : null,
        scope: scope || null,
        account: kind === "jira" ? account : null,
        token: kind === "github" ? null : token,
      });
      if (res?.ok) {
        setToken("");
        onDone();
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex flex-col gap-2 rounded-md border border-dashed border-line p-3">
      <div className="flex gap-1" role="radiogroup" aria-label="Tracker">
        {KINDS.map((k) => (
          <button
            key={k.kind}
            type="button"
            role="radio"
            aria-checked={kind === k.kind}
            onClick={() => setKind(k.kind)}
            className={`rounded-md px-2.5 py-1 text-[12.5px] ${kind === k.kind ? "bg-raised text-fg" : "text-muted hover:text-fg"}`}
          >
            {k.label}
          </button>
        ))}
      </div>
      {kind === "github" && (
        <>
          <p className="text-[12px] text-faint">Uses this repository's GitHub issues through your `gh` login.</p>
          <label className={label}>
            Only issues with this label (optional)
            <input value={scope} onChange={(e) => setScope(e.target.value)} placeholder="e.g. agent" className={field} />
          </label>
        </>
      )}
      {kind === "jira" && (
        <div className="grid gap-2 sm:grid-cols-2">
          <label className={label}>
            Site
            <input value={site} onChange={(e) => setSite(e.target.value)} placeholder="https://acme.atlassian.net" className={field} />
          </label>
          <label className={label}>
            Project key or JQL
            <input value={scope} onChange={(e) => setScope(e.target.value)} placeholder="WEB" spellCheck={false} className={`${field} font-mono`} />
          </label>
          <label className={label}>
            Your Atlassian email
            <input value={account} onChange={(e) => setAccount(e.target.value)} type="email" autoComplete="email" className={field} />
          </label>
          <label className={label}>
            API token
            <input value={token} onChange={(e) => setToken(e.target.value)} type="password" autoComplete="off" className={field} />
          </label>
          <p className="text-[12px] text-faint sm:col-span-2">Create a token at id.atlassian.com → Security → API tokens.</p>
        </div>
      )}
      {kind === "clickup" && (
        <div className="grid gap-2 sm:grid-cols-2">
          <label className={label}>
            List id
            <input value={scope} onChange={(e) => setScope(e.target.value)} placeholder="901234567" spellCheck={false} className={`${field} font-mono`} />
          </label>
          <label className={label}>
            Personal API token
            <input value={token} onChange={(e) => setToken(e.target.value)} type="password" autoComplete="off" placeholder="pk_…" className={field} />
          </label>
          <p className="text-[12px] text-faint sm:col-span-2">
            The list id is the number after <span className="font-mono">/li/</span> in the list's URL. Tokens are under Settings → Apps.
          </p>
        </div>
      )}
      <p className="text-[12px] text-faint">The token stays on the machine running Bantu, readable only by you, and is never sent to the app.</p>
      <button type="submit" disabled={busy || !ready} className="pressable self-start rounded-md bg-accent px-3 py-1 text-[12px] font-medium text-bg disabled:opacity-40">
        {busy ? "Checking…" : "Connect"}
      </button>
    </form>
  );
}
