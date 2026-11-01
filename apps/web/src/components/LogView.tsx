import { useEffect, useRef } from "react";
import type { LogLine } from "../lib/store.ts";

const streamTone = { stdout: "text-fg", stderr: "text-bad", system: "text-accent" } as const;

export function LogView({ lines }: { lines: LogLine[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  return (
    <div
      ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
      className="min-h-0 flex-1 overflow-y-auto rounded-md border border-line bg-bg p-2.5 font-mono text-[12px] leading-5"
    >
      {lines.length === 0 ? (
        <p className="text-faint">No output yet. Setup and archive script output appears here.</p>
      ) : (
        lines.map((l) => (
          <div key={l.seq} className={`whitespace-pre-wrap break-words ${streamTone[l.stream]}`}>
            {l.line}
          </div>
        ))
      )}
    </div>
  );
}
