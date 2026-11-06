import { useEffect, useRef, useState } from "react";
import type { Command, CommandResponse, ImportPreview, ImportSource, ImportStatus } from "@bantu/protocol";
import { afterExit } from "../lib/motion.ts";

const LABEL: Record<ImportSource, string> = { conductor: "Conductor", pane: "Pane" };

/** Brings active workspaces and chats over from another tool, without changing it. */
export function ImportDialog({
  status,
  run,
  onClose,
}: {
  /** The latest import's progress, per source. */
  status: Partial<Record<ImportSource, ImportStatus>>;
  run: (command: Command) => Promise<CommandResponse | null>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [previews, setPreviews] = useState<ImportPreview[] | null>(null);

  useEffect(() => {
    dialog.current?.showModal();
    let cancelled = false;
    void (async () => {
      const found: ImportPreview[] = [];
      for (const source of ["conductor", "pane"] as const) {
        const res = await run({ type: "import.scan", source, path: null });
        if (res?.result) found.push(res.result as ImportPreview);
      }
      if (!cancelled) setPreviews(found);
    })();
    return () => {
      cancelled = true;
    };
    // Scans once when the dialog opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <dialog
      ref={dialog}
      onClose={() => afterExit(onClose)}
      className="m-auto max-h-[85dvh] w-[min(560px,calc(100vw-2rem))] rounded-lg border border-line bg-panel p-0 text-fg backdrop:bg-black/60"
    >
      <div className="flex items-center justify-between border-b border-line px-4 py-3">
        <h2 className="font-medium">Import from another tool</h2>
        <button type="button" onClick={() => dialog.current?.close()} className="px-1 text-muted hover:text-fg" aria-label="Close">
          ✕
        </button>
      </div>
      <div className="flex flex-col gap-3 p-4">
        <p className="text-[12.5px] text-muted">
          Bantu reads the other tool's data without changing it, and works in its existing worktrees. Archiving an imported workspace here only forgets it. Each chat
          keeps its last 1,000 messages, and Claude and Codex chats can carry on where they left off. Importing again skips what's already here.
        </p>
        {!previews && <p className="text-muted">Looking…</p>}
        {previews?.map((p) => {
          const current = status[p.source];
          return (
            <section key={p.source} className="rounded-md border border-line p-3">
              <div className="flex items-center gap-2">
                <span className="font-medium">{LABEL[p.source]}</span>
                <span className="flex-1" />
                {p.found && (
                  <button
                    type="button"
                    disabled={current?.running || p.workspaces === 0}
                    onClick={() => void run({ type: "import.run", source: p.source, path: null })}
                    className="pressable rounded-md bg-accent px-2.5 py-1 text-[12px] font-medium text-bg disabled:opacity-40"
                  >
                    {current?.running ? "Importing…" : "Import"}
                  </button>
                )}
              </div>
              <p className="mt-1 text-[12.5px] text-muted">
                {p.found
                  ? `${p.workspaces} active workspace${p.workspaces === 1 ? "" : "s"} with ${p.chats} chat${p.chats === 1 ? "" : "s"}${
                      p.missing_worktrees > 0 ? `, and ${p.missing_worktrees} whose folders are gone` : ""
                    }.`
                  : "Not found on this machine."}
              </p>
              {current && (
                <p className={`mt-1 text-[12.5px] ${current.error ? "text-bad" : "text-ok"}`}>
                  {current.error ??
                    `${current.running ? "So far: " : "Done: "}${current.workspaces} workspaces, ${current.chats} chats, ${current.items} messages${
                      current.skipped > 0 ? `; ${current.skipped} skipped` : ""
                    }.`}
                </p>
              )}
            </section>
          );
        })}
      </div>
    </dialog>
  );
}
