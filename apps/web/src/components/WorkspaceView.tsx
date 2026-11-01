import { useEffect, useState, type ReactNode } from "react";
import type { Repo, Workspace } from "@bantu/protocol";
import type { LogLine } from "../lib/store.ts";
import { CopyButton } from "./CopyButton.tsx";
import { LogView } from "./LogView.tsx";
import { StateDot } from "./StateDot.tsx";

export function WorkspaceView({
  workspace,
  repo,
  logs,
  onArchive,
}: {
  workspace: Workspace;
  repo: Repo | undefined;
  logs: LogLine[];
  onArchive: (id: string) => Promise<void>;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => setConfirming(false), [workspace.id]);

  const canArchive = workspace.state === "ready" || workspace.state === "failed";

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 p-5">
      <header className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <p className="text-xs text-muted">{repo?.name ?? "Unknown repository"}</p>
          <h2 className="mt-0.5 flex items-center gap-2 text-lg font-semibold tracking-tight">
            {workspace.name}
            <span className="flex items-center gap-1.5 rounded-full border border-line px-2 py-0.5 text-xs font-normal text-muted">
              <StateDot state={workspace.state} decorative />
              {workspace.state}
            </span>
          </h2>
        </div>
        {canArchive &&
          (confirming ? (
            <div className="flex items-center gap-1.5">
              <span className="text-muted">Remove the worktree? The branch is kept.</span>
              <button
                type="button"
                disabled={busy}
                onClick={async () => {
                  setBusy(true);
                  try {
                    await onArchive(workspace.id);
                  } finally {
                    setBusy(false);
                    setConfirming(false);
                  }
                }}
                className="rounded-md bg-bad px-2.5 py-1 font-medium text-bg disabled:opacity-40"
              >
                Archive
              </button>
              <button type="button" onClick={() => setConfirming(false)} className="rounded-md px-2.5 py-1 text-muted hover:bg-raised">
                Cancel
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setConfirming(true)}
              className="rounded-md border border-line px-2.5 py-1 text-muted hover:bg-raised hover:text-fg"
            >
              Archive…
            </button>
          ))}
      </header>

      {workspace.error && (
        <div className="rounded-md border border-bad/40 bg-bad/10 px-3 py-2 text-bad">{workspace.error}</div>
      )}

      <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 rounded-md border border-line bg-panel p-3">
        <Field label="Branch">
          <span className="font-mono">{workspace.branch}</span>
          <CopyButton text={workspace.branch} />
        </Field>
        <Field label="Base">
          <span className="font-mono">{workspace.base_ref}</span>
        </Field>
        <Field label="Path">
          <span className="truncate font-mono" title={workspace.path}>
            {workspace.path}
          </span>
          <CopyButton text={workspace.path} />
        </Field>
        <Field label="Ports">
          <span className="font-mono tabular-nums">
            {workspace.port_base}–{workspace.port_base + 9}
          </span>
          <span className="text-faint">BANTU_PORT</span>
        </Field>
        {workspace.archive_commit && (
          <Field label="Saved changes">
            <span className="font-mono">{workspace.archive_commit.slice(0, 12)}</span>
            <CopyButton text={workspace.archive_commit} />
          </Field>
        )}
        <Field label="Created">
          <span className="tabular-nums">{new Date(workspace.created_at).toLocaleString()}</span>
        </Field>
      </dl>

      <section className="flex min-h-0 flex-1 flex-col gap-1.5">
        <h3 className="text-xs font-medium uppercase tracking-wide text-muted">Log</h3>
        <LogView lines={logs} />
      </section>
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-muted">{label}</dt>
      <dd className="flex min-w-0 items-center gap-2">{children}</dd>
    </>
  );
}
