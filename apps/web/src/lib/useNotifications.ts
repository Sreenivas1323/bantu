import { useEffect, useRef } from "react";
import type { DaemonState } from "./store.ts";

/**
 * Desktop notifications for the moments that need a human: an agent asking
 * for approval, and an agent finishing its work. Only fires while the page is
 * hidden (you're in another app), and never for state that existed on load.
 * The tab title also counts pending approvals.
 */
export function useNotifications(state: DaemonState) {
  const prev = useRef<{ permissions: Set<string>; running: Set<string> } | null>(null);

  useEffect(() => {
    const pending = Object.keys(state.permissions).length;
    document.title = pending > 0 ? `(${pending}) Bantu` : "Bantu";
  }, [state.permissions]);

  useEffect(() => {
    const permissions = new Set(Object.keys(state.permissions));
    const running = new Set(
      Object.values(state.sessions)
        .filter((s) => s.status === "running")
        .map((s) => s.id),
    );
    const before = prev.current;
    prev.current = { permissions, running };
    if (!before || !canNotify() || !document.hidden) return;

    for (const id of permissions) {
      if (before.permissions.has(id)) continue;
      const p = state.permissions[id];
      const ws = state.workspaces[p.workspace_id];
      show(`${ws?.name ?? "An agent"} needs your approval`, p.tool_name, `perm-${id}`);
    }
    for (const id of before.running) {
      if (running.has(id)) continue;
      const s = state.sessions[id];
      if (!s) continue;
      const ws = state.workspaces[s.workspace_id];
      show(`${ws?.name ?? "Agent"} is done`, s.title, `done-${id}`);
    }
  }, [state.permissions, state.sessions, state.workspaces]);
}

export function canNotify(): boolean {
  return typeof Notification !== "undefined" && Notification.permission === "granted";
}

export function notificationsAvailable(): boolean {
  return typeof Notification !== "undefined" && Notification.permission === "default";
}

function show(title: string, body: string, tag: string) {
  const n = new Notification(title, { body, tag });
  n.onclick = () => {
    window.focus();
    n.close();
  };
}
