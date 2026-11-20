import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { PermissionMode, Session } from "@bantu/protocol";
import { formatCost } from "./format.ts";
import { IconButton } from "../Menu.tsx";

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "ask", label: "Ask first" },
  { value: "accept_edits", label: "Accept edits" },
  { value: "plan", label: "Plan" },
  { value: "bypass_permissions", label: "Bypass permissions" },
];

// ACP agents list their own models; type one with Custom.
const MODELS: Partial<Record<Session["agent"], string[]>> = {
  claude: ["opus", "sonnet", "haiku"],
  codex: ["gpt-5-codex", "gpt-5"],
};
const CUSTOM = "__custom__";

/** The settings under the message box are quiet until hovered or turned on, so the message stays the focus. */
const CONTROL = "rounded-md bg-transparent px-1.5 py-1 text-[12px] hover:bg-raised";
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
  onUpdate: (change: { model?: string; permission_mode?: PermissionMode; effort?: string; fast?: boolean; browser?: boolean }) => Promise<void>;
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
    // On a touch screen, focusing would open the keyboard over the transcript.
    if (!window.matchMedia("(pointer: coarse)").matches) ref.current?.focus();
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
      if (!window.matchMedia("(pointer: coarse)").matches) ref.current?.focus();
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void submit();
    }
  };

  const model = session.model ?? "";
  const known = MODELS[session.agent] ?? [];
  const modelOptions = model && !known.includes(model) ? [...known, model] : known;

  return (
    <div className="px-5 pt-1 pb-4 max-md:px-2 max-md:pb-2">
      <div className="mx-auto max-w-3xl">
        <div className="rounded-2xl border border-line bg-bg shadow-sm shadow-black/5 transition-[border-color,box-shadow] focus-within:border-faint focus-within:shadow-md focus-within:shadow-black/8">
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
            placeholder={disabledReason ?? (running ? "Queue a follow-up…" : "What should the agent do?")}
            aria-label="Message"
            className="block max-h-60 min-h-14 w-full resize-none bg-transparent px-4 pt-3.5 pb-1 text-[16px] leading-6 placeholder:text-faint focus:outline-none disabled:opacity-50 md:text-[14.5px]"
          />
          <div className="flex flex-wrap items-center gap-0.5 px-2.5 pb-2.5">
            <select
              value={session.permission_mode}
              onChange={(e) => void onUpdate({ permission_mode: e.target.value as PermissionMode })}
              aria-label="Permission mode"
              className={`${CONTROL} ${session.permission_mode === "bypass_permissions" ? "bg-warn/10 text-warn" : "text-muted hover:text-fg"}`}
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
              className={`${CONTROL} text-muted hover:text-fg`}
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
                className={`${CONTROL} text-muted hover:text-fg`}
              >
                <option value="">Default effort</option>
                {EFFORTS.map((effort) => (
                  <option key={effort} value={effort}>
                    {effort} effort
                  </option>
                ))}
              </select>
            )}
            {session.queued > 0 && (
              <span className="ml-1 rounded-full bg-raised px-2 py-0.5 text-[11.5px] text-muted">{session.queued} queued</span>
            )}
            <span className="flex-1" />
            {session.agent === "claude" && (
              <IconButton label={session.fast ? "Fast mode is on: faster output at a higher cost" : "Fast mode: faster output at a higher cost"} pressed={session.fast} onClick={() => void onUpdate({ fast: !session.fast })}>
                <path d="M8.8 1.8L3.5 9h4l-.8 5.2L12.5 7h-4z" />
              </IconButton>
            )}
            <IconButton
              label={session.browser ? "Browser is on: the agent can open and screenshot its app" : "Browser: let the agent open and screenshot its own app on this workspace's ports (needs npx)"}
              pressed={session.browser}
              onClick={() => void onUpdate({ browser: !session.browser })}
            >
              <circle cx="8" cy="8" r="5.8" />
              <path d="M2.2 8h11.6M8 2.2c1.6 1.6 2.4 3.5 2.4 5.8S9.6 12.2 8 13.8C6.4 12.2 5.6 10.3 5.6 8S6.4 3.8 8 2.2z" />
            </IconButton>
            <IconButton label="Save this agent and these settings as a loadout for new chats" onClick={onSaveLoadout}>
              <path d="M4.5 2.5h7v11L8 11l-3.5 2.5z" />
            </IconButton>
            <span className="px-1.5 text-[11.5px] tabular-nums text-faint" title="Total cost of this chat">
              {formatCost(session.total_cost_usd)}
            </span>
            {running && (
              <button
                type="button"
                onClick={() => void onStop()}
                aria-label="Stop the agent"
                title="Stop the agent"
                className="flex size-8 items-center justify-center rounded-full border border-line text-muted hover:border-bad/60 hover:text-bad"
              >
                <span aria-hidden className="size-2.5 rounded-[2px] bg-current" />
              </button>
            )}
            <button
              type="button"
              onClick={() => void submit()}
              disabled={disabled || sending || !text.trim()}
              aria-label={running ? "Queue this message" : "Send"}
              title={running ? "Queue: sent when the agent finishes this turn" : "Send"}
              className="pressable ml-1 flex size-8 items-center justify-center rounded-full bg-fg text-bg hover:bg-fg/85 disabled:bg-raised disabled:text-faint"
            >
              <svg aria-hidden viewBox="0 0 16 16" className="size-4" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                <path d="M8 13V3.5M3.8 7.5L8 3.3l4.2 4.2" />
              </svg>
            </button>
          </div>
        </div>
        <p className="mt-1.5 px-1 text-[11px] text-faint max-md:hidden">
          Enter to send · Shift+Enter for a new line
          {session.permission_mode === "bypass_permissions" && (
            <span className="text-warn"> · Bypass lets the agent run anything without asking</span>
          )}
        </p>
      </div>
    </div>
  );
}
