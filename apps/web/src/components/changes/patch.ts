export type PatchLine =
  | { kind: "hunk"; text: string }
  | { kind: "context" | "add" | "del"; text: string; oldLine: number | null; newLine: number | null };

/** Splits unified-diff hunks into lines numbered in the old and new file. */
export function parsePatch(patch: string): PatchLine[] {
  const out: PatchLine[] = [];
  let oldLine = 0;
  let newLine = 0;
  for (const raw of patch.split("\n")) {
    if (raw.startsWith("@@")) {
      const m = /@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(raw);
      oldLine = m ? Number(m[1]) : 0;
      newLine = m ? Number(m[2]) : 0;
      out.push({ kind: "hunk", text: raw });
    } else if (raw.startsWith("+")) {
      out.push({ kind: "add", text: raw.slice(1), oldLine: null, newLine: newLine++ });
    } else if (raw.startsWith("-")) {
      out.push({ kind: "del", text: raw.slice(1), oldLine: oldLine++, newLine: null });
    } else if (raw.startsWith(" ")) {
      out.push({ kind: "context", text: raw.slice(1), oldLine: oldLine++, newLine: newLine++ });
    }
    // "\ No newline at end of file" and trailing blanks carry no content.
  }
  return out;
}
