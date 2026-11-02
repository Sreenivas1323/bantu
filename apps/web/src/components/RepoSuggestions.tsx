import { useEffect, useState } from "react";
import type { DaemonClient } from "../lib/api.ts";

type Suggestion = { path: string; name: string };

/** One-click "add" for git repos already on this machine, most recently used first. */
export function RepoSuggestions({
  client,
  onAdd,
  filter = "",
  limit = 8,
}: {
  client: DaemonClient;
  onAdd: (path: string) => Promise<void>;
  filter?: string;
  limit?: number;
}) {
  const [items, setItems] = useState<Suggestion[] | null>(null);
  const [adding, setAdding] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    client
      .repoSuggestions()
      .then((s) => live && setItems(s))
      .catch(() => live && setItems([]));
    return () => {
      live = false;
    };
  }, [client]);

  const q = filter.trim().toLowerCase();
  const shown = (items ?? []).filter((s) => !q || s.path.toLowerCase().includes(q)).slice(0, limit);
  if (shown.length === 0) return null;

  return (
    <ul className="flex flex-col gap-0.5" aria-label="Repositories on this machine">
      {shown.map((s) => (
        <li key={s.path}>
          <button
            type="button"
            disabled={adding !== null}
            onClick={async () => {
              setAdding(s.path);
              try {
                await onAdd(s.path);
                setItems((all) => all?.filter((x) => x.path !== s.path) ?? null);
              } finally {
                setAdding(null);
              }
            }}
            className="flex w-full items-baseline gap-2 rounded-md px-2 py-1 text-left hover:bg-raised disabled:opacity-50"
            title={`Add ${s.path}`}
          >
            <span className="shrink-0 font-medium">{s.name}</span>
            <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-faint">{tildify(s.path)}</span>
            <span className="shrink-0 text-[11.5px] text-muted">{adding === s.path ? "Adding…" : "Add"}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

function tildify(path: string) {
  const m = path.match(/^\/(Users|home)\/[^/]+/);
  return m ? `~${path.slice(m[0].length)}` : path;
}
