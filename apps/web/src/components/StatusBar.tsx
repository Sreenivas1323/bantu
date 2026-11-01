import type { ConnStatus } from "../lib/useDaemon.ts";

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
}: {
  status: ConnStatus;
  lastSeq: number;
  daemonVersion: string | null;
  error: string | null;
  onDismissError: () => void;
  onDisconnect: () => void;
}) {
  return (
    <footer className="col-span-2 flex items-center gap-4 border-t border-line bg-panel px-3 py-1.5 text-xs text-muted">
      <span className="flex items-center gap-1.5">
        <span className={`size-2 rounded-full ${tone[status]}`} />
        {label[status]}
      </span>
      {daemonVersion && <span>bantud {daemonVersion}</span>}
      <span className="tabular-nums">seq {lastSeq}</span>
      {error && (
        <button type="button" onClick={onDismissError} className="min-w-0 truncate text-bad" title="Dismiss">
          {error}
        </button>
      )}
      <span className="flex-1" />
      <button type="button" onClick={onDisconnect} className="rounded px-1.5 py-0.5 hover:bg-raised hover:text-fg">
        Disconnect
      </button>
    </footer>
  );
}
