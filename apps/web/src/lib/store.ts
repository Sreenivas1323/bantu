import type {
  ChatItem,
  Event,
  LogStream,
  Repo,
  Session,
  StateSnapshot,
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
};

export const emptyState: DaemonState = {
  lastSeq: 0,
  daemonVersion: null,
  repos: {},
  workspaces: {},
  logs: {},
  sessions: {},
  chat: {},
};

export type Action =
  | { type: "snapshot"; snapshot: StateSnapshot }
  | { type: "event"; event: Event }
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
        // History isn't in the snapshot; it's backfilled per workspace.
        logs: state.logs,
        chat: state.chat,
      };
    }
    case "event":
      return applyEvent(state, action.event);
  }
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
      const entry: ChatEntry = { seq: ev.seq, ts: ev.ts, turn: data.turn, item: data.item };
      return { ...next, chat: { ...state.chat, [data.session_id]: [...prev, entry] } };
    }
  }
}
