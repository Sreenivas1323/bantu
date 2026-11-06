import type {
  AgentStatus,
  ChildWorkspace,
  Command,
  CommandResponse,
  Event,
  ForwardedPort,
  Health,
  PullRequest,
  StateSnapshot,
  StreamDelta,
  WorkspaceChecks,
  WorkspaceDiff,
} from "@bantu/protocol";
import type { Connection } from "./connection.ts";

export class AuthError extends Error {}

/**
 * Something in front of the daemon, like Cloudflare Access, wants the user to
 * sign in again. The daemon's API never redirects, so a redirect means that.
 */
export class SignInError extends Error {}

export class DaemonClient {
  constructor(private readonly conn: Connection) {}

  private url(path: string) {
    return `${this.conn.baseUrl.replace(/\/$/, "")}${path}`;
  }

  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const res = await fetch(this.url(path), {
      ...init,
      redirect: "manual",
      headers: {
        Authorization: `Bearer ${this.conn.token}`,
        ...(init.body ? { "Content-Type": "application/json" } : {}),
        ...init.headers,
      },
    });
    if (res.type === "opaqueredirect") throw new SignInError("Your sign-in in front of the daemon has expired.");
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

  /** CI runs, deployments, PR comments and todos for the Checks tab. */
  checks(workspaceId: string) {
    return this.request<WorkspaceChecks>(`/v1/workspaces/${encodeURIComponent(workspaceId)}/checks`);
  }

  /** The workspaces this one's agent started, for its board. */
  children(workspaceId: string) {
    return this.request<ChildWorkspace[]>(`/v1/workspaces/${encodeURIComponent(workspaceId)}/children`);
  }

  /** Servers listening on a workspace's reserved ports, with links to open them. */
  ports(workspaceId: string) {
    return this.request<ForwardedPort[]>(`/v1/workspaces/${encodeURIComponent(workspaceId)}/ports`);
  }

  /** The key a browser needs to subscribe to this daemon's Web Push alerts. */
  pushKey() {
    return this.request<{ public_key: string }>("/v1/push/key");
  }

  /** Agent CLIs on the daemon's machine and the accounts they're signed in to. */
  agents() {
    return this.request<AgentStatus[]>("/v1/agents");
  }

  /** Git repos found in common code folders on the daemon's machine. */
  repoSuggestions() {
    return this.request<{ path: string; name: string }[]>("/v1/repo-suggestions");
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
  events(
    since: number,
    onEvent: (ev: Event) => void,
    onError: () => void,
    onOpen: () => void,
    onDelta: (d: StreamDelta) => void = () => {},
  ) {
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
    // Live text fragments: separate SSE event name, no seq, never replayed.
    source.addEventListener("delta", (msg) => {
      try {
        onDelta(JSON.parse((msg as MessageEvent<string>).data) as StreamDelta);
      } catch {
        // Ignore malformed frames.
      }
    });
    source.onerror = () => {
      // We reconnect ourselves so the resume point is always our own last seq.
      source.close();
      onError();
    };
    return () => source.close();
  }
}
