/** One-line description of a tool call, e.g. the command for Bash or the file for Edit. */
export function toolSummary(name: string, input: unknown, workspacePath: string): string {
  const obj = (input && typeof input === "object" ? input : {}) as Record<string, unknown>;
  const str = (key: string) => (typeof obj[key] === "string" ? (obj[key] as string) : undefined);
  // macOS reports /tmp paths as /private/tmp, so compare without that prefix.
  const norm = (p: string) => p.replace(/^\/private(?=\/)/, "");
  const rel = (p: string | undefined) => {
    if (!p) return undefined;
    const base = norm(workspacePath).replace(/\/$/, "") + "/";
    return norm(p).startsWith(base) ? norm(p).slice(base.length) : p;
  };
  const pick =
    (name === "Bash" && str("command")) ||
    (["Read", "Edit", "Write", "MultiEdit", "NotebookEdit"].includes(name) && rel(str("file_path") ?? str("notebook_path"))) ||
    (["Grep", "Glob"].includes(name) && [str("pattern"), rel(str("path"))].filter(Boolean).join("  in ")) ||
    ((name === "WebFetch" || name === "WebSearch") && (str("url") ?? str("query"))) ||
    ((name === "Task" || name === "Agent") && str("description")) ||
    Object.values(obj).find((v): v is string => typeof v === "string");
  return (pick || "").split("\n")[0] ?? "";
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${Math.round(s % 60)}s`;
}

export function formatCost(usd: number): string {
  if (usd === 0) return "$0";
  if (usd < 0.01) return "<$0.01";
  return `$${usd.toFixed(2)}`;
}

export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}
