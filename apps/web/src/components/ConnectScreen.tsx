import { useState, type FormEvent } from "react";
import type { Connection } from "../lib/connection.ts";

export function ConnectScreen({
  error,
  onConnect,
}: {
  error: string | null;
  onConnect: (conn: Connection) => void;
}) {
  const [baseUrl, setBaseUrl] = useState("");
  const [token, setToken] = useState("");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!token.trim()) return;
    onConnect({ baseUrl: baseUrl.trim(), token: token.trim() });
  };

  return (
    <div className="flex h-full items-center justify-center p-6">
      <form onSubmit={submit} className="w-full max-w-sm space-y-4 rounded-lg border border-line bg-panel p-6">
        <div>
          <h1 className="text-base font-semibold">Connect to a Bantu daemon</h1>
          <p className="mt-1 text-muted">
            Run <code className="font-mono text-fg">bantud --print-token</code> on the machine running the daemon.
          </p>
        </div>
        <label className="block space-y-1">
          <span className="text-muted">Daemon URL</span>
          <input
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder="Same as this page"
            autoComplete="url"
            spellCheck={false}
            className="w-full rounded-md border border-line bg-bg px-2.5 py-1.5 text-[16px] sm:text-[13px] placeholder:text-faint"
          />
        </label>
        <label className="block space-y-1">
          <span className="text-muted">Token</span>
          <input
            value={token}
            onChange={(e) => setToken(e.target.value)}
            type="password"
            autoComplete="off"
            spellCheck={false}
            autoFocus
            className="w-full rounded-md border border-line bg-bg px-2.5 py-1.5 font-mono text-[16px] sm:text-[13px]"
          />
        </label>
        {error && <p className="text-bad">{error}</p>}
        <button
          type="submit"
          disabled={!token.trim()}
          className="w-full rounded-md bg-accent px-3 py-1.5 font-medium text-bg disabled:opacity-40"
        >
          Connect
        </button>
      </form>
    </div>
  );
}
