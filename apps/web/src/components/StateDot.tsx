import type { WorkspaceState } from "@bantu/protocol";

const tone: Record<WorkspaceState, string> = {
  creating: "bg-warn animate-pulse",
  ready: "bg-ok",
  failed: "bg-bad",
  archiving: "bg-warn animate-pulse",
  archived: "bg-faint",
};

/** Pass `decorative` when the state is also shown as text next to the dot. */
export function StateDot({ state, decorative = false }: { state: WorkspaceState; decorative?: boolean }) {
  return (
    <span
      {...(decorative ? { "aria-hidden": true } : { role: "img", "aria-label": state, title: state })}
      className={`inline-block size-2 shrink-0 rounded-full ${tone[state]}`}
    />
  );
}
