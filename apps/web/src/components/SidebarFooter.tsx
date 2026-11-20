import { useEffect, useState } from "react";
import type { AgentStatus } from "@bantu/protocol";
import type { DaemonClient } from "../lib/api.ts";
import { notificationsAvailable } from "../lib/useNotifications.ts";
import { currentPushSubscription, disablePush, enablePush, pushSupported } from "../lib/push.ts";
import { accountKind, agentLabel } from "../lib/useAgents.ts";
import { Icon, Menu, MenuItem, MenuSeparator } from "./Menu.tsx";

/**
 * The bottom of the sidebar: which agent account chats use, and a settings
 * menu for the things you set once (alerts, import, archived, disconnect).
 */
export function SidebarFooter({
  agents,
  onOpenAccounts,
  showArchived,
  onToggleArchived,
  onOpenImport,
  onDisconnect,
  daemonVersion,
  client,
  onError,
}: {
  agents: AgentStatus[] | null;
  onOpenAccounts: () => void;
  showArchived: boolean;
  onToggleArchived: () => void;
  onOpenImport: () => void;
  onDisconnect: () => void;
  daemonVersion: string | null;
  client: DaemonClient;
  onError: (message: string) => void;
}) {
  const [askable, setAskable] = useState(notificationsAvailable);
  // Web Push reaches this device even with the app closed.
  const [pushOn, setPushOn] = useState<boolean | null>(null);
  useEffect(() => {
    if (pushSupported()) void currentPushSubscription().then((s) => setPushOn(!!s));
  }, []);

  const installed = agents?.filter((a) => a.installed) ?? [];
  const first = installed[0];
  const allReady = installed.length > 0 && installed.every((a) => a.logged_in);

  return (
    <div className="flex items-center gap-1 border-t border-line px-2 py-2">
      <button
        type="button"
        onClick={onOpenAccounts}
        title={installed.length ? installed.map((a) => `${a.name}: ${accountKind(a)}`).join("\n") : "Agent accounts"}
        className="flex min-w-0 flex-1 items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-raised"
      >
        <span className={`size-2 shrink-0 rounded-full ${!agents ? "bg-faint" : allReady ? "bg-ok" : "bg-warn"}`} />
        <span className="min-w-0 flex-1 truncate">
          {first ? (
            <>
              {agentLabel(first.agent)}
              {(first.agent === "claude" || first.agent === "codex") && <span className="text-muted"> · {accountKind(first)}</span>}
            </>
          ) : agents ? (
            "Set up an agent"
          ) : (
            <span className="text-muted">Checking agents…</span>
          )}
        </span>
        {installed.length > 1 && <span className="shrink-0 text-[12px] text-faint tabular-nums">+{installed.length - 1}</span>}
      </button>
      <Menu
        label="Settings"
        side="above"
        width="w-64"
        trigger={
          <Icon className="size-4">
            <circle cx="8" cy="8" r="2" />
            <path d="M8 1.8v1.6M8 12.6v1.6M14.2 8h-1.6M3.4 8H1.8M12.4 3.6l-1.1 1.1M4.7 11.3l-1.1 1.1M12.4 12.4l-1.1-1.1M4.7 4.7L3.6 3.6" />
          </Icon>
        }
      >
        {(close) => (
          <>
            <MenuItem
              onSelect={() => {
                close();
                onOpenAccounts();
              }}
            >
              Agent accounts and organization…
            </MenuItem>
            {pushOn !== null && (
              <MenuItem
                onSelect={async () => {
                  close();
                  try {
                    if (pushOn) await disablePush(client);
                    else await enablePush(client);
                    setPushOn(!pushOn);
                    setAskable(notificationsAvailable());
                  } catch (err) {
                    onError(err instanceof Error ? err.message : String(err));
                  }
                }}
              >
                {pushOn ? "Turn off alerts on this device" : "Turn on alerts on this device"}
              </MenuItem>
            )}
            {askable && pushOn === null && (
              <MenuItem
                onSelect={async () => {
                  close();
                  await Notification.requestPermission();
                  setAskable(notificationsAvailable());
                }}
              >
                Turn on notifications
              </MenuItem>
            )}
            <MenuItem
              onSelect={() => {
                close();
                onOpenImport();
              }}
            >
              Import from Conductor or Pane…
            </MenuItem>
            <button
              type="button"
              role="menuitemcheckbox"
              aria-checked={showArchived}
              onClick={onToggleArchived}
              className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left hover:bg-raised focus-visible:bg-raised focus-visible:outline-none"
            >
              <span className="flex-1">Show archived workspaces</span>
              <span aria-hidden className={`flex h-4 w-7 items-center rounded-full p-0.5 transition-colors ${showArchived ? "bg-accent" : "bg-line"}`}>
                <span className={`size-3 rounded-full bg-bg shadow-sm transition-transform duration-150 ease-out ${showArchived ? "translate-x-3" : ""}`} />
              </span>
            </button>
            <MenuSeparator />
            <MenuItem
              onSelect={() => {
                close();
                onDisconnect();
              }}
            >
              Disconnect from this daemon
            </MenuItem>
            {daemonVersion && <p className="px-2.5 pt-1 pb-1.5 text-[11.5px] text-faint">bantud {daemonVersion}</p>}
          </>
        )}
      </Menu>
    </div>
  );
}
