import { useMemo, useState } from "react";
import type { DiffComment, WorkspaceDiff } from "@bantu/protocol";
import { FileDiffView } from "./FileDiffView.tsx";

export function ChangesPanel({
  diff,
  diffError,
  comments,
  canSend,
  onRefresh,
  onAddComment,
  onDeleteComment,
  onSendComments,
}: {
  diff: WorkspaceDiff | null;
  diffError: string | null;
  /** This workspace's comments. */
  comments: DiffComment[];
  /** False when there's no chat to send comments to. */
  canSend: boolean;
  onRefresh: () => Promise<void>;
  onAddComment: (path: string, line: number, body: string) => Promise<boolean>;
  onDeleteComment: (id: string) => Promise<void>;
  onSendComments: () => Promise<void>;
}) {
  const [sending, setSending] = useState(false);
  const drafts = comments.filter((c) => !c.sent_at);
  const byPath = useMemo(() => {
    const map = new Map<string, DiffComment[]>();
    for (const c of comments) map.set(c.path, [...(map.get(c.path) ?? []), c]);
    return map;
  }, [comments]);

  if (diffError) {
    return (
      <div className="m-5 rounded-md border border-bad/40 bg-bad/10 px-3 py-2 text-bad">
        Couldn't load changes: {diffError}{" "}
        <button type="button" onClick={() => void onRefresh()} className="underline">
          Retry
        </button>
      </div>
    );
  }
  if (!diff) return <p className="p-5 text-muted">Loading changes…</p>;

  const totals = diff.files.reduce((t, f) => ({ add: t.add + f.additions, del: t.del + f.deletions }), { add: 0, del: 0 });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Changed files" className="w-60 shrink-0 overflow-y-auto border-r border-line py-2">
          <p className="px-3 pb-1.5 text-[11.5px] text-muted tabular-nums">
            {diff.files.length} {diff.files.length === 1 ? "file" : "files"} · <span className="text-ok">+{totals.add}</span>{" "}
            <span className="text-bad">−{totals.del}</span>
          </p>
          {diff.files.map((f) => (
            <a
              key={f.path}
              href={`#file-${f.path}`}
              onClick={(e) => {
                e.preventDefault();
                document.getElementById(`file-${f.path}`)?.scrollIntoView({ block: "start" });
              }}
              className="flex items-center gap-2 px-3 py-1 hover:bg-raised/60"
              title={f.path}
            >
              <span className="min-w-0 flex-1 truncate font-mono text-[12px]">
                {f.path.split("/").pop()}
                <span className="text-faint"> {f.path.includes("/") ? f.path.slice(0, f.path.lastIndexOf("/")) : ""}</span>
              </span>
              {(byPath.get(f.path)?.length ?? 0) > 0 && (
                <span className="rounded bg-raised px-1 text-[10.5px] text-muted" title="Comments">
                  {byPath.get(f.path)?.length}
                </span>
              )}
              <span className="shrink-0 font-mono text-[11px] tabular-nums text-faint">
                <span className="text-ok">+{f.additions}</span> <span className="text-bad">−{f.deletions}</span>
              </span>
            </a>
          ))}
        </nav>
        <div className="min-w-0 flex-1 overflow-y-auto p-4">
          {diff.files.length === 0 ? (
            <p className="py-10 text-center text-muted">
              No changes yet compared with <span className="font-mono">{diff.base_ref}</span>.
            </p>
          ) : (
            <div className="space-y-3">
              {diff.files.map((f) => (
                <FileDiffView
                  key={f.path}
                  file={f}
                  comments={byPath.get(f.path) ?? []}
                  onAddComment={(line, body) => onAddComment(f.path, line, body)}
                  onDeleteComment={onDeleteComment}
                />
              ))}
            </div>
          )}
        </div>
      </div>
      {drafts.length > 0 && (
        <div className="flex items-center gap-3 border-t border-line bg-panel px-5 py-2">
          <span className="text-muted">
            {drafts.length} draft {drafts.length === 1 ? "comment" : "comments"}
          </span>
          <button
            type="button"
            disabled={!canSend || sending}
            onClick={async () => {
              setSending(true);
              try {
                await onSendComments();
              } finally {
                setSending(false);
              }
            }}
            className="pressable ml-auto rounded-md bg-accent px-3 py-1 font-medium text-bg disabled:opacity-40"
          >
            Send {drafts.length} {drafts.length === 1 ? "comment" : "comments"} to agent
          </button>
        </div>
      )}
    </div>
  );
}
