import { useState } from "react";
import type { ChatItem } from "@bantu/protocol";
import { toolSummary } from "./format.ts";

type ToolUse = Extract<ChatItem, { kind: "tool_use" }>;
type ToolResult = Extract<ChatItem, { kind: "tool_result" }>;

/** A tool call and (once it arrives) its result. Collapsed to one line by default. */
export function ToolCard({ use, result, workspacePath }: { use: ToolUse; result?: ToolResult; workspacePath: string }) {
  const [open, setOpen] = useState(false);
  const summary = toolSummary(use.name, use.input, workspacePath);
  const failed = result?.is_error ?? false;
  const pending = !result;

  return (
    <div className={`rounded-md border ${failed ? "border-bad/40" : "border-line"} bg-panel`}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left hover:bg-raised/60"
      >
        <span
          aria-hidden
          className={`inline-block size-1.5 shrink-0 rounded-full ${
            pending ? "animate-pulse bg-warn" : failed ? "bg-bad" : "bg-ok"
          }`}
        />
        <span className="shrink-0 font-medium">{use.name}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-muted" title={summary}>
          {summary}
        </span>
        <span aria-hidden className={`shrink-0 text-faint transition-transform ${open ? "rotate-90" : ""}`}>
          ›
        </span>
      </button>
      {open && (
        <div className="space-y-2 border-t border-line px-2.5 py-2">
          <Block label="Input" text={JSON.stringify(use.input, null, 2)} />
          {result ? (
            <Block label={failed ? "Error" : "Result"} text={result.content || "(no output)"} tone={failed ? "bad" : undefined} />
          ) : (
            <p className="text-faint">Waiting for result…</p>
          )}
        </div>
      )}
    </div>
  );
}

function Block({ label, text, tone }: { label: string; text: string; tone?: "bad" }) {
  return (
    <div>
      <p className="mb-1 text-[11px] font-medium uppercase tracking-wide text-faint">{label}</p>
      <pre
        className={`max-h-72 overflow-auto whitespace-pre-wrap break-words rounded border border-line bg-bg p-2 font-mono text-[12px] leading-5 ${
          tone === "bad" ? "text-bad" : "text-fg"
        }`}
      >
        {text}
      </pre>
    </div>
  );
}
