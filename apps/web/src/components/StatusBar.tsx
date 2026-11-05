import { useState } from "react";
import type { ConnStatus } from "../lib/useDaemon.ts";
import { notificationsAvailable } from "../lib/useNotifications.ts";
import type { AgentStatus } from "@bantu/protocol";
import { accountKind, agentLabel } from "../lib/useAgents.ts";

const label: Record<ConnStatus, string> = {
  connecting: "Connecting…",
  live: "Connected",
  reconnecting: "Reconnecting…",
  unauthorized: "Token rejected",
};

const tone: Record<ConnStatus, string> = {
  connecting: "bg-warn",
  live: "bg-ok",
  reconnecting: "bg-warn animate-pulse",
  unauthorized: "bg-bad",
};

export function StatusBar({
  status,
  lastSeq,
  daemonVersion,
  error,
  onDismissError,
  onDisconnect,
  agents,
  onOpenAccounts,
  inboxCount,
  onOpenInbox,
}: {
  status: ConnStatus;
  lastSeq: number;
  daemonVersion: string | null;
  error: string | null;
  onDismissError: () => void;
  onDisconnect: () => void;
  agents: AgentStatus[] | null;
  onOpenAccounts: () => void;
  /** Questions, notices and approvals waiting for the user. */
  inboxCount: number;
  onOpenInbox: () => void;
}) {
  const [askable, setAskable] = useState(notificationsAvailable);
  return (
    <footer className="col-span-2 flex items-center gap-4 border-t border-line bg-panel px-3 py-1.5 text-xs text-muted">
      <span className="flex items-center gap-1.5">
        <span className={`size-2 rounded-full ${tone[status]}`} />
        {label[status]}
      </span>
      {daemonVersion && <span>bantud {daemonVersion}</span>}
      <span className="tabular-nums">seq {lastSeq}</span>
      {agents
        ?.filter((a) => a.installed)
        .map((a) => (
          <button
            key={a.agent}
            type="button"
            onClick={onOpenAccounts}
            title={[a.account, a.organization].filter(Boolean).join(" · ") || "Agent account"}
            className="flex items-center gap-1.5 rounded px-1.5 py-0.5 hover:bg-raised hover:text-fg"
          >
            <span className={`size-1.5 rounded-full ${a.logged_in ? "bg-ok" : "bg-warn"}`} />
            {agentLabel(a.agent)} · {accountKind(a)}
          </button>
        ))}
      {error && (
        <button type="button" onClick={onDismissError} className="min-w-0 truncate text-bad" title="Dismiss">
          {error}
        </button>
      )}
      <span className="flex-1" />
      <button
        type="button"
        onClick={onOpenInbox}
        className={`rounded px-1.5 py-0.5 hover:bg-raised hover:text-fg ${inboxCount > 0 ? "font-medium text-warn" : ""}`}
      >
        Inbox{inboxCount > 0 ? ` (${inboxCount})` : ""}
      </button>
      {askable && (
        <button
          type="button"
          onClick={async () => {
            await Notification.requestPermission();
            setAskable(notificationsAvailable());
          }}
          className="rounded px-1.5 py-0.5 hover:bg-raised hover:text-fg"
          title="Get a notification when an agent needs approval or finishes"
        >
          Enable notifications
        </button>
      )}
      <button type="button" onClick={onDisconnect} className="rounded px-1.5 py-0.5 hover:bg-raised hover:text-fg">
        Disconnect
      </button>
    </footer>
  );
}
