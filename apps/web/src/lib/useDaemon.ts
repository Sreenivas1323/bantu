import { useCallback, useEffect, useMemo, useReducer, useState } from "react";
import type { Command } from "@bantu/protocol";
import { AuthError, DaemonClient } from "./api.ts";
import type { Connection } from "./connection.ts";
import { emptyState, reducer } from "./store.ts";

export type ConnStatus = "connecting" | "live" | "reconnecting" | "unauthorized";

const MAX_BACKOFF_MS = 10_000;

/**
 * Loads a snapshot, then follows the event stream from its `last_seq`.
 * Every reconnect re-fetches the snapshot first: that detects a revoked token
 * (EventSource can't see status codes) and repairs any gap in the stream.
 */
export function useDaemon(conn: Connection) {
  const [state, dispatch] = useReducer(reducer, emptyState);
  const [status, setStatus] = useState<ConnStatus>("connecting");
  const client = useMemo(() => new DaemonClient(conn), [conn]);

  useEffect(() => {
    let cancelled = false;
    let closeStream: (() => void) | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;
    let seq = 0;

    dispatch({ type: "reset" });
    setStatus("connecting");

    const scheduleRetry = () => {
      if (cancelled) return;
      setStatus("reconnecting");
      const delay = Math.min(500 * 2 ** attempt, MAX_BACKOFF_MS);
      attempt += 1;
      clearTimeout(retryTimer);
      retryTimer = setTimeout(sync, delay);
    };

    async function sync() {
      closeStream?.();
      closeStream = null;
      try {
        const snapshot = await client.state();
        if (cancelled) return;
        dispatch({ type: "snapshot", snapshot });
        seq = snapshot.last_seq;
        closeStream = client.events(
          seq,
          (event) => {
            if (event.seq <= seq) return;
            if (event.seq > seq + 1) {
              // The daemon promises gap-free seqs; a gap means we missed something.
              void sync();
              return;
            }
            seq = event.seq;
            dispatch({ type: "event", event });
          },
          scheduleRetry,
          () => {
            attempt = 0;
            setStatus("live");
          },
          (delta) => dispatch({ type: "delta", delta }),
        );
      } catch (err) {
        if (cancelled) return;
        if (err instanceof AuthError) {
          setStatus("unauthorized");
          return;
        }
        scheduleRetry();
      }
    }

    void sync();
    return () => {
      cancelled = true;
      clearTimeout(retryTimer);
      closeStream?.();
    };
  }, [client]);

  const send = useCallback((command: Command) => client.send(command), [client]);

  const loadHistory = useCallback(
    async (workspaceId: string) => {
      const events = await client.history(workspaceId);
      dispatch({ type: "history", events });
    },
    [client],
  );

  return { state, status, send, loadHistory, client };
}
