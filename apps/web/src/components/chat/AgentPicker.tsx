import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { AgentKind, AgentStatus } from "@bantu/protocol";
import { AGENT_KINDS, agentLabel, isChatAgent, isInstalled } from "../../lib/useAgents.ts";

export type AgentChoice =
  /** A chat with the repo's agent for the role (`[agents]` in .bantu/settings.toml). */
  | { kind: "default" }
  | { kind: "lead" }
  | { kind: "chat"; agent: AgentKind }
  /** A terminal-only agent, in a workspace terminal. */
  | { kind: "terminal"; agent: AgentKind };

/**
 * "+ New chat" plus a menu of every agent: installed chat agents first, then
 * terminal-only ones, then the rest with how to install them.
 */
export function AgentPicker({
  agents,
  onPick,
  primaryLabel = "+ New chat",
  primaryClassName = "rounded px-2 py-1 hover:bg-raised hover:text-fg",
}: {
  agents: AgentStatus[] | null;
  onPick: (choice: AgentChoice) => void;
  primaryLabel?: string;
  primaryClassName?: string;
}) {
  const [open, setOpen] = useState(false);
  const toggle = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);

  // The menu sits in fixed position so the scrolling tab bar can't clip it.
  useLayoutEffect(() => {
    if (!open || !toggle.current) return;
    const r = toggle.current.getBoundingClientRect();
    const width = 320;
    setPos({ top: r.bottom + 4, left: Math.max(8, Math.min(r.left, window.innerWidth - width - 8)) });
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!menu.current?.contains(e.target as Node) && !toggle.current?.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        toggle.current?.focus();
      }
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);

  const pick = (choice: AgentChoice) => {
    setOpen(false);
    onPick(choice);
  };
  const status = (a: AgentKind) => agents?.find((s) => s.agent === a);
  const installed = AGENT_KINDS.filter((a) => isInstalled(agents, a));
  const chat = installed.filter((a) => isChatAgent(a, agents));
  const terminal = installed.filter((a) => !isChatAgent(a, agents));
  const missing = agents ? AGENT_KINDS.filter((a) => !isInstalled(agents, a)) : [];
  const item = "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left hover:bg-raised hover:text-fg focus-visible:bg-raised";

  return (
    <span className="flex shrink-0 items-center">
      <button
        type="button"
        onClick={() => pick({ kind: "default" })}
        className={primaryClassName}
        title="New chat with this repo's worker agent ([agents] worker in .bantu/settings.toml; Claude by default)"
      >
        {primaryLabel}
      </button>
      <button
        ref={toggle}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Choose an agent"
        onClick={() => setOpen((o) => !o)}
        className="rounded px-1.5 py-1 text-muted hover:bg-raised hover:text-fg"
      >
        ▾
      </button>
      {open && pos && (
        <div
          ref={menu}
          role="menu"
          style={{ top: pos.top, left: pos.left }}
          className="enter-rise fixed z-50 flex max-h-[min(70vh,520px)] w-80 flex-col overflow-y-auto rounded-md border border-line bg-panel p-1 text-[12.5px] whitespace-normal text-muted shadow-lg"
        >
          <button type="button" role="menuitem" className={item} onClick={() => pick({ kind: "lead" })}>
            <span className="flex-1">Lead chat</span>
            <span className="text-[11.5px] text-faint">splits work into workspaces</span>
          </button>
          <MenuHeading>Chat with</MenuHeading>
          {chat.map((a) => (
            <button key={a} type="button" role="menuitem" className={item} onClick={() => pick({ kind: "chat", agent: a })}>
              <span className="flex-1 text-fg">{status(a)?.name ?? agentLabel(a)}</span>
              {status(a) && !status(a)!.logged_in && <span className="text-[11.5px] text-warn">not signed in</span>}
            </button>
          ))}
          {terminal.length > 0 && (
            <>
              <MenuHeading>In a terminal</MenuHeading>
              {terminal.map((a) => (
                <button key={a} type="button" role="menuitem" className={item} onClick={() => pick({ kind: "terminal", agent: a })}>
                  <span className="flex-1 text-fg">{status(a)?.name ?? agentLabel(a)}</span>
                  <span className="text-[11.5px] text-faint">opens its own UI</span>
                </button>
              ))}
            </>
          )}
          {missing.length > 0 && (
            <>
              <MenuHeading>Not installed</MenuHeading>
              {missing.map((a) => {
                const s = status(a)!;
                return (
                  <div key={a} className="flex flex-col gap-0.5 px-2 py-1.5">
                    <span className="flex items-baseline gap-2">
                      <a href={s.docs_url} target="_blank" rel="noreferrer" className="hover:text-fg hover:underline">
                        {s.name}
                      </a>
                      {s.mode === "terminal" && <span className="text-[11px] text-faint">terminal</span>}
                    </span>
                    <code className="font-mono text-[11px] break-all text-faint select-all">{s.install_hint}</code>
                  </div>
                );
              })}
            </>
          )}
        </div>
      )}
    </span>
  );
}

function MenuHeading({ children }: { children: ReactNode }) {
  return <div className="mt-1 border-t border-line px-2 pt-2 pb-1 text-[10.5px] tracking-wide text-faint uppercase">{children}</div>;
}
