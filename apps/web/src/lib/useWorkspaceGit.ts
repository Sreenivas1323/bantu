import { useCallback, useEffect, useRef, useState } from "react";
import type { PullRequest, Session, Workspace, WorkspaceDiff } from "@bantu/protocol";
import type { DaemonClient } from "./api.ts";

const PR_POLL_MS = 30_000;

/**
 * The workspace's diff and pull request. The diff refreshes when an agent
 * turn finishes and when the window regains focus (the user may have edited
 * files elsewhere); the PR is polled because CI checks change on their own.
 */
export function useWorkspaceGit(client: DaemonClient, workspace: Workspace, sessions: Session[]) {
  const [diff, setDiff] = useState<WorkspaceDiff | null>(null);
  const [diffError, setDiffError] = useState<string | null>(null);
  const [pr, setPr] = useState<PullRequest | null>(null);
  const ready = workspace.state === "ready";
  const id = workspace.id;

  // Drop results that arrive after the user switched workspaces.
  const current = useRef(id);
  current.current = id;

  const refreshDiff = useCallback(async () => {
    try {
      const next = await client.diff(id);
      if (current.current === id) {
        setDiff(next);
        setDiffError(null);
      }
    } catch (err) {
      if (current.current === id) setDiffError(err instanceof Error ? err.message : String(err));
    }
  }, [client, id]);

  const refreshPr = useCallback(async () => {
    try {
      const next = await client.pullRequest(id);
      if (current.current === id) setPr(next);
    } catch {
      // No PR info is fine; the button just stays "Create PR".
    }
  }, [client, id]);

  useEffect(() => {
    setDiff(null);
    setDiffError(null);
    setPr(null);
  }, [id]);

  useEffect(() => {
    if (!ready) return;
    void refreshDiff();
    void refreshPr();
    const timer = setInterval(() => void refreshPr(), PR_POLL_MS);
    const onFocus = () => {
      void refreshDiff();
      void refreshPr();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, [ready, refreshDiff, refreshPr]);

  // Refresh once every agent in the workspace has gone quiet after working.
  const anyRunning = sessions.some((s) => s.status === "running");
  const turns = sessions.reduce((n, s) => n + s.turns, 0);
  const wasRunning = useRef(anyRunning);
  useEffect(() => {
    if (wasRunning.current && !anyRunning && ready) {
      void refreshDiff();
      void refreshPr();
    }
    wasRunning.current = anyRunning;
  }, [anyRunning, turns, ready, refreshDiff, refreshPr]);

  return { diff, diffError, pr, refreshDiff };
}
