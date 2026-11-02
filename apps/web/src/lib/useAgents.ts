import { useCallback, useEffect, useState } from "react";
import type { AgentStatus } from "@bantu/protocol";
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

export function agentLabel(agent: AgentStatus["agent"]) {
  return agent === "claude" ? "Claude" : "Codex";
}

/** "Max plan", "Team plan", "API key", "ChatGPT", … */
export function accountKind(a: AgentStatus): string {
  if (!a.logged_in) return "not signed in";
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
