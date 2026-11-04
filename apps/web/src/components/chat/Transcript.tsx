import { useEffect, useMemo, useRef, useState } from "react";
import type { ChatItem, Session } from "@bantu/protocol";
import type { ChatEntry } from "../../lib/store.ts";
import { formatCost, formatDuration, formatTokens } from "./format.ts";
import { Markdown } from "./Markdown.tsx";
import { ToolCard } from "./ToolCard.tsx";

type ToolResult = Extract<ChatItem, { kind: "tool_result" }>;

export function Transcript({
  entries,
  session,
  streaming,
  workspacePath,
  onRewind,
}: {
  entries: ChatEntry[];
  session: Session;
  /** Assistant text arriving live, before the complete item lands. */
  streaming?: { turn: number; text: string };
  workspacePath: string;
  /** Restore files to the checkpoint taken just before `turn`. */
  onRewind: (turn: number) => Promise<void>;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const running = session.status === "running";

  const { turns, results } = useMemo(() => {
    const results = new Map<string, ToolResult>();
    const turns = new Map<number, ChatEntry[]>();
    for (const e of entries) {
      if (e.item.kind === "tool_result") results.set(e.item.tool_use_id, e.item);
      const list = turns.get(e.turn) ?? [];
      list.push(e);
      turns.set(e.turn, list);
    }
    return { turns: [...turns.entries()].sort(([a], [b]) => a - b), results };
  }, [entries]);

  // Follow new output unless the user has scrolled up to read.
  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [entries, running, streaming?.text]);
  useEffect(() => {
    stick.current = true;
  }, [session.id]);

  return (
    <div
      ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
      }}
      className="min-h-0 flex-1 overflow-y-auto"
    >
      <div className="mx-auto flex min-w-0 max-w-3xl flex-col gap-6 px-5 py-5">
        {turns.length === 0 && !running && (
          <p className="py-10 text-center text-muted">
            Describe a task. The agent works on this workspace's branch, so other workspaces aren't affected.
          </p>
        )}
        {turns.map(([turn, list]) => (
          <section key={turn} className="group/turn flex flex-col gap-2.5" aria-label={`Turn ${turn}`}>
            {!running && list.some((e) => e.item.kind === "user") && (
              <RewindButton turn={turn} onRewind={onRewind} />
            )}
            {list.map((e) => (
              <Item key={e.seq} entry={e} results={results} workspacePath={workspacePath} />
            ))}
          </section>
        ))}
        {running && streaming && streaming.text && (
          <div aria-live="polite">
            {/* The caret rides at the end of the text so it sits inline even inside lists. */}
            <Markdown text={`${streaming.text}\u2009▍`} />
          </div>
        )}
        {running && <Working queued={session.queued} />}
      </div>
    </div>
  );
}

function Item({
  entry,
  results,
  workspacePath,
}: {
  entry: ChatEntry;
  results: Map<string, ToolResult>;
  workspacePath: string;
}) {
  const item = entry.item;
  switch (item.kind) {
    case "user":
      return (
        <div className="ml-auto max-w-[85%] whitespace-pre-wrap [overflow-wrap:anywhere] rounded-lg bg-raised px-3 py-2 text-[14px] leading-6">
          {item.text}
        </div>
      );
    case "assistant":
      return <Markdown text={item.text} />;
    case "thinking":
      return <Thinking text={item.text} />;
    case "tool_use":
      return <ToolCard use={item} result={results.get(item.id)} workspacePath={workspacePath} />;
    case "tool_result":
      // Rendered inside its tool card.
      return null;
    case "turn_end":
      return <TurnFooter item={item} />;
    case "check":
      return <CheckResult item={item} />;
    case "rewound":
      return (
        <p className="border-l-2 border-warn/60 pl-3 text-[12.5px] text-muted">
          Files restored to how they were before turn {item.to_turn}. The agent is told on its next turn.
        </p>
      );
    case "error":
      return (
        <div role="alert" className="rounded-md border border-bad/40 bg-bad/10 px-3 py-2 text-bad">
          {item.message}
        </div>
      );
  }
}

/** Shown on hover over a turn: undo the file changes from this turn onward. */
function RewindButton({ turn, onRewind }: { turn: number; onRewind: (turn: number) => Promise<void> }) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  if (confirming) {
    return (
      <div className="ml-auto flex items-center gap-2 text-[12px]">
        <span className="text-muted">Undo all file changes from this turn on?</span>
        <button
          type="button"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await onRewind(turn);
            } finally {
              setBusy(false);
              setConfirming(false);
            }
          }}
          className="rounded border border-warn/50 px-2 py-0.5 text-warn hover:bg-warn/10 disabled:opacity-50"
        >
          {busy ? "Restoring…" : "Restore files"}
        </button>
        <button type="button" onClick={() => setConfirming(false)} className="px-1 text-muted hover:text-fg">
          Cancel
        </button>
      </div>
    );
  }
  return (
    <button
      type="button"
      onClick={() => setConfirming(true)}
      className="ml-auto text-[11.5px] text-faint opacity-0 transition-opacity hover:text-fg focus-visible:opacity-100 group-hover/turn:opacity-100"
    >
      ↺ Restore files to before this turn
    </button>
  );
}

function CheckResult({ item }: { item: Extract<ChatItem, { kind: "check" }> }) {
  const [open, setOpen] = useState(!item.passed);
  return (
    <div className={`rounded-md border px-3 py-2 text-[12.5px] ${item.passed ? "border-ok/30 bg-ok/5" : "border-bad/40 bg-bad/5"}`}>
      <button type="button" onClick={() => setOpen((v) => !v)} aria-expanded={open} className="flex w-full items-center gap-2 text-left">
        <span className={item.passed ? "text-ok" : "text-bad"}>{item.passed ? "✓ Checks passed" : "✗ Checks failed"}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-muted" title={item.command}>
          {item.command}
        </span>
        <span className="shrink-0 tabular-nums text-faint">{formatDuration(item.duration_ms)}</span>
      </button>
      {open && item.output && (
        <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap rounded bg-bg p-2 font-mono text-[12px] text-muted">{item.output}</pre>
      )}
      {!item.passed && <p className="mt-1.5 text-[11.5px] text-faint">Sent back to the agent (at most twice in a row).</p>}
    </div>
  );
}

function Thinking({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="text-muted">
      <button type="button" onClick={() => setOpen((v) => !v)} aria-expanded={open} className="flex items-center gap-1.5 hover:text-fg">
        <span aria-hidden className={`text-faint transition-transform ${open ? "rotate-90" : ""}`}>
          ›
        </span>
        <span className="italic">Thinking…</span>
      </button>
      {open && <div className="mt-1.5 whitespace-pre-wrap border-l-2 border-line pl-3 text-[12.5px] leading-5">{text}</div>}
    </div>
  );
}

function TurnFooter({ item }: { item: Extract<ChatItem, { kind: "turn_end" }> }) {
  const parts = [
    item.duration_ms != null && formatDuration(item.duration_ms),
    item.cost_usd != null && formatCost(item.cost_usd),
    item.input_tokens != null &&
      item.output_tokens != null &&
      `${formatTokens(item.input_tokens)} in / ${formatTokens(item.output_tokens)} out`,
  ].filter(Boolean);
  return (
    <p className={`text-[11.5px] tabular-nums ${item.is_error ? "text-bad" : "text-faint"}`}>
      {item.is_error ? "Turn ended with an error" : "Done"}
      {parts.length > 0 && ` · ${parts.join(" · ")}`}
    </p>
  );
}

function Working({ queued }: { queued: number }) {
  return (
    <div className="flex items-center gap-2 text-muted" role="status">
      <span className="flex gap-1" aria-hidden>
        {[0, 1, 2].map((i) => (
          <span key={i} className="size-1.5 animate-bounce rounded-full bg-accent" style={{ animationDelay: `${i * 120}ms` }} />
        ))}
      </span>
      Working…
      {queued > 0 && <span className="text-faint">· {queued} queued</span>}
    </div>
  );
}
