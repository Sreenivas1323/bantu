import { Fragment, useMemo, useState } from "react";
import type { DiffComment, FileDiff, FileStatus } from "@bantu/protocol";
import { CommentForm, CommentList } from "./CommentThread.tsx";
import { parsePatch } from "./patch.ts";

const statusTone: Record<FileStatus, string> = {
  added: "text-ok",
  modified: "text-accent",
  deleted: "text-bad",
  renamed: "text-warn",
};

const lineTone = { context: "", add: "bg-ok/10", del: "bg-bad/10" } as const;
const signs = { context: " ", add: "+", del: "-" } as const;

export function FileDiffView({
  file,
  comments,
  onAddComment,
  onDeleteComment,
}: {
  file: FileDiff;
  comments: DiffComment[];
  onAddComment: (line: number, body: string) => Promise<boolean>;
  onDeleteComment: (id: string) => Promise<void>;
}) {
  const [open, setOpen] = useState(true);
  const [draftLine, setDraftLine] = useState<number | null>(null);
  const lines = useMemo(() => parsePatch(file.patch), [file.patch]);

  const byLine = useMemo(() => {
    const map = new Map<number, DiffComment[]>();
    for (const c of comments) map.set(c.line, [...(map.get(c.line) ?? []), c]);
    return map;
  }, [comments]);
  // Comments on lines outside the visible hunks still need a home.
  const shown = new Set(lines.flatMap((l) => (l.kind !== "hunk" && l.newLine != null ? [l.newLine] : [])));
  const orphans = comments.filter((c) => !shown.has(c.line));

  return (
    <section id={`file-${file.path}`} className="overflow-hidden rounded-md border border-line">
      <header className="sticky top-0 z-10 flex items-center gap-2 border-b border-line bg-panel px-2.5 py-1.5">
        <button type="button" onClick={() => setOpen((v) => !v)} aria-expanded={open} className="text-faint hover:text-fg">
          <span aria-hidden className={`inline-block transition-transform ${open ? "rotate-90" : ""}`}>
            ›
          </span>
        </button>
        <span className={`w-16 shrink-0 text-[11px] uppercase tracking-wide ${statusTone[file.status]}`}>{file.status}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[12px]" title={file.path}>
          {file.old_path && file.old_path !== file.path ? `${file.old_path} → ${file.path}` : file.path}
        </span>
        <span className="shrink-0 font-mono text-[11.5px] tabular-nums">
          <span className="text-ok">+{file.additions}</span> <span className="text-bad">−{file.deletions}</span>
        </span>
      </header>
      {open && (
        <div className="overflow-x-auto bg-bg font-mono text-[12px] leading-5">
          {file.binary ? (
            <p className="px-3 py-2 font-sans text-faint">Binary file</p>
          ) : lines.length === 0 ? (
            <p className="px-3 py-2 font-sans text-faint">No textual changes</p>
          ) : (
            <table className="w-full border-collapse">
              <tbody>
                {lines.map((l, i) => {
                  if (l.kind === "hunk") {
                    return (
                      <tr key={i} className="bg-raised/60 text-faint">
                        <td colSpan={4} className="px-3 py-0.5">
                          {l.text}
                        </td>
                      </tr>
                    );
                  }
                  const n = l.newLine;
                  const here = n != null ? byLine.get(n) : undefined;
                  return (
                    <Fragment key={i}>
                      <tr className={lineTone[l.kind]}>
                        <td className="w-12 select-none px-2 text-right text-faint tabular-nums">{l.oldLine ?? ""}</td>
                        <td className="w-12 select-none px-2 text-right tabular-nums">
                          {n != null ? (
                            <button
                              type="button"
                              onClick={() => setDraftLine(n)}
                              className="text-faint hover:text-accent"
                              title={`Comment on line ${n}`}
                            >
                              {n}
                            </button>
                          ) : null}
                        </td>
                        <td className="w-4 select-none text-faint">{signs[l.kind]}</td>
                        <td className="whitespace-pre pr-3">{l.text}</td>
                      </tr>
                      {(here || draftLine === n) && n != null && (
                        <tr>
                          <td colSpan={4} className="space-y-1.5 border-y border-line bg-panel/50 px-3 py-2">
                            {here && <CommentList comments={here} onDelete={onDeleteComment} />}
                            {draftLine === n && (
                              <CommentForm line={n} onSubmit={(body) => onAddComment(n, body)} onCancel={() => setDraftLine(null)} />
                            )}
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          )}
          {file.truncated && <p className="border-t border-line px-3 py-1.5 font-sans text-faint">Diff truncated; the file changed a lot.</p>}
          {orphans.length > 0 && (
            <div className="border-t border-line px-3 py-2">
              <CommentList comments={orphans} onDelete={onDeleteComment} />
            </div>
          )}
        </div>
      )}
    </section>
  );
}
