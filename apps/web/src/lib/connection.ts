/** Where the daemon lives and how to authenticate. Persisted in localStorage. */
export type Connection = {
  /** Empty string means "same origin as this page" (daemon-served or vite proxy). */
  baseUrl: string;
  token: string;
};

const KEY = "bantu.connection.v1";

export function loadConnection(): Connection | null {
  const fromHash = takeFromHash();
  const saved = readSaved();
  if (fromHash) {
    const conn = { baseUrl: fromHash.daemon ?? saved?.baseUrl ?? "", token: fromHash.token };
    saveConnection(conn);
    return conn;
  }
  return saved;
}

export function saveConnection(conn: Connection) {
  localStorage.setItem(KEY, JSON.stringify(conn));
}

export function clearConnection() {
  localStorage.removeItem(KEY);
}

function readSaved(): Connection | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<Connection>;
    if (typeof parsed.token !== "string" || !parsed.token) return null;
    return { baseUrl: parsed.baseUrl ?? "", token: parsed.token };
  } catch {
    return null;
  }
}

/**
 * Reads `#token=...` (and `#daemon=...`, for an app hosted apart from the daemon)
 * and strips them from the address bar so they don't linger in history.
 */
function takeFromHash(): { token: string; daemon: string | null } | null {
  const params = new URLSearchParams(window.location.hash.slice(1));
  const token = params.get("token");
  if (!token) return null;
  const daemon = params.get("daemon");
  params.delete("token");
  params.delete("daemon");
  const rest = params.toString();
  const url = window.location.pathname + window.location.search + (rest ? `#${rest}` : "");
  window.history.replaceState(null, "", url);
  return { token, daemon };
}

/**
 * Why this daemon can't be reached, in terms the user can act on, or null if it
 * answers. A daemon that's reachable but rejects the token fails later, with
 * its own message.
 */
export async function checkDaemon(baseUrl: string): Promise<string | null> {
  const base = baseUrl.replace(/\/$/, "");
  const blocked = window.location.protocol === "https:" && base.startsWith("http://") && !/^http:\/\/(localhost|127\.0\.0\.1|\[::1\])(:|\/|$)/.test(base);
  if (blocked) return "This page is on HTTPS, so browsers block a plain http:// daemon. Give the daemon an HTTPS address (see the remote access guide).";
  try {
    const res = await fetch(`${base}/v1/health`);
    const body = (await res.json()) as { ok?: boolean };
    if (body.ok) return null;
  } catch {
    // Reported below.
  }
  return baseUrl
    ? `No Bantu daemon answered at ${baseUrl}. Check the address, and that the daemon is running.`
    : "This page isn't served by a Bantu daemon. Enter your daemon's URL.";
}
