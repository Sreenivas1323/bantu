// Writes the dot-matrix mark as standalone SVG files, from the same grid the
// app's <DotLogo> draws, so every copy stays identical:
//
//   bun apps/web/scripts/dot-logo.ts
//
// - apps/web/public/logo-dots.svg       brand colours, light and dark
// - apps/web/public/logo-dots-mono.svg  one colour (currentColor)
// - site/assets/mark-dots.svg           the brand file, for the website
// - apps/desktop/splash/index.html      the shimmering mark on the start screen
import { DOT_COLS, DOT_ROWS, DOTS, type Dot } from "../src/components/DotLogo.tsx";

const root = new URL("../../../", import.meta.url).pathname;
const OPACITY: Record<Dot["kind"], number> = { pawn: 1, rim: 0.9, shield: 0.42 };
const MONO: Record<Dot["kind"], number> = { pawn: 1, rim: 0.55, shield: 0.28 };

const circles = (opacity: Record<Dot["kind"], number>, withStep = false) =>
  DOTS.map(
    (d) =>
      `<circle class="${d.kind}" cx="${d.x + 0.5}" cy="${d.y + 0.5}" r="0.36" opacity="${opacity[d.kind]}"${
        withStep ? ` style="--o:${opacity[d.kind]};--step:${d.x + d.y}"` : ""
      }/>`,
  ).join("");

const brandStyle = `.pawn{fill:oklch(0.22 0.01 260)}.rim,.shield{fill:oklch(0.55 0.16 255)}@media (prefers-color-scheme:dark){.pawn{fill:oklch(0.93 0.005 260)}.rim,.shield{fill:oklch(0.72 0.13 250)}}`;
const svg = (body: string, style: string, attrs = "") =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${DOT_COLS} ${DOT_ROWS}" width="${DOT_COLS * 8}" height="${DOT_ROWS * 8}"${attrs}><title>Bantu</title>${
    style ? `<style>${style}</style>` : ""
  }${body}</svg>\n`;

const brand = svg(circles(OPACITY), brandStyle);
await Bun.write(`${root}apps/web/public/logo-dots.svg`, brand);
await Bun.write(`${root}site/assets/mark-dots.svg`, brand);
await Bun.write(`${root}apps/web/public/logo-dots-mono.svg`, svg(circles(MONO), "", ' fill="currentColor"'));

// The splash has no stylesheet of ours, so the shimmer comes along inline.
const splashPath = `${root}apps/desktop/splash/index.html`;
const splash = await Bun.file(splashPath).text();
const shimmer =
  `.mark circle{transform-box:fill-box;transform-origin:center;animation:shimmer 1.8s ease-in-out infinite;animation-delay:calc(var(--step) * 50ms - 1.8s)}` +
  `@keyframes shimmer{0%,100%{opacity:calc(var(--o) * .45);scale:.8}45%{opacity:calc(var(--o) * .4 + .6);scale:1}}` +
  `@media (prefers-reduced-motion:reduce){.mark circle{animation:none}}`;
const mark = `<svg class="mark" viewBox="0 0 ${DOT_COLS} ${DOT_ROWS}" width="${(56 * DOT_COLS) / DOT_ROWS}" height="56" aria-hidden="true"><style>${brandStyle}${shimmer}</style>${circles(OPACITY, true)}</svg>`;
const marked = splash.replace(/<!-- dot-logo -->[\s\S]*?<!-- \/dot-logo -->/, `<!-- dot-logo -->${mark}<!-- /dot-logo -->`);
if (marked === splash && !splash.includes(mark)) throw new Error("splash is missing the <!-- dot-logo --> markers");
await Bun.write(splashPath, marked);
console.log(`Wrote ${DOTS.length} dots to 4 files.`);
