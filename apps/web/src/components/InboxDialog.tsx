import { useEffect, useRef, useState } from "react";
import type { Command, CommandResponse, InboxItem, PermissionRequest, Workspace } from "@bantu/protocol";

type Run = (command: Command) => Promise<CommandResponse | null>;

/** Everything agents are waiting on or want you to see, across workspaces. */
export function InboxDialog({
  inbox,
  permissions,
  workspaces,
  run,
  onSelectWorkspace,
  onClose,
}: {
  inbox: InboxItem[];
  permissions: PermissionRequest[];
  workspaces: Record<string, Workspace>;
  run: Run;
  onSelectWorkspace: (id: string) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
  }, []);

  const byDate = <T extends { created_at: string }>(list: T[]) => [...list].sort((a, b) => a.created_at.localeCompare(b.created_at));
  const questions = byDate(inbox.filter((i) => i.kind === "question"));
  const notices = byDate(inbox.filter((i) => i.kind === "notice")).reverse();
  const empty = inbox.length === 0 && permissions.length === 0;

  const source = (workspaceId: string) => (
    <button type="button" onClick={() => onSelectWorkspace(workspaceId)} className="text-[12px] text-muted underline-offset-2 hover:text-fg hover:underline">
      {workspaces[workspaceId]?.name ?? "A workspace"}
    </button>
  );

  return (
    <dialog
      ref={dialog}
      onClose={onClose}
      className="m-auto max-h-[80dvh] w-[min(560px,calc(100vw-2rem))] rounded-lg border border-line bg-panel p-0 text-fg backdrop:bg-black/60"
    >
      <div className="flex items-center justify-between border-b border-line px-4 py-3">
        <h2 className="font-medium">Inbox</h2>
        <button type="button" onClick={() => dialog.current?.close()} className="px-1 text-muted hover:text-fg" aria-label="Close">
          ✕
        </button>
      </div>
      <div className="flex flex-col gap-3 p-4">
        {empty && <p className="py-6 text-center text-muted">Nothing needs you right now.</p>}
        {permissions.map((p) => (
          <div key={p.id} className="flex items-center gap-3 rounded-md border border-warn/40 bg-warn/5 px-3 py-2">
            <div className="min-w-0 flex-1">
              {source(p.workspace_id)}
              <p className="truncate">Wants to use {p.tool_name}</p>
            </div>
            <button type="button" onClick={() => onSelectWorkspace(p.workspace_id)} className="shrink-0 rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:bg-raised hover:text-fg">
              Review
            </button>
          </div>
        ))}
        {questions.map((q) => (
          <div key={q.id} className="flex flex-col gap-1">
            {source(q.workspace_id)}
            <QuestionCard item={q} run={run} />
          </div>
        ))}
        {notices.map((n) => (
          <div key={n.id} className="rounded-md border border-line px-3 py-2">
            <div className="flex items-baseline gap-2">
              {source(n.workspace_id)}
              <span className="flex-1" />
              <button type="button" onClick={() => void run({ type: "inbox.dismiss", item_id: n.id })} className="text-[12px] text-faint hover:text-fg">
                Dismiss
              </button>
            </div>
            <p className="font-medium">{n.title}</p>
            {n.body && <p className="mt-0.5 whitespace-pre-wrap [overflow-wrap:anywhere] text-[12.5px] text-muted">{n.body}</p>}
          </div>
        ))}
      </div>
    </dialog>
  );
}

/** A question an agent is waiting on: suggested answers, or type your own. */
export function QuestionCard({ item, run }: { item: InboxItem; run: Run }) {
  const [text, setText] = useState("");
  const answer = (value: string) => run({ type: "inbox.respond", item_id: item.id, answer: value });
  return (
    <form
      className="flex flex-col gap-2 rounded-md border border-accent/40 bg-accent/5 px-3 py-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (text.trim()) void answer(text);
      }}
    >
      <p className="whitespace-pre-wrap [overflow-wrap:anywhere] font-medium">{item.title}</p>
      {item.options.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {item.options.map((o) => (
            <button key={o} type="button" onClick={() => void answer(o)} className="rounded-md border border-line bg-bg px-2.5 py-1 text-[12px] hover:bg-raised">
              {o}
            </button>
          ))}
        </div>
      )}
      <div className="flex gap-1.5">
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Type an answer…"
          aria-label="Answer"
          className="min-w-0 flex-1 rounded-md border border-line bg-bg px-2 py-1 text-[16px] sm:text-[13px]"
        />
        <button type="submit" disabled={!text.trim()} className="rounded-md bg-accent px-2.5 py-1 text-[12px] font-medium text-bg disabled:opacity-40">
          Answer
        </button>
        <button type="button" onClick={() => void run({ type: "inbox.dismiss", item_id: item.id })} className="rounded-md px-2 py-1 text-[12px] text-muted hover:bg-raised hover:text-fg">
          Dismiss
        </button>
      </div>
    </form>
  );
}
