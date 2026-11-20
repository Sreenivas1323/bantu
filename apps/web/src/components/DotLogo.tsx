import type { CSSProperties } from "react";

/**
 * Bantu's mark drawn in dots: the pawn standing on its shield. `#` is the
 * pawn, `o` the shield. Fifteen by seventeen, so it centres on one column.
 */
const GRID = [
  ".......o.......",
  "....ooooooo....",
  "oooooo###oooooo",
  "ooooo#####ooooo",
  "ooooo#####ooooo",
  ".ooooo###ooooo.",
  ".ooo#######ooo.",
  ".ooooo###ooooo.",
  ".ooooo###ooooo.",
  ".ooooo###ooooo.",
  "..ooo#####ooo..",
  "..o#########o..",
  "...ooooooooo...",
  "....ooooooo....",
  ".....ooooo.....",
  "......ooo......",
  ".......o.......",
];

export const DOT_COLS = GRID[0].length;
export const DOT_ROWS = GRID.length;

export type Dot = { x: number; y: number; kind: "pawn" | "rim" | "shield" };

const at = (x: number, y: number) => GRID[y]?.[x] ?? ".";

/** Every lit dot. Shield dots on the outside edge are the rim, drawn brighter, like the icon's. */
export const DOTS: Dot[] = GRID.flatMap((row, y) =>
  [...row].flatMap((cell, x): Dot[] => {
    if (cell === "#") return [{ x, y, kind: "pawn" }];
    if (cell !== "o") return [];
    const edge = [at(x - 1, y), at(x + 1, y), at(x, y - 1), at(x, y + 1)].includes(".");
    return [{ x, y, kind: edge ? "rim" : "shield" }];
  }),
);

/** Where the pawn's head sits, which entrances grow out from. */
const CENTRE = { x: 7, y: 3.5 };

export type DotLogoMotion = "none" | "enter" | "loading";

/**
 * The dot-matrix mark. `brand` uses Bantu's blue for the shield and the text
 * colour for the pawn; `mono` draws everything in `currentColor`, so it sits
 * on any surface. Reads best at 40px and up; use the app icon below that.
 *
 * - `enter` assembles once, from the pawn outwards.
 * - `loading` sends a slow diagonal shimmer across it, for as long as it's shown.
 */
export function DotLogo({
  size = 64,
  tone = "brand",
  motion = "none",
  label = "Bantu",
  className = "",
}: {
  /** Height in pixels; the width follows the grid. */
  size?: number;
  tone?: "brand" | "mono";
  motion?: DotLogoMotion;
  /** Read by screen readers. Pass "" when text beside it already says Bantu. */
  label?: string;
  className?: string;
}) {
  const fill = (kind: Dot["kind"]) =>
    tone === "mono"
      ? { pawn: "currentColor", rim: "currentColor", shield: "currentColor" }[kind]
      : { pawn: "var(--color-fg)", rim: "var(--color-accent)", shield: "var(--color-accent)" }[kind];
  const opacity = (kind: Dot["kind"]) => ({ pawn: 1, rim: tone === "mono" ? 0.55 : 0.9, shield: tone === "mono" ? 0.28 : 0.42 })[kind];

  return (
    <svg
      viewBox={`0 0 ${DOT_COLS} ${DOT_ROWS}`}
      height={size}
      width={(size * DOT_COLS) / DOT_ROWS}
      role={label ? "img" : undefined}
      aria-label={label || undefined}
      aria-hidden={label ? undefined : true}
      className={`dot-logo ${motion !== "none" ? `dot-logo-${motion}` : ""} ${className}`}
    >
      {DOTS.map((d) => {
        // Stagger by distance: diagonal for the shimmer, outward from the pawn for the entrance.
        const step = motion === "loading" ? d.x + d.y : Math.hypot(d.x - CENTRE.x, d.y - CENTRE.y);
        return (
          <circle
            key={`${d.x}-${d.y}`}
            cx={d.x + 0.5}
            cy={d.y + 0.5}
            r={0.36}
            fill={fill(d.kind)}
            opacity={opacity(d.kind)}
            data-kind={d.kind}
            style={{ "--o": opacity(d.kind), "--step": step.toFixed(2) } as CSSProperties}
          />
        );
      })}
    </svg>
  );
}

/** A full-area wait: the mark shimmering above a line of text. */
export function LogoLoader({ label, size = 72 }: { label: string; size?: number }) {
  return (
    <div role="status" className="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
      <DotLogo size={size} motion="loading" label="" />
      <p className="text-muted">{label}</p>
    </div>
  );
}
