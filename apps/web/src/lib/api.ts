import type {
  Command,
  CommandResponse,
  Event,
  Health,
  PullRequest,
  StateSnapshot,
  WorkspaceDiff,
} from "@bantu/protocol";
import type { Connection } from "./connection.ts";

export class AuthError extends Error {}

export class DaemonClient {
  constructor(private readonly conn: Connection) {}

  private url(path: string) {
    return `${this.conn.baseUrl.replace(/\/$/, "")}${path}`;
  }

  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const res = await fetch(this.url(path), {
      ...init,
      headers: {
        Authorization: `Bearer ${this.conn.token}`,
        ...(init.body ? { "Content-Type": "application/json" } : {}),
        ...init.headers,
      },
    });
    if (res.status === 401) throw new AuthError("The daemon rejected this token.");
    let body: unknown;
    try {
      body = await res.json();
    } catch {
      throw new Error(`${path} failed with HTTP ${res.status}`);
    }
    if (!res.ok) {
      // Errors come back as a CommandResponse-shaped body with a message.
      const message = (body as { error?: unknown } | null)?.error;
      throw new Error(typeof message === "string" ? message : `${path} failed with HTTP ${res.status}`);
    }
    return body as T;
  }

  health() {
    return this.request<Health>("/v1/health");
  }

  state() {
    return this.request<StateSnapshot>("/v1/state");
  }

  /** Recent events for one workspace (logs, chat, updates), oldest first. */
  history(workspaceId: string, limit = 2000) {
    return this.request<Event[]>(`/v1/workspaces/${encodeURIComponent(workspaceId)}/history?limit=${limit}`);
  }

  diff(workspaceId: string) {
    return this.request<WorkspaceDiff>(`/v1/workspaces/${encodeURIComponent(workspaceId)}/diff`);
  }

  /** The workspace branch's pull request, or null when there isn't one (or `gh` can't tell). */
  pullRequest(workspaceId: string) {
    return this.request<PullRequest | null>(`/v1/workspaces/${encodeURIComponent(workspaceId)}/pr`);
  }

  /** WebSocket URL carrying a terminal's raw PTY bytes. */
  terminalSocketUrl(terminalId: string) {
    const base = new URL(this.conn.baseUrl || window.location.origin);
    base.protocol = base.protocol === "https:" ? "wss:" : "ws:";
    base.pathname = `${base.pathname.replace(/\/$/, "")}/v1/terminals/${encodeURIComponent(terminalId)}/ws`;
    base.search = new URLSearchParams({ token: this.conn.token }).toString();
    return base.toString();
  }

  /** Sends a command. The idempotency key makes retries after a dropped connection safe. */
  async send(command: Command): Promise<CommandResponse> {
    const res = await this.request<CommandResponse>("/v1/commands", {
      method: "POST",
      headers: { "Idempotency-Key": crypto.randomUUID() },
      body: JSON.stringify(command),
    });
    if (!res.ok) throw new Error(res.error ?? "Command failed");
    return res;
  }

  /** Opens the SSE stream from `since`. Returns a function that closes it. */
  events(since: number, onEvent: (ev: Event) => void, onError: () => void, onOpen: () => void) {
    const qs = new URLSearchParams({ since: String(since), token: this.conn.token });
    const source = new EventSource(this.url(`/v1/events?${qs}`));
    source.onopen = onOpen;
    source.onmessage = (msg) => {
      try {
        onEvent(JSON.parse(msg.data) as Event);
      } catch {
        // Ignore malformed frames; keepalives are SSE comments and never reach here.
      }
    };
    source.onerror = () => {
      // We reconnect ourselves so the resume point is always our own last seq.
      source.close();
      onError();
    };
    return () => source.close();
  }
}
