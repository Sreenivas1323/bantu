import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { PermissionMode, Session } from "@bantu/protocol";
import { formatCost } from "./format.ts";

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "ask", label: "Ask first" },
  { value: "accept_edits", label: "Accept edits" },
  { value: "plan", label: "Plan" },
  { value: "bypass_permissions", label: "Bypass permissions" },
];

const MODELS: Record<Session["agent"], string[]> = {
  claude: ["opus", "sonnet", "haiku"],
  codex: ["gpt-5-codex", "gpt-5"],
  // ACP agents list their own models; type one with Custom.
  opencode: [],
  gemini: [],
  cursor: [],
};
const CUSTOM = "__custom__";
const EFFORTS = ["low", "medium", "high", "xhigh", "max"];

export function Composer({
  session,
  disabledReason,
  onSend,
  onStop,
  onUpdate,
  onSaveLoadout,
}: {
  session: Session;
  /** Set when sending isn't possible (e.g. the workspace isn't ready). */
  disabledReason: string | null;
  onSend: (text: string) => Promise<boolean>;
  onStop: () => Promise<void>;
  onUpdate: (change: { model?: string; permission_mode?: PermissionMode; effort?: string; fast?: boolean }) => Promise<void>;
  /** Saves this chat's agent and settings under a name. */
  onSaveLoadout: () => void;
}) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);
  const running = session.status === "running";
  const disabled = disabledReason !== null;

  // Each chat keeps its own draft while you switch around.
  const drafts = useRef(new Map<string, string>());
  useEffect(() => {
    setText(drafts.current.get(session.id) ?? "");
    ref.current?.focus();
  }, [session.id]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
  }, [text]);

  const submit = async () => {
    const value = text.trim();
    if (!value || disabled || sending) return;
    setSending(true);
    try {
      if (await onSend(value)) {
        setText("");
        drafts.current.delete(session.id);
      }
    } finally {
      setSending(false);
      ref.current?.focus();
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void submit();
    }
  };

  const model = session.model ?? "";
  const known = MODELS[session.agent];
  const modelOptions = model && !known.includes(model) ? [...known, model] : known;

  return (
    <div className="border-t border-line bg-panel/60 px-5 pt-3 pb-3">
      <div className="mx-auto max-w-3xl">
        <div className="rounded-lg border border-line bg-bg focus-within:border-accent/60">
          <textarea
            ref={ref}
            value={text}
            rows={1}
            disabled={disabled}
            onChange={(e) => {
              setText(e.target.value);
              drafts.current.set(session.id, e.target.value);
            }}
            onKeyDown={onKeyDown}
            placeholder={disabledReason ?? (running ? "Queue a follow-up…" : "Ask the agent to do something…")}
            aria-label="Message"
            className="block max-h-60 w-full resize-none bg-transparent px-3 py-2.5 text-[16px] leading-6 placeholder:text-faint focus:outline-none disabled:opacity-50"
          />
          <div className="flex flex-wrap items-center gap-2 px-2 pb-2">
            <select
              value={session.permission_mode}
              onChange={(e) => void onUpdate({ permission_mode: e.target.value as PermissionMode })}
              aria-label="Permission mode"
              className={`rounded-md border bg-panel px-1.5 py-1 text-[12px] ${
                session.permission_mode === "bypass_permissions" ? "border-warn/60 text-warn" : "border-line text-muted"
              }`}
            >
              {MODES.map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>
            <select
              value={model}
              onChange={(e) => {
                let next = e.target.value;
                if (next === CUSTOM) {
                  const typed = window.prompt("Model name or alias", model)?.trim();
                  if (!typed) return;
                  next = typed;
                }
                void onUpdate({ model: next });
              }}
              aria-label="Model"
              className="rounded-md border border-line bg-panel px-1.5 py-1 text-[12px] text-muted"
            >
              <option value="">Default model</option>
              {modelOptions.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
              <option value={CUSTOM}>Custom…</option>
            </select>
            {(session.agent === "claude" || session.agent === "codex") && (
              <select
                value={session.effort ?? ""}
                onChange={(e) => void onUpdate({ effort: e.target.value })}
                aria-label="Effort"
                className="rounded-md border border-line bg-panel px-1.5 py-1 text-[12px] text-muted"
              >
                <option value="">Default effort</option>
                {EFFORTS.map((effort) => (
                  <option key={effort} value={effort}>
                    {effort} effort
                  </option>
                ))}
              </select>
            )}
            {session.agent === "claude" && (
              <button
                type="button"
                aria-pressed={session.fast}
                onClick={() => void onUpdate({ fast: !session.fast })}
                title="Fast mode: faster output at a higher cost"
                className={`rounded-md border px-1.5 py-1 text-[12px] ${session.fast ? "border-accent/60 text-accent" : "border-line text-muted hover:text-fg"}`}
              >
                Fast
              </button>
            )}
            <button
              type="button"
              onClick={onSaveLoadout}
              title="Save this agent and these settings as a loadout for new chats"
              className="rounded-md border border-line px-1.5 py-1 text-[12px] text-muted hover:text-fg"
            >
              Save…
            </button>
            {session.queued > 0 && (
              <span className="rounded-full bg-raised px-2 py-0.5 text-[11.5px] text-muted">{session.queued} queued</span>
            )}
            <span className="ml-auto text-[11.5px] tabular-nums text-faint" title="Total cost of this chat">
              {formatCost(session.total_cost_usd)}
            </span>
            {running && (
              <button
                type="button"
                onClick={() => void onStop()}
                className="rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:border-bad/60 hover:text-bad"
              >
                Stop
              </button>
            )}
            <button
              type="button"
              onClick={() => void submit()}
              disabled={disabled || sending || !text.trim()}
              className="rounded-md bg-accent px-3 py-1 text-[12px] font-medium text-bg disabled:opacity-40"
            >
              {running ? "Queue" : "Send"}
            </button>
          </div>
        </div>
        <p className="mt-1.5 text-[11px] text-faint">
          Enter to send · Shift+Enter for a new line
          {session.permission_mode === "bypass_permissions" && (
            <span className="text-warn"> · Bypass lets the agent run anything without asking</span>
          )}
        </p>
      </div>
    </div>
  );
}
