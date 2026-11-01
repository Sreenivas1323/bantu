/** Where the daemon lives and how to authenticate. Persisted in localStorage. */
export type Connection = {
  /** Empty string means "same origin as this page" (daemon-served or vite proxy). */
  baseUrl: string;
  token: string;
};

const KEY = "bantu.connection.v1";

export function loadConnection(): Connection | null {
  const fromHash = takeTokenFromHash();
  const saved = readSaved();
  if (fromHash) {
    const conn = { baseUrl: saved?.baseUrl ?? "", token: fromHash };
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

/** Reads `#token=...` and strips it from the address bar so it doesn't linger in history. */
function takeTokenFromHash(): string | null {
  const params = new URLSearchParams(window.location.hash.slice(1));
  const token = params.get("token");
  if (!token) return null;
  params.delete("token");
  const rest = params.toString();
  const url = window.location.pathname + window.location.search + (rest ? `#${rest}` : "");
  window.history.replaceState(null, "", url);
  return token;
}
