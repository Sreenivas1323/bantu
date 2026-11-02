import { useState, type FormEvent } from "react";
import type { DiffComment } from "@bantu/protocol";

export function CommentList({ comments, onDelete }: { comments: DiffComment[]; onDelete: (id: string) => Promise<void> }) {
  return (
    <div className="space-y-1.5">
      {comments.map((c) => (
        <div key={c.id} className="flex items-start gap-2 rounded-md border border-line bg-panel px-2.5 py-1.5 font-sans text-[13px]">
          <p className="min-w-0 flex-1 whitespace-pre-wrap break-words">{c.body}</p>
          {c.sent_at ? (
            <span className="shrink-0 text-[11px] text-faint" title={`Sent ${new Date(c.sent_at).toLocaleString()}`}>
              Sent
            </span>
          ) : (
            <>
              <span className="shrink-0 rounded bg-warn/15 px-1.5 text-[11px] text-warn">Draft</span>
              <button
                type="button"
                onClick={() => void onDelete(c.id)}
                className="shrink-0 text-[11px] text-faint hover:text-bad"
                aria-label="Delete comment"
              >
                Delete
              </button>
            </>
          )}
        </div>
      ))}
    </div>
  );
}

export function CommentForm({
  line,
  onSubmit,
  onCancel,
}: {
  line: number;
  onSubmit: (body: string) => Promise<boolean>;
  onCancel: () => void;
}) {
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!body.trim() || busy) return;
    setBusy(true);
    try {
      if (await onSubmit(body.trim())) onCancel();
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit} className="space-y-1.5 rounded-md border border-accent/50 bg-panel p-2 font-sans">
      <textarea
        autoFocus
        value={body}
        onChange={(e) => setBody(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onCancel();
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void submit();
        }}
        rows={2}
        placeholder={`Comment on line ${line} for the agent…`}
        aria-label={`Comment on line ${line}`}
        className="block w-full resize-y rounded border border-line bg-bg px-2 py-1.5 text-[16px] sm:text-[13px] placeholder:text-faint focus:outline-none"
      />
      <div className="flex items-center gap-1.5">
        <button type="submit" disabled={!body.trim() || busy} className="rounded-md bg-accent px-2.5 py-1 text-[12px] font-medium text-bg disabled:opacity-40">
          Add draft
        </button>
        <button type="button" onClick={onCancel} className="rounded-md px-2.5 py-1 text-[12px] text-muted hover:bg-raised">
          Cancel
        </button>
        <span className="ml-auto text-[11px] text-faint">⌘↩ to add</span>
      </div>
    </form>
  );
}
