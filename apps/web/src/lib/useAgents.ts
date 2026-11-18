import { useCallback, useEffect, useState } from "react";
import type { AgentKind, AgentStatus } from "@bantu/protocol";
import type { DaemonClient } from "./api.ts";

/** Agent sign-in status; refreshed on demand and when the window regains focus. */
export function useAgents(client: DaemonClient) {
  const [agents, setAgents] = useState<AgentStatus[] | null>(null);
  const refresh = useCallback(async () => {
    try {
      setAgents(await client.agents());
    } catch {
      // Status is advisory; keep the last known value.
    }
  }, [client]);
  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);
  return { agents, refresh };
}

// The daemon's catalogue sends names too (`AgentStatus.name`); these cover the
// moment before statuses load, and old chats in any list.
const AGENT_LABELS: Record<AgentKind, string> = {
  claude: "Claude",
  codex: "Codex",
  opencode: "OpenCode",
  gemini: "Gemini",
  cursor: "Cursor",
  goose: "Goose",
  qwen: "Qwen Code",
  copilot: "GitHub Copilot",
  auggie: "Auggie",
  kimi: "Kimi",
  kiro: "Kiro",
  cline: "Cline",
  kilo: "Kilo Code",
  vibe: "Mistral Vibe",
  aider: "Aider",
  amp: "Amp",
  crush: "Crush",
  droid: "Droid",
};

/** Agents with no structured interface: they open in a terminal. */
const TERMINAL_ONLY = new Set<AgentKind>(["aider", "amp", "crush", "droid"]);

/** Every agent Bantu can drive, in the order the UI lists them. */
export const AGENT_KINDS = Object.keys(AGENT_LABELS) as AgentKind[];

export function agentLabel(agent: AgentKind) {
  return AGENT_LABELS[agent];
}

/** Whether Bantu can hold a structured chat with this agent. */
export function isChatAgent(agent: AgentKind, agents?: AgentStatus[] | null) {
  const status = agents?.find((a) => a.agent === agent);
  return status ? status.mode !== "terminal" : !TERMINAL_ONLY.has(agent);
}

/** Codex stays visible while status is loading, as it was before more agents existed. */
export function isInstalled(agents: AgentStatus[] | null, agent: AgentKind) {
  const status = agents?.find((a) => a.agent === agent);
  return status ? status.installed : agent === "claude" || agent === "codex";
}

/** "Max plan", "Team plan", "API key", "ChatGPT", … */
export function accountKind(a: AgentStatus): string {
  if (!a.logged_in) return "not signed in";
  // Other agents can't report their account without starting a session.
  if (!a.auth_method && !a.plan && a.agent !== "claude" && a.agent !== "codex") return "installed";
  if (a.plan) return `${a.plan[0].toUpperCase()}${a.plan.slice(1)} plan`;
  switch (a.auth_method) {
    case "claude.ai":
      return "subscription";
    case "chatgpt":
      return "ChatGPT";
    case "console":
    case "api_key":
      return "API key";
    default:
      return a.auth_method ?? "signed in";
  }
}
