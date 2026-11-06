import type { Workspace } from "@bantu/protocol";

/** "Will conflict with lisbon in README.md" — shown before anyone opens a PR. */
export function ConflictBanner({
  conflicts,
  workspaces,
  onSelect,
  onResolve,
}: {
  conflicts: { other: string | null; base: string | null; files: string[] }[];
  workspaces: Record<string, Workspace>;
  onSelect: (workspaceId: string) => void;
  /** Asks the agent to sort it out; missing when no chat can take it. */
  onResolve?: () => Promise<void>;
}) {
  if (conflicts.length === 0) return null;
  return (
    <div role="status" className="flex flex-col gap-0.5 border-b border-warn/30 bg-warn/5 px-5 py-1.5 text-[12.5px]">
      {conflicts.map((c, i) => (
        <p key={i} className="truncate text-warn">
          <span aria-hidden>⚠ </span>
          Will conflict with{" "}
          {c.other ? (
            <button type="button" onClick={() => onSelect(c.other!)} className="font-medium underline-offset-2 hover:underline">
              {workspaces[c.other]?.name ?? "another workspace"}
            </button>
          ) : (
            <span className="font-mono">{c.base}</span>
          )}{" "}
          in <span className="font-mono text-muted">{c.files.slice(0, 4).join(", ")}</span>
          {c.files.length > 4 && <span className="text-muted"> and {c.files.length - 4} more</span>}
        </p>
      ))}
      {onResolve && (
        <button type="button" onClick={() => void onResolve()} className="self-start font-medium text-warn underline underline-offset-2">
          Ask the agent to resolve this
        </button>
      )}
    </div>
  );
}
