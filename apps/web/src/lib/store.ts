import type {
  ChatItem,
  DiffComment,
  Event,
  LogStream,
  PermissionRequest,
  Repo,
  Session,
  StateSnapshot,
  StreamDelta,
  Terminal,
  Workspace,
} from "@bantu/protocol";

export const LOG_LIMIT = 500;

export type LogLine = { seq: number; ts: string; stream: LogStream; line: string };
export type ChatEntry = { seq: number; ts: string; turn: number; item: ChatItem };

export type DaemonState = {
  lastSeq: number;
  daemonVersion: string | null;
  repos: Record<string, Repo>;
  workspaces: Record<string, Workspace>;
  logs: Record<string, LogLine[]>;
  sessions: Record<string, Session>;
  /** Transcript per session id, oldest first. */
  chat: Record<string, ChatEntry[]>;
  /** Diff review comments by id (drafts and sent). */
  comments: Record<string, DiffComment>;
  terminals: Record<string, Terminal>;
  /** Tool calls waiting for the user to allow or deny, by request id. */
  permissions: Record<string, PermissionRequest>;
  /** Assistant text still being written, per session. Cleared when the full item lands. */
  streaming: Record<string, { turn: number; text: string }>;
};

export const emptyState: DaemonState = {
  lastSeq: 0,
  daemonVersion: null,
  repos: {},
  workspaces: {},
  logs: {},
  sessions: {},
  chat: {},
  comments: {},
  terminals: {},
  permissions: {},
  streaming: {},
};

export type Action =
  | { type: "snapshot"; snapshot: StateSnapshot }
  | { type: "event"; event: Event }
  /** Older events for one workspace, merged into logs and transcripts by seq. */
  | { type: "history"; events: Event[] }
  | { type: "delta"; delta: StreamDelta }
  | { type: "reset" };

export function reducer(state: DaemonState, action: Action): DaemonState {
  switch (action.type) {
    case "reset":
      return emptyState;
    case "snapshot": {
      const { snapshot } = action;
      return {
        lastSeq: snapshot.last_seq,
        daemonVersion: snapshot.daemon_version,
        repos: Object.fromEntries(snapshot.repos.map((r) => [r.id, r])),
        workspaces: Object.fromEntries(snapshot.workspaces.map((w) => [w.id, w])),
        sessions: Object.fromEntries(snapshot.sessions.map((s) => [s.id, s])),
        comments: Object.fromEntries(snapshot.comments.map((c) => [c.id, c])),
        terminals: Object.fromEntries(snapshot.terminals.map((t) => [t.id, t])),
        permissions: Object.fromEntries(snapshot.permissions.map((p) => [p.id, p])),
        // History isn't in the snapshot; it's backfilled per workspace.
        logs: state.logs,
        chat: state.chat,
        streaming: state.streaming,
      };
    }
    case "event":
      return applyEvent(state, action.event);
    case "history":
      return mergeHistory(state, action.events);
    case "delta": {
      const { session_id, turn, text } = action.delta;
      const prev = state.streaming[session_id];
      const sofar = prev && prev.turn === turn ? prev.text : "";
      return { ...state, streaming: { ...state.streaming, [session_id]: { turn, text: sofar + text } } };
    }
  }
}

/**
 * Merges historical log lines and chat items without touching `lastSeq`:
 * live events keep flowing from the stream, and anything we already have
 * (same seq) is skipped, so the order of history vs. live arrival doesn't matter.
 */
function mergeHistory(state: DaemonState, events: Event[]): DaemonState {
  const logAdds: Record<string, LogLine[]> = {};
  const chatAdds: Record<string, ChatEntry[]> = {};
  for (const ev of events) {
    const data = ev.data;
    if (data.type === "workspace.log") {
      (logAdds[data.workspace_id] ??= []).push({ seq: ev.seq, ts: ev.ts, stream: data.stream, line: data.line });
    } else if (data.type === "session.item") {
      (chatAdds[data.session_id] ??= []).push({ seq: ev.seq, ts: ev.ts, turn: data.turn, item: data.item });
    }
  }
  const logs = { ...state.logs };
  for (const [id, adds] of Object.entries(logAdds)) logs[id] = mergeBySeq(logs[id] ?? [], adds).slice(-LOG_LIMIT);
  const chat = { ...state.chat };
  for (const [id, adds] of Object.entries(chatAdds)) chat[id] = mergeBySeq(chat[id] ?? [], adds);
  return { ...state, logs, chat };
}

function mergeBySeq<T extends { seq: number }>(current: T[], adds: T[]): T[] {
  const seen = new Set(current.map((x) => x.seq));
  const fresh = adds.filter((x) => !seen.has(x.seq));
  if (fresh.length === 0) return current;
  return [...current, ...fresh].sort((a, b) => a.seq - b.seq);
}

function applyEvent(state: DaemonState, ev: Event): DaemonState {
  if (ev.seq <= state.lastSeq) return state;
  const next = { ...state, lastSeq: ev.seq };
  const data = ev.data;
  switch (data.type) {
    case "repo.added":
      return { ...next, repos: { ...state.repos, [data.repo.id]: data.repo } };
    case "repo.removed": {
      const { [data.repo_id]: _removed, ...repos } = state.repos;
      const workspaces = Object.fromEntries(
        Object.entries(state.workspaces).filter(([, w]) => w.repo_id !== data.repo_id),
      );
      return { ...next, repos, workspaces };
    }
    case "workspace.updated":
      return {
        ...next,
        workspaces: { ...state.workspaces, [data.workspace.id]: data.workspace },
      };
    case "workspace.log": {
      const prev = state.logs[data.workspace_id] ?? [];
      const line: LogLine = { seq: ev.seq, ts: ev.ts, stream: data.stream, line: data.line };
      const lines = prev.length >= LOG_LIMIT ? [...prev.slice(prev.length - LOG_LIMIT + 1), line] : [...prev, line];
      return { ...next, logs: { ...state.logs, [data.workspace_id]: lines } };
    }
    case "session.updated":
      return { ...next, sessions: { ...state.sessions, [data.session.id]: data.session } };
    case "session.item": {
      const prev = state.chat[data.session_id] ?? [];
      // Any real item means the streamed text has been delivered in full.
      const { [data.session_id]: _streamed, ...streaming } = state.streaming;
      const entry: ChatEntry = { seq: ev.seq, ts: ev.ts, turn: data.turn, item: data.item };
      return { ...next, streaming, chat: { ...state.chat, [data.session_id]: [...prev, entry] } };
    }
    case "comment.updated":
      return { ...next, comments: { ...state.comments, [data.comment.id]: data.comment } };
    case "comment.deleted": {
      const { [data.comment_id]: _gone, ...comments } = state.comments;
      return { ...next, comments };
    }
    case "permission.requested":
      return { ...next, permissions: { ...state.permissions, [data.request.id]: data.request } };
    case "permission.resolved": {
      const { [data.request_id]: _done, ...permissions } = state.permissions;
      return { ...next, permissions };
    }
    case "terminal.updated":
      return { ...next, terminals: { ...state.terminals, [data.terminal.id]: data.terminal } };
    case "terminal.closed": {
      const { [data.terminal_id]: _closed, ...terminals } = state.terminals;
      return { ...next, terminals };
    }
    default:
      // Event types this client doesn't render yet still advance lastSeq.
      return next;
  }
}
