import { useEffect, useState } from "react";
import type { ForwardedPort, Workspace } from "@bantu/protocol";
import type { DaemonClient } from "../lib/api.ts";

const POLL_MS = 4000;

/** Dev servers listening on this workspace's reserved ports, with links to open them. */
export function PortsPanel({ workspace, client }: { workspace: Workspace; client: DaemonClient }) {
  const [ports, setPorts] = useState<ForwardedPort[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const next = await client.ports(workspace.id);
        if (!cancelled) {
          setPorts(next);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    };
    void load();
    const timer = setInterval(load, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [client, workspace.id]);

  const range = `${workspace.port_base}–${workspace.port_base + 9}`;
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex max-w-3xl flex-col gap-3 px-5 py-4">
        <p className="text-muted">
          Ports <span className="font-mono tabular-nums">{range}</span> are reserved for this workspace. Dev servers that use{" "}
          <code className="font-mono">$BANTU_PORT</code> show up here, and you can open them from any device connected to this daemon.
        </p>
        {error && <p className="rounded-md border border-bad/40 bg-bad/10 px-3 py-2 text-bad">{error}</p>}
        {ports?.length === 0 && <p className="rounded-md border border-line bg-panel px-3 py-6 text-center text-muted">Nothing is listening on these ports yet.</p>}
        {ports && ports.length > 0 && (
          <ul className="overflow-hidden rounded-md border border-line bg-panel">
            {ports.map((p) => (
              <li key={p.port} className="flex items-center gap-3 border-b border-line px-3 py-2 last:border-b-0">
                <span aria-hidden className="size-2 shrink-0 rounded-full bg-ok" />
                <span className="flex-1 font-mono tabular-nums">{p.port}</span>
                {p.url ? (
                  <a href={p.url} target="_blank" rel="noreferrer" className="rounded-md bg-accent px-2.5 py-1 text-[12px] font-medium text-bg">
                    Open
                  </a>
                ) : (
                  <span className="text-[12px] text-faint" title="See Port forwarding in the remote access guide">
                    Set BANTU_PREVIEW_URL to open this remotely
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
