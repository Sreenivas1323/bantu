import { useEffect, useState, type FormEvent } from "react";
import type { AgentKind, AgentStatus, Command, CommandResponse, LoginMethod, Org, Preferences, Repo, Terminal, Tracker } from "@bantu/protocol";
import type { DaemonClient } from "../lib/api.ts";
import { accountKind, agentLabel } from "../lib/useAgents.ts";
import { TerminalView } from "./terminal/TerminalView.tsx";
import { claudeGuidance, orgTrackers, syncLine } from "../lib/org.ts";
import { Dialog } from "./Dialog.tsx";

// Agents with more than one way in; the rest get one button for their own flow.
const METHODS: Partial<Record<AgentKind, { method: LoginMethod; label: string; hint: string }[]>> = {
  claude: [
    { method: "subscription", label: "Claude subscription", hint: "Pro, Max or Team: turns use your plan's limits" },
    { method: "sso", label: "SSO", hint: "Team or Enterprise with single sign-on" },
    { method: "console", label: "API key", hint: "Anthropic Console, billed per use" },
  ],
  codex: [
    { method: "subscription", label: "ChatGPT", hint: "Use your ChatGPT plan" },
    { method: "console", label: "API key", hint: "OpenAI API, billed per use" },
  ],
  opencode: [{ method: "subscription", label: "OpenCode", hint: "Pick a provider in OpenCode's own sign-in" }],
  gemini: [{ method: "subscription", label: "Google", hint: "Sign in from Gemini CLI's first-run screen" }],
  cursor: [{ method: "subscription", label: "Cursor", hint: "Sign in to your Cursor account" }],
};

/**
 * Which accounts the agents use, and signing in. Bantu runs your own
 * `claude` / `codex`, so sign-in happens in their own flow (shown here as a
 * terminal) and credentials never pass through Bantu.
 */
export function AccountsDialog({
  agents,
  terminals,
  preferences,
  org,
  orgEnded,
  trackers,
  repos,
  client,
  run,
  onRefresh,
  onClose,
}: {
  agents: AgentStatus[] | null;
  terminals: Record<string, Terminal>;
  preferences: Preferences;
  org: Org | null;
  orgEnded: string | null;
  trackers: Tracker[];
  repos: Record<string, Repo>;
  client: DaemonClient;
  run: (command: Command) => Promise<CommandResponse | null>;
  onRefresh: () => Promise<void>;
  onClose: () => void;
}) {
  const [loginId, setLoginId] = useState<string | null>(null);
  const login = loginId ? terminals[loginId] : undefined;

  // When the sign-in flow finishes, show the new status.
  useEffect(() => {
    if (login?.exited) void onRefresh();
  }, [login?.exited, onRefresh]);

  const start = async (agent: AgentKind, method: LoginMethod) => {
    const res = await run({ type: "agent.login", agent, method });
    const t = res?.result as Terminal | undefined;
    if (t?.id) setLoginId(t.id);
  };

  return (
    <Dialog title="Agent accounts" width={720} onClose={onClose}>
      <div className="flex flex-col gap-3 p-4">
        <p className="text-[12.5px] text-muted">
          Bantu runs the agent CLIs installed on this machine, so chats use whatever account they're signed in to. Your
          subscription's limits apply; Bantu never sees your credentials.
        </p>
        <OrganizationSection org={org} ended={orgEnded} trackers={orgTrackers(trackers, org)} repos={repos} run={run} />
        {!agents && <p className="text-muted">Checking…</p>}
        {agents?.filter((a) => a.installed || a.agent === "claude").map((a) => (
          <section key={a.agent} className="rounded-md border border-line p-3">
            <div className="flex items-baseline gap-2">
              <span className={`size-2 shrink-0 self-center rounded-full ${a.logged_in ? "bg-ok" : a.installed ? "bg-warn" : "bg-faint"}`} />
              <span className="font-medium">{a.name}</span>
              {a.version && <span className="font-mono text-[11.5px] text-faint">{a.version}</span>}
              {a.mode === "terminal" && <span className="text-[11.5px] text-faint">terminal only</span>}
              <span className="flex-1" />
              <span className={a.logged_in ? "text-ok" : "text-warn"}>{a.installed ? accountKind(a) : "not installed"}</span>
            </div>
            {a.logged_in && (a.account || a.organization) && (
              <p className="mt-1 pl-4 text-[12.5px] text-muted">
                {[a.account, a.organization].filter(Boolean).join(" · ")}
              </p>
            )}
            {a.installed ? (
              <div className="mt-2.5 flex flex-wrap items-center gap-1.5 pl-4">
                {a.can_sign_in && <span className="mr-0.5 text-[12px] text-faint">{a.logged_in ? "Switch account" : "Sign in with"}</span>}
                {!a.can_sign_in && <span className="text-[12.5px] text-muted">Reads provider API keys from your environment.</span>}
                {(a.can_sign_in ? (METHODS[a.agent] ?? [{ method: "subscription" as const, label: a.name, hint: `${a.name}'s own sign-in` }]) : []).map((m) => (
                  <button
                    key={m.method}
                    type="button"
                    onClick={() => void start(a.agent, m.method)}
                    title={m.hint}
                    className="rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:bg-raised hover:text-fg"
                  >
                    {m.label}
                  </button>
                ))}
              </div>
            ) : (
              <p className="mt-2 pl-4 text-[12.5px] text-muted">
                Install it, then reopen this dialog:{" "}
                <code className="rounded bg-bg px-1.5 py-0.5 font-mono text-[12px]">{a.install_hint}</code>
              </p>
            )}
          </section>
        ))}
        {agents && agents.some((a) => !a.installed && a.agent !== "claude") && (
          <details className="rounded-md border border-line p-3">
            <summary className="cursor-pointer text-muted hover:text-fg">
              More agents ({agents.filter((a) => !a.installed && a.agent !== "claude").length} not installed)
            </summary>
            <p className="mt-2 text-[12.5px] text-muted">
              Install one, then reopen this dialog. Chat agents work in the chat; terminal-only ones open in a workspace terminal.
            </p>
            <ul className="mt-2 flex flex-col gap-2">
              {agents
                .filter((a) => !a.installed && a.agent !== "claude")
                .map((a) => (
                  <li key={a.agent} className="flex flex-col gap-0.5">
                    <span className="flex items-baseline gap-2">
                      <a href={a.docs_url} target="_blank" rel="noreferrer" className="hover:underline">
                        {a.name}
                      </a>
                      <span className="text-[11.5px] text-faint">{a.mode === "terminal" ? "terminal only" : "chat"}</span>
                    </span>
                    <code className="w-fit rounded bg-bg px-1.5 py-0.5 font-mono text-[12px] break-all select-all">{a.install_hint}</code>
                  </li>
                ))}
            </ul>
          </details>
        )}
        <label className="flex cursor-pointer items-start gap-2.5 rounded-md border border-line p-3">
          <input
            type="checkbox"
            checked={preferences.claude_user_setup}
            onChange={(e) => void run({ type: "preferences.update", claude_user_setup: e.target.checked, loadouts: null })}
            className="mt-0.5 accent-accent"
          />
          <span className="flex flex-col gap-0.5">
            <span>Load my Claude setup in Bantu chats</span>
            <span className="text-[12.5px] text-muted">
              Your global plugins, hooks, skills, MCP servers and <code className="font-mono">~/.claude/CLAUDE.md</code>.
              Off by default, which cuts each turn's input to about a third and saves your plan's limits. The repo's
              own <code className="font-mono">CLAUDE.md</code> and settings always load.
            </span>
          </span>
        </label>
        {preferences.loadouts.length > 0 && (
          <section className="rounded-md border border-line p-3">
            <h3 className="font-medium">Loadouts</h3>
            <p className="mb-2 text-[12.5px] text-muted">Saved from a chat's settings. They appear next to + New chat.</p>
            <ul className="flex flex-col gap-1">
              {preferences.loadouts.map((l) => (
                <li key={l.name} className="flex items-center gap-2">
                  <span className="min-w-0 flex-1 truncate">
                    {l.name} <span className="text-[12px] text-faint">{agentLabel(l.agent)}{l.model ? ` · ${l.model}` : ""}</span>
                  </span>
                  <button
                    type="button"
                    onClick={() =>
                      void run({
                        type: "preferences.update",
                        claude_user_setup: null,
                        loadouts: preferences.loadouts.filter((other) => other.name !== l.name),
                      })
                    }
                    className="text-[12px] text-faint hover:text-bad"
                  >
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}
        {login && (
          <section className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between text-[12.5px]">
              <span className="text-muted">
                {login.title}
                {login.exited && (login.exit_code === 0 ? " · done" : ` · exited ${login.exit_code ?? ""}`)}
              </span>
              <span className="text-faint">Follow the steps; a browser window may open.</span>
            </div>
            <div className="h-56 overflow-hidden rounded-md border border-line">
              <TerminalView socketUrl={client.terminalSocketUrl(login.id)} visible />
            </div>
          </section>
        )}
      </div>
    </Dialog>
  );
}

/**
 * Membership of an organization through Bantu Connect: join with a link, or
 * see what the org manages here and leave it.
 */
function OrganizationSection({
  org,
  ended,
  trackers,
  repos,
  run,
}: {
  org: Org | null;
  ended: string | null;
  trackers: Tracker[];
  repos: Record<string, Repo>;
  run: (command: Command) => Promise<CommandResponse | null>;
}) {
  const [link, setLink] = useState("");
  const [busy, setBusy] = useState<"join" | "sync" | "leave" | null>(null);

  const act = async (kind: "join" | "sync" | "leave", command: Command) => {
    setBusy(kind);
    try {
      return await run(command);
    } finally {
      setBusy(null);
    }
  };

  if (!org) {
    const submit = async (e: FormEvent) => {
      e.preventDefault();
      if (!link.trim()) return;
      const res = await act("join", { type: "org.join", link: link.trim() });
      if (res?.ok) setLink("");
    };
    return (
      <section className="rounded-md border border-line p-3">
        <h3 className="font-medium">Organization</h3>
        {ended && <p className="mt-1 text-[12.5px] text-warn">{ended}</p>}
        <p className="mt-1 text-[12.5px] text-muted">
          Join your team's Bantu Connect to get its trackers and Claude settings here. Paste the join link an admin sent you.
        </p>
        <form onSubmit={(e) => void submit(e)} className="mt-2 flex gap-2">
          <label className="sr-only" htmlFor="org-join-link">
            Join link
          </label>
          <input
            id="org-join-link"
            value={link}
            onChange={(e) => setLink(e.target.value)}
            placeholder="https://connect.example.com/c/…"
            spellCheck={false}
            autoComplete="off"
            className="min-w-0 flex-1 rounded-md border border-line bg-bg px-2 py-1 font-mono text-[16px] sm:text-[13px]"
          />
          <button
            type="submit"
            disabled={busy !== null || !link.trim()}
            className="pressable rounded-md bg-accent px-3 py-1 text-[12px] font-medium text-bg disabled:opacity-40"
          >
            {busy === "join" ? "Joining…" : "Join"}
          </button>
        </form>
      </section>
    );
  }

  const sync = syncLine(org);
  const guidance = claudeGuidance(org.claude.mode, org.claude.plan);
  return (
    <section className="rounded-md border border-line p-3">
      <div className="flex items-baseline gap-2">
        <span className={`size-2 shrink-0 self-center rounded-full ${sync.failed ? "bg-bad" : "bg-ok"}`} />
        <h3 className="font-medium">{org.name}</h3>
        <span className="text-[12px] text-faint">
          {org.member.login} · {org.member.role}
        </span>
        <span className="flex-1" />
        <button
          type="button"
          disabled={busy !== null}
          onClick={() => void act("sync", { type: "org.sync" })}
          className="rounded-md border border-line px-2 py-0.5 text-[12px] text-muted hover:bg-raised hover:text-fg disabled:opacity-50"
        >
          {busy === "sync" ? "Syncing…" : "Sync now"}
        </button>
      </div>
      <p className={`mt-1 pl-4 text-[12.5px] ${sync.failed ? "text-bad" : "text-muted"}`}>
        {sync.text}
        {sync.failed && " · Using the last settings it sent."}
      </p>
      <div className="mt-2.5 flex flex-col gap-2 pl-4 text-[12.5px]">
        <div>
          <p className="text-fg">Claude: {guidance.title}</p>
          <p className="text-muted">{guidance.body}</p>
        </div>
        <div>
          <p className="text-fg">Trackers from {org.name}</p>
          {trackers.length === 0 ? (
            <p className="text-muted">None yet.</p>
          ) : (
            <ul className="flex flex-col gap-0.5 text-muted">
              {trackers.map((t) => (
                <li key={t.id} className="flex items-center gap-1.5">
                  <span aria-hidden className={`size-1.5 rounded-full ${t.last_error ? "bg-bad" : "bg-ok"}`} />
                  <span className="truncate" title={t.last_error ?? undefined}>
                    {t.name} <span className="text-faint">on {repos[t.repo_id]?.name ?? "a repo"}</span>
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
        <button
          type="button"
          disabled={busy !== null}
          onClick={() => {
            if (
              confirm(
                `Leave ${org.name}? Its trackers and Claude settings are removed from this machine. Workspaces you started from its tasks are kept.`,
              )
            )
              void act("leave", { type: "org.leave" });
          }}
          className="self-start rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:border-bad hover:text-bad disabled:opacity-50"
        >
          {busy === "leave" ? "Leaving…" : "Leave organization"}
        </button>
      </div>
    </section>
  );
}
