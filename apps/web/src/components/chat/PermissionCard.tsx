import { useState } from "react";
import type { PermissionRequest } from "@bantu/protocol";
import { toolSummary } from "./format.ts";

/** An agent tool call waiting for the user. Allow runs it; Deny tells the agent why. */
export function PermissionCard({
  request,
  workspacePath,
  onRespond,
}: {
  request: PermissionRequest;
  workspacePath: string;
  onRespond: (allow: boolean, message: string | null) => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [denying, setDenying] = useState(false);
  const [reason, setReason] = useState("");
  const [showInput, setShowInput] = useState(false);
  const summary = toolSummary(request.tool_name, request.tool_input, workspacePath);

  const respond = async (allow: boolean, message: string | null) => {
    setBusy(true);
    try {
      await onRespond(allow, message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div role="alertdialog" aria-label={`Allow ${request.tool_name}?`} className="rounded-lg border border-warn/50 bg-warn/5 p-3">
      <div className="flex items-baseline gap-2">
        <span className="text-[12px] font-medium text-warn">Needs your approval</span>
        <span className="font-medium">{request.tool_name}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-muted" title={summary}>
          {summary}
        </span>
        <button type="button" onClick={() => setShowInput((v) => !v)} className="shrink-0 text-[12px] text-faint hover:text-fg">
          {showInput ? "Hide details" : "Details"}
        </button>
      </div>
      {showInput && (
        <pre className="mt-2 max-h-48 overflow-auto rounded bg-bg p-2 font-mono text-[12px] text-muted">
          {JSON.stringify(request.tool_input, null, 2)}
        </pre>
      )}
      {denying ? (
        <form
          className="mt-2.5 flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void respond(false, reason.trim() || null);
          }}
        >
          <input
            autoFocus
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder="Optional: tell the agent what to do instead"
            className="min-w-0 flex-1 rounded border border-line bg-bg px-2 py-1 text-[16px] outline-none focus:border-accent sm:text-[13px]"
          />
          <button type="submit" disabled={busy} className="rounded border border-bad/50 px-3 py-1 text-bad hover:bg-bad/10 disabled:opacity-50">
            Deny
          </button>
          <button type="button" onClick={() => setDenying(false)} className="px-2 text-muted hover:text-fg">
            Back
          </button>
        </form>
      ) : (
        <div className="mt-2.5 flex gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={() => void respond(true, null)}
            className="rounded bg-accent px-3 py-1 font-medium text-bg hover:opacity-90 disabled:opacity-50"
          >
            Allow
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => setDenying(true)}
            className="rounded border border-line px-3 py-1 text-muted hover:text-fg disabled:opacity-50"
          >
            Deny…
          </button>
        </div>
      )}
    </div>
  );
}
