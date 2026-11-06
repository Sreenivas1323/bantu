import { useEffect, useRef, useState } from "react";
import type { AgentKind, AgentStatus, Command, CommandResponse, LoginMethod, Preferences, Terminal } from "@bantu/protocol";
import type { DaemonClient } from "../lib/api.ts";
import { accountKind, agentLabel } from "../lib/useAgents.ts";
import { TerminalView } from "./terminal/TerminalView.tsx";
import { afterExit } from "../lib/motion.ts";

const INSTALL: Record<AgentKind, string> = {
  claude: "curl -fsSL https://claude.ai/install.sh | bash",
  codex: "npm install -g @openai/codex",
  opencode: "curl -fsSL https://opencode.ai/install | bash",
  gemini: "npm install -g @google/gemini-cli",
  cursor: "curl https://cursor.com/install -fsS | bash",
};

const METHODS: Record<AgentKind, { method: LoginMethod; label: string; hint: string }[]> = {
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
  client,
  run,
  onRefresh,
  onClose,
}: {
  agents: AgentStatus[] | null;
  terminals: Record<string, Terminal>;
  preferences: Preferences;
  client: DaemonClient;
  run: (command: Command) => Promise<CommandResponse | null>;
  onRefresh: () => Promise<void>;
  onClose: () => void;
}) {
  const [loginId, setLoginId] = useState<string | null>(null);
  const login = loginId ? terminals[loginId] : undefined;
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    dialog.current?.showModal();
  }, []);
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
    <dialog
      ref={dialog}
      onClose={() => afterExit(onClose)}
      className="m-auto w-[min(720px,calc(100vw-2rem))] rounded-lg border border-line bg-panel p-0 text-fg backdrop:bg-black/60"
    >
      <div className="flex items-center justify-between border-b border-line px-4 py-3">
        <h2 className="font-medium">Agent accounts</h2>
        <button type="button" onClick={() => dialog.current?.close()} className="px-1 text-muted hover:text-fg" aria-label="Close">
          ✕
        </button>
      </div>
      <div className="flex flex-col gap-3 p-4">
        <p className="text-[12.5px] text-muted">
          Bantu runs the agent CLIs installed on this machine, so chats use whatever account they're signed in to. Your
          subscription's limits apply; Bantu never sees your credentials.
        </p>
        {!agents && <p className="text-muted">Checking…</p>}
        {agents?.map((a) => (
          <section key={a.agent} className="rounded-md border border-line p-3">
            <div className="flex items-baseline gap-2">
              <span className={`size-2 shrink-0 self-center rounded-full ${a.logged_in ? "bg-ok" : a.installed ? "bg-warn" : "bg-faint"}`} />
              <span className="font-medium">{agentLabel(a.agent)}</span>
              {a.version && <span className="font-mono text-[11.5px] text-faint">{a.version}</span>}
              <span className="flex-1" />
              <span className={a.logged_in ? "text-ok" : "text-warn"}>{a.installed ? accountKind(a) : "not installed"}</span>
            </div>
            {a.logged_in && (a.account || a.organization) && (
              <p className="mt-1 pl-4 text-[12.5px] text-muted">
                {[a.account, a.organization].filter(Boolean).join(" · ")}
              </p>
            )}
            {a.installed ? (
              <div className="mt-2.5 flex flex-wrap gap-1.5 pl-4">
                {METHODS[a.agent].map((m) => (
                  <button
                    key={m.method}
                    type="button"
                    onClick={() => void start(a.agent, m.method)}
                    title={m.hint}
                    className="rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:bg-raised hover:text-fg"
                  >
                    {a.logged_in ? "Switch account:" : "Sign in with"} {m.label}
                  </button>
                ))}
              </div>
            ) : (
              <p className="mt-2 pl-4 text-[12.5px] text-muted">
                Install it, then reopen this dialog:{" "}
                <code className="rounded bg-bg px-1.5 py-0.5 font-mono text-[12px]">{INSTALL[a.agent]}</code>
              </p>
            )}
          </section>
        ))}
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
    </dialog>
  );
}
