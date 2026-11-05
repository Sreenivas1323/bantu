import { useCallback, useEffect, useState, type ReactNode } from "react";
import type { CheckRun, WorkspaceChecks } from "@bantu/protocol";
import type { DaemonClient } from "../../lib/api.ts";

const BUCKET_TONE: Record<string, string> = {
  pass: "text-ok",
  fail: "text-bad",
  cancel: "text-bad",
  pending: "text-warn",
  skipping: "text-faint",
};
const BUCKET_LABEL: Record<string, string> = {
  pass: "Passed",
  fail: "Failed",
  cancel: "Cancelled",
  pending: "Running",
  skipping: "Skipped",
};

/** CI, deployments, PR comments and todos, loaded when the tab opens. */
export function ChecksPanel({
  workspaceId,
  client,
  canSend,
  onFixChecks,
  onAddressComments,
}: {
  workspaceId: string;
  client: DaemonClient;
  canSend: boolean;
  onFixChecks: () => Promise<void>;
  onAddressComments: () => Promise<void>;
}) {
  const [data, setData] = useState<WorkspaceChecks | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setData(await client.checks(workspaceId));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [client, workspaceId]);

  useEffect(() => {
    setData(null);
    void refresh();
  }, [refresh]);

  const failing = data?.checks.filter((c) => c.bucket === "fail").length ?? 0;

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex max-w-3xl flex-col gap-6 px-5 py-4">
        <div className="flex items-center gap-3">
          <p className="min-w-0 flex-1 truncate text-muted">
            {data?.pull_request ? (
              <a href={data.pull_request.url} target="_blank" rel="noreferrer" className="hover:text-fg">
                #{data.pull_request.number} {data.pull_request.title}
              </a>
            ) : data ? (
              "No pull request for this branch yet."
            ) : (
              "Loading…"
            )}
          </p>
          <button
            type="button"
            disabled={loading}
            onClick={() => void refresh()}
            className="shrink-0 rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:bg-raised hover:text-fg disabled:opacity-40"
          >
            {loading ? "Refreshing…" : "Refresh"}
          </button>
        </div>
        {error && <p className="rounded-md border border-bad/40 bg-bad/10 px-3 py-2 text-bad">{error}</p>}

        {data && (
          <>
            <Section
              title="CI checks"
              empty={data.pull_request ? "This pull request has no checks." : "Checks appear once there's a pull request."}
              count={data.checks.length}
              action={
                failing > 0 && (
                  <SendButton disabled={!canSend} onClick={onFixChecks} title="Send the failing checks and their logs to the agent">
                    Fix {failing} failing
                  </SendButton>
                )
              }
            >
              {sortChecks(data.checks).map((c) => (
                <Row key={`${c.workflow}/${c.name}`} href={c.link}>
                  <span className={`w-16 shrink-0 ${BUCKET_TONE[c.bucket] ?? "text-muted"}`}>{BUCKET_LABEL[c.bucket] ?? c.bucket}</span>
                  <span className="min-w-0 flex-1 truncate">
                    {c.workflow ? `${c.workflow} / ` : ""}
                    {c.name}
                  </span>
                  {c.description && <span className="hidden max-w-60 truncate text-faint sm:block">{c.description}</span>}
                </Row>
              ))}
            </Section>

            <Section title="Deployments" empty="No deployments for this branch." count={data.deployments.length}>
              {data.deployments.map((d) => (
                <Row key={d.environment} href={d.url}>
                  <span className={`w-16 shrink-0 ${d.state === "success" ? "text-ok" : d.state === "failure" || d.state === "error" ? "text-bad" : "text-warn"}`}>
                    {d.state.replaceAll("_", " ")}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{d.environment}</span>
                  {d.url && <span className="hidden max-w-60 truncate text-faint sm:block">{d.url}</span>}
                </Row>
              ))}
            </Section>

            <Section
              title="Comments"
              empty="No comments on the pull request."
              count={data.comments.length}
              action={
                data.comments.length > 0 && (
                  <SendButton disabled={!canSend} onClick={onAddressComments} title="Ask the agent to work through these comments">
                    Send to agent
                  </SendButton>
                )
              }
            >
              {data.comments.map((c, i) => (
                <article key={`${c.created_at}-${i}`} className="border-b border-line px-3 py-2 last:border-b-0">
                  <p className="flex flex-wrap items-baseline gap-x-2 text-[12px] text-muted">
                    <span className="font-medium text-fg">{c.author}</span>
                    {c.path && (
                      <span className="font-mono">
                        {c.path}
                        {c.line ? `:${c.line}` : ""}
                      </span>
                    )}
                    {c.url && (
                      <a href={c.url} target="_blank" rel="noreferrer" className="ml-auto hover:text-fg">
                        Open
                      </a>
                    )}
                  </p>
                  <p className="mt-1 whitespace-pre-wrap [overflow-wrap:anywhere] text-[13px] leading-5">{c.body}</p>
                </article>
              ))}
            </Section>

            <Section title="Todos" empty="No todos yet. The agent's task list and the PR's task list show up here." count={data.todos.length}>
              {data.todos.map((t, i) => (
                <div key={`${t.source}-${i}`} className="flex items-baseline gap-2 border-b border-line px-3 py-1.5 last:border-b-0">
                  <span aria-label={t.done ? "done" : "to do"} className={t.done ? "text-ok" : "text-faint"}>
                    {t.done ? "✓" : "○"}
                  </span>
                  <span className={`min-w-0 flex-1 ${t.done ? "text-muted line-through" : ""}`}>{t.text}</span>
                  <span className="shrink-0 text-[11px] text-faint">{t.source === "agent" ? "agent" : "PR"}</span>
                </div>
              ))}
            </Section>
          </>
        )}
      </div>
    </div>
  );
}

/** Failures first, then whatever's still running. */
function sortChecks(checks: CheckRun[]) {
  const order = ["fail", "cancel", "pending", "pass", "skipping"];
  return [...checks].sort((a, b) => order.indexOf(a.bucket) - order.indexOf(b.bucket));
}

function Section({ title, count, empty, action, children }: { title: string; count: number; empty: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section>
      <div className="mb-1.5 flex items-center gap-2">
        <h3 className="text-xs font-medium uppercase tracking-wide text-muted">{title}</h3>
        {count > 0 && <span className="text-[11.5px] tabular-nums text-faint">{count}</span>}
        <span className="flex-1" />
        {action}
      </div>
      {count === 0 ? <p className="text-[12.5px] text-faint">{empty}</p> : <div className="overflow-hidden rounded-md border border-line bg-panel">{children}</div>}
    </section>
  );
}

function Row({ href, children }: { href?: string | null; children: ReactNode }) {
  const className = "flex items-center gap-3 border-b border-line px-3 py-1.5 text-[13px] last:border-b-0";
  return href ? (
    <a href={href} target="_blank" rel="noreferrer" className={`${className} hover:bg-raised`}>
      {children}
    </a>
  ) : (
    <div className={className}>{children}</div>
  );
}

function SendButton({ disabled, onClick, title, children }: { disabled: boolean; onClick: () => Promise<void>; title: string; children: ReactNode }) {
  const [busy, setBusy] = useState(false);
  return (
    <button
      type="button"
      disabled={disabled || busy}
      title={title}
      onClick={async () => {
        setBusy(true);
        try {
          await onClick();
        } finally {
          setBusy(false);
        }
      }}
      className="rounded-md border border-line px-2.5 py-1 text-[12px] text-muted hover:bg-raised hover:text-fg disabled:opacity-40"
    >
      {children}
    </button>
  );
}
