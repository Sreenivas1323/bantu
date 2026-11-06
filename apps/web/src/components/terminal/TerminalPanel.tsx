import type { Command, CommandResponse, Terminal, Workspace } from "@bantu/protocol";
import type { DaemonClient } from "../../lib/api.ts";
import { TerminalView } from "./TerminalView.tsx";

export function TerminalPanel({
  workspace,
  terminals,
  activeId,
  onSelect,
  client,
  run,
}: {
  workspace: Workspace;
  /** This workspace's terminals, oldest first. */
  terminals: Terminal[];
  activeId: string | null;
  onSelect: (id: string) => void;
  client: DaemonClient;
  run: (command: Command) => Promise<CommandResponse | null>;
}) {
  const active = terminals.find((t) => t.id === activeId) ?? terminals[terminals.length - 1];
  const ready = workspace.state === "ready";

  const open = async (command: string | null = null) => {
    const res = await run({ type: "terminal.open", workspace_id: workspace.id, cols: null, rows: null, command });
    const t = res?.result as Terminal | undefined;
    if (t?.id) onSelect(t.id);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="scroll-quiet flex items-center gap-0.5 overflow-x-auto border-b border-line px-3" role="tablist" aria-label="Terminals">
        {terminals.map((t) => (
          <div
            key={t.id}
            className={`group flex shrink-0 items-center gap-1 border-b-2 ${
              t.id === active?.id ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg"
            }`}
          >
            <button type="button" role="tab" aria-selected={t.id === active?.id} onClick={() => onSelect(t.id)} className="flex items-center gap-1.5 py-1.5 pl-2.5">
              <span
                aria-hidden
                className={`size-1.5 rounded-full ${t.exited ? (t.exit_code === 0 ? "bg-faint" : "bg-bad") : t.kind === "run" ? "bg-ok" : "bg-accent"}`}
              />
              <span className="max-w-40 truncate">{t.title}</span>
              {t.exited && <span className="text-[11px] text-faint">exited {t.exit_code ?? ""}</span>}
            </button>
            <button
              type="button"
              onClick={() => void run({ type: "terminal.close", terminal_id: t.id })}
              aria-label={`Close ${t.title}`}
              className="px-1.5 text-faint opacity-60 hover:text-bad group-hover:opacity-100"
            >
              ×
            </button>
          </div>
        ))}
        {ready && (
          <button type="button" onClick={() => void open()} className="ml-1 shrink-0 rounded px-2 py-1 text-muted hover:bg-raised hover:text-fg" title="New shell in this workspace">
            + Shell
          </button>
        )}
        {ready && (
          <button
            type="button"
            onClick={() => {
              const command = window.prompt("Command to run in this workspace (any CLI, e.g. aider)")?.trim();
              if (command) void open(command);
            }}
            className="shrink-0 rounded px-2 py-1 text-muted hover:bg-raised hover:text-fg"
            title="Run any CLI in a terminal in this workspace"
          >
            + Command
          </button>
        )}
      </div>
      <div className="relative min-h-0 flex-1 bg-[#16181d]">
        {terminals.length === 0 ? (
          <div className="flex h-full items-center justify-center text-muted">
            {ready ? (
              <button type="button" onClick={() => void open()} className="pressable rounded-md bg-accent px-3 py-1.5 font-medium text-bg">
                Open a shell in {workspace.name}
              </button>
            ) : (
              <p>Terminals open once the workspace is ready.</p>
            )}
          </div>
        ) : (
          terminals.map((t) => <TerminalView key={t.id} socketUrl={client.terminalSocketUrl(t.id)} visible={t.id === active?.id} />)
        )}
      </div>
    </div>
  );
}
