import { useState } from "react";
import type { WorkspaceSource } from "@bantu/protocol";

type Kind = WorkspaceSource["kind"];

const LABELS: Record<Kind, { label: string; placeholder: string }> = {
  branch: { label: "Branch", placeholder: "feature/login" },
  pull_request: { label: "Pull request", placeholder: "PR number, e.g. 42" },
  issue: { label: "Issue", placeholder: "Issue number, e.g. 17" },
};

/** Inline form: start a workspace from an existing branch, a PR, or an issue. */
export function NewFromForm({
  onCreate,
  onCancel,
}: {
  onCreate: (from: WorkspaceSource) => Promise<void>;
  onCancel: () => void;
}) {
  const [kind, setKind] = useState<Kind>("pull_request");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const number = Number.parseInt(value.replace(/^#/, ""), 10);
  const valid = kind === "branch" ? value.trim().length > 0 : Number.isInteger(number) && number > 0;

  const submit = async () => {
    if (!valid) return;
    const from: WorkspaceSource =
      kind === "branch"
        ? { kind, branch: value.trim() }
        : { kind, number };
    setBusy(true);
    try {
      await onCreate(from);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="mx-2 mb-2 flex flex-col gap-1.5 rounded-md border border-line bg-bg p-2"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") onCancel();
      }}
    >
      <div className="flex gap-1" role="radiogroup" aria-label="Start from">
        {(Object.keys(LABELS) as Kind[]).map((k) => (
          <button
            key={k}
            type="button"
            role="radio"
            aria-checked={kind === k}
            onClick={() => setKind(k)}
            className={`rounded px-1.5 py-0.5 text-[11.5px] ${kind === k ? "bg-raised text-fg" : "text-muted hover:text-fg"}`}
          >
            {LABELS[k].label}
          </button>
        ))}
      </div>
      <input
        autoFocus
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder={LABELS[kind].placeholder}
        inputMode={kind === "branch" ? "text" : "numeric"}
        className="rounded border border-line bg-panel px-2 py-1 font-mono text-[16px] outline-none focus:border-accent sm:text-[12.5px]"
      />
      {kind === "issue" && <p className="text-[11px] text-faint">The agent starts on the issue right away.</p>}
      <div className="flex justify-end gap-1.5">
        <button type="button" onClick={onCancel} className="px-1.5 text-[12px] text-muted hover:text-fg">
          Cancel
        </button>
        <button
          type="submit"
          disabled={!valid || busy}
          className="rounded bg-accent px-2 py-0.5 text-[12px] font-medium text-bg disabled:opacity-40"
        >
          {busy ? "Creating…" : "Create"}
        </button>
      </div>
    </form>
  );
}
