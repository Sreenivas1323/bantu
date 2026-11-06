import { useEffect, useRef, useState, type FormEvent } from "react";
import type { AgentKind, AgentStatus, Command, CommandResponse, Loop, LoopTrigger, PermissionMode, Repo, Workspace } from "@bantu/protocol";
import { AGENT_KINDS, agentLabel } from "../lib/useAgents.ts";

type TriggerKind = LoopTrigger["kind"];

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "accept_edits", label: "Accept edits" },
  { value: "plan", label: "Plan (read-only)" },
  { value: "ask", label: "Ask first" },
  { value: "bypass_permissions", label: "Bypass permissions" },
];

function describe(trigger: LoopTrigger): string {
  switch (trigger.kind) {
    case "cron":
      return trigger.schedule;
    case "github_issue":
      return `New issues labelled ${trigger.label}`;
    case "github_pull_request":
      return `New PRs labelled ${trigger.label}`;
  }
}

/** Agents that start on their own, each run in a fresh workspace. */
export function LoopsDialog({
  repo,
  loops,
  workspaces,
  agents,
  run,
  onSelectWorkspace,
  onClose,
}: {
  repo: Repo;
  loops: Loop[];
  workspaces: Record<string, Workspace>;
  agents: AgentStatus[] | null;
  run: (command: Command) => Promise<CommandResponse | null>;
  onSelectWorkspace: (id: string) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
  }, []);
  const sorted = [...loops].sort((a, b) => a.created_at.localeCompare(b.created_at));

  return (
    <dialog
      ref={dialog}
      onClose={onClose}
      className="m-auto max-h-[85dvh] w-[min(640px,calc(100vw-2rem))] rounded-lg border border-line bg-panel p-0 text-fg backdrop:bg-black/60"
    >
      <div className="flex items-center justify-between border-b border-line px-4 py-3">
        <h2 className="font-medium">Loops in {repo.name}</h2>
        <button type="button" onClick={() => dialog.current?.close()} className="px-1 text-muted hover:text-fg" aria-label="Close">
          ✕
        </button>
      </div>
      <div className="flex flex-col gap-3 p-4">
        <p className="text-[12.5px] text-muted">
          Each run gets a fresh workspace and spends your agent's quota. A run that changes nothing archives itself. Loops only run while this daemon is on.
        </p>
        {sorted.map((l) => (
          <section key={l.id} className="rounded-md border border-line p-3">
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span aria-hidden className={`size-2 rounded-full ${l.enabled ? "bg-ok" : "bg-faint"}`} />
              <span className="font-medium">{l.name}</span>
              <span className="font-mono text-[12px] text-muted">{describe(l.trigger)}</span>
              <span className="flex-1" />
              <button type="button" onClick={() => void run({ type: "loop.run", loop_id: l.id })} className="rounded-md border border-line px-2 py-0.5 text-[12px] text-muted hover:bg-raised hover:text-fg">
                Run now
              </button>
              <button
                type="button"
                onClick={() => void run({ type: "loop.update", loop_id: l.id, enabled: !l.enabled, prompt: null })}
                className="rounded-md border border-line px-2 py-0.5 text-[12px] text-muted hover:bg-raised hover:text-fg"
              >
                {l.enabled ? "Pause" : "Resume"}
              </button>
              <button
                type="button"
                onClick={() => {
                  if (confirm(`Delete the loop ${l.name}? Workspaces from its runs are kept.`)) void run({ type: "loop.delete", loop_id: l.id });
                }}
                className="rounded-md px-2 py-0.5 text-[12px] text-faint hover:text-bad"
              >
                Delete
              </button>
            </div>
            <p className="mt-1.5 line-clamp-2 whitespace-pre-wrap text-[12.5px] text-muted">{l.prompt}</p>
            <p className="mt-1 text-[12px] text-faint">
              {agentLabel(l.agent)} · {MODES.find((m) => m.value === l.permission_mode)?.label}
              {l.next_run_at && ` · next ${new Date(l.next_run_at).toLocaleString()}`}
            </p>
            {l.last_result && (
              <p className="mt-1 text-[12px] text-muted">
                Last run: {l.last_result}
                {l.last_workspace_id && workspaces[l.last_workspace_id] && workspaces[l.last_workspace_id].state !== "archived" && (
                  <>
                    {" · "}
                    <button type="button" onClick={() => onSelectWorkspace(l.last_workspace_id!)} className="underline underline-offset-2 hover:text-fg">
                      Open
                    </button>
                  </>
                )}
              </p>
            )}
          </section>
        ))}
        <NewLoop repoId={repo.id} agents={agents} run={run} />
      </div>
    </dialog>
  );
}

function NewLoop({ repoId, agents, run }: { repoId: string; agents: AgentStatus[] | null; run: (command: Command) => Promise<CommandResponse | null> }) {
  const [name, setName] = useState("");
  const [kind, setKind] = useState<TriggerKind>("cron");
  const [value, setValue] = useState("0 3 * * *");
  const [prompt, setPrompt] = useState("");
  const [agent, setAgent] = useState<AgentKind>("claude");
  const [mode, setMode] = useState<PermissionMode>("auto");
  const [busy, setBusy] = useState(false);

  const isCron = kind === "cron";
  const github = !isCron;
  const installed = AGENT_KINDS.filter((a) => a === "claude" || agents?.find((s) => s.agent === a)?.installed);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !prompt.trim() || !value.trim()) return;
    const trigger: LoopTrigger = kind === "cron" ? { kind, schedule: value.trim() } : { kind, label: value.trim() };
    setBusy(true);
    try {
      const res = await run({ type: "loop.create", repo_id: repoId, name: name.trim(), trigger, prompt: prompt.trim(), agent, model: null, permission_mode: mode });
      if (res) {
        setName("");
        setPrompt("");
      }
    } finally {
      setBusy(false);
    }
  };

  const field = "w-full rounded-md border border-line bg-bg px-2 py-1 text-[16px] sm:text-[13px]";
  return (
    <form onSubmit={(e) => void submit(e)} className="flex flex-col gap-2 rounded-md border border-dashed border-line p-3">
      <h3 className="font-medium">New loop</h3>
      <label className="flex flex-col gap-1 text-[12.5px] text-muted">
        Name
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Nightly dependency check" className={field} />
      </label>
      <div className="grid gap-2 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-[12.5px] text-muted">
          Starts
          <select
            value={kind}
            onChange={(e) => {
              const next = e.target.value as TriggerKind;
              setKind(next);
              setValue(next === "cron" ? "0 3 * * *" : "agent");
              if (next !== "cron" && mode === "bypass_permissions") setMode("auto");
            }}
            className={field}
          >
            <option value="cron">On a schedule</option>
            <option value="github_issue">When a GitHub issue gets a label</option>
            <option value="github_pull_request">When a GitHub PR gets a label</option>
          </select>
        </label>
        <label className="flex flex-col gap-1 text-[12.5px] text-muted">
          {isCron ? "Cron schedule" : "Label"}
          <input value={value} onChange={(e) => setValue(e.target.value)} spellCheck={false} className={`${field} font-mono`} />
        </label>
      </div>
      {isCron && <p className="text-[12px] text-faint">Minute, hour, day of month, month, day of week, in the daemon's time zone. 0 3 * * * is every day at 3:00.</p>}
      {github && <p className="text-[12px] text-faint">The issue's text goes to the agent, so a label makes sure someone with triage rights chose it. Forks' PRs are skipped.</p>}
      <label className="flex flex-col gap-1 text-[12.5px] text-muted">
        What should the agent do?
        <textarea value={prompt} onChange={(e) => setPrompt(e.target.value)} rows={3} className={field} />
      </label>
      <div className="grid gap-2 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-[12.5px] text-muted">
          Agent
          <select value={agent} onChange={(e) => setAgent(e.target.value as AgentKind)} className={field}>
            {installed.map((a) => (
              <option key={a} value={a}>
                {agentLabel(a)}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1 text-[12.5px] text-muted">
          Permissions
          <select value={mode} onChange={(e) => setMode(e.target.value as PermissionMode)} className={field}>
            {MODES.filter((m) => isCron || m.value !== "bypass_permissions").map((m) => (
              <option key={m.value} value={m.value}>
                {m.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      <button type="submit" disabled={busy || !name.trim() || !prompt.trim() || !value.trim()} className="self-start rounded-md bg-accent px-3 py-1 text-[12px] font-medium text-bg disabled:opacity-40">
        {busy ? "Creating…" : "Create loop"}
      </button>
    </form>
  );
}
