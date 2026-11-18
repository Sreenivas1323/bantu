// Served at /styles.css. The palette, type and status dots follow site/styles.css, which takes
// them from the app itself: cool grey neutrals, one accent, finished/working/needs-you dots.

export const STYLES = `
:root {
  color-scheme: light dark;
  --paper: light-dark(oklch(0.975 0.003 260), oklch(0.19 0.006 260));
  --paper-2: light-dark(oklch(0.945 0.005 260), oklch(0.225 0.007 260));
  --ink: light-dark(oklch(0.22 0.008 260), oklch(0.93 0.005 260));
  --ink-2: light-dark(oklch(0.42 0.01 260), oklch(0.72 0.01 260));
  --rule: light-dark(oklch(0.87 0.006 260), oklch(0.32 0.008 260));
  --accent: light-dark(oklch(0.48 0.14 250), oklch(0.74 0.12 250));
  --ok: oklch(0.7 0.15 150);
  --run: oklch(0.68 0.14 250);
  --wait: oklch(0.8 0.14 80);
  --bad: light-dark(oklch(0.52 0.17 25), oklch(0.72 0.14 25));
  --serif: "Iowan Old Style", "Palatino Linotype", Palatino, "Book Antiqua", Georgia, serif;
  --sans: ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  --mono: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  --page: min(60rem, 100% - 2.5rem);
}
* { box-sizing: border-box; }
html {
  background: var(--paper);
  color: var(--ink);
  font: 16px/1.55 var(--sans);
  -webkit-font-smoothing: antialiased;
}
body { margin: 0; }
::selection { background: color-mix(in oklch, var(--accent) 25%, transparent); }
a { color: inherit; text-decoration-color: color-mix(in oklch, currentColor 35%, transparent); text-underline-offset: 0.2em; }
a:hover { text-decoration-color: currentColor; }
:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; border-radius: 4px; }
h1, h2, h3 { font-family: var(--serif); font-weight: 600; letter-spacing: -0.01em; text-wrap: balance; line-height: 1.15; }
h1 { font-size: clamp(1.9rem, 4.5vw, 2.6rem); margin: 0 0 0.6rem; }
h2 { font-size: 1.45rem; margin: 0 0 0.8rem; }
h3 { font-size: 1.1rem; margin: 1.4rem 0 0.5rem; }
p, li { text-wrap: pretty; }
code, .mono { font-family: var(--mono); font-size: 0.9em; }

.skip { position: absolute; left: -999px; }
.skip:focus { left: 1rem; top: 1rem; background: var(--paper); padding: 0.4rem 0.8rem; }
.sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
.mt { margin-top: 1.2rem; }

.top { width: var(--page); margin: 0 auto; padding: 1.2rem 0; display: flex; align-items: center; justify-content: space-between; gap: 1rem; flex-wrap: wrap; }
.wordmark { display: inline-flex; align-items: center; gap: 0.6rem; font: 600 1.2rem/1 var(--serif); text-decoration: none; }
.dots { display: inline-flex; gap: 4px; }
.dots i { width: 8px; height: 8px; border-radius: 50%; }
.ok { background: var(--ok); } .run { background: var(--run); } .wait { background: var(--wait); }
.top nav { display: flex; align-items: center; gap: 1.2rem; font-size: 0.92rem; color: var(--ink-2); }
.top nav a { text-decoration: none; }
.top nav a:hover { color: var(--ink); }

main { width: var(--page); margin: 0 auto; padding: 1rem 0 4rem; }
main > section { padding: 1.8rem 0; border-top: 1px solid var(--rule); }
main > section:first-child, main > header + section { border-top: 0; }
.lede { color: var(--ink-2); max-width: 38rem; font-size: 1.05rem; margin: 0 0 1rem; }
.muted { color: var(--ink-2); }
.small { font-size: 0.88rem; }

.button, button {
  display: inline-flex; align-items: center; justify-content: center; gap: 0.4rem;
  min-height: 2.5rem; padding: 0 1rem; border: 1px solid var(--rule); border-radius: 9px;
  background: transparent; color: inherit; font: 550 0.93rem/1 var(--sans); text-decoration: none; cursor: pointer;
  transition: background-color 150ms ease, scale 150ms ease-out;
}
.button:hover, button:hover { background: var(--paper-2); }
.button:active, button:active { scale: 0.97; }
.primary { border-color: var(--ink); background: var(--ink); color: var(--paper); }
.primary:hover { background: color-mix(in oklch, var(--ink) 86%, var(--paper)); }
button.link { min-height: 0; padding: 0; border: 0; background: none; color: var(--ink-2); font-weight: 450; text-decoration: underline; text-underline-offset: 0.2em; }
button.danger { color: var(--bad); }
button.link:hover { color: var(--ink); background: none; }

form.inline { display: inline; }
.stack { display: grid; gap: 0.9rem; max-width: 34rem; }
label { display: grid; gap: 0.3rem; font-size: 0.92rem; font-weight: 550; }
label .hint { font-weight: 400; color: var(--ink-2); font-size: 0.85rem; }
input[type=text], input[type=email], input[type=password], input[type=url], select, textarea {
  font: 16px/1.4 var(--sans); color: inherit; background: var(--paper);
  padding: 0.55rem 0.7rem; border: 1px solid var(--rule); border-radius: 8px; width: 100%;
}
input:focus-visible, select:focus-visible, textarea:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
fieldset { border: 1px solid var(--rule); border-radius: 10px; padding: 0.9rem 1rem; margin: 0; display: grid; gap: 0.6rem; }
legend { font-weight: 600; padding: 0 0.3rem; }
.radio { display: flex; gap: 0.6rem; align-items: baseline; font-weight: 450; }
.radio input { margin: 0; }
details { border: 1px solid var(--rule); border-radius: 10px; padding: 0.7rem 1rem; margin: 0.6rem 0; }
details > summary { cursor: pointer; font-weight: 550; }
details[open] > summary { margin-bottom: 0.8rem; }

table { border-collapse: collapse; width: 100%; font-size: 0.93rem; }
th, td { text-align: left; padding: 0.55rem 0.6rem 0.55rem 0; border-bottom: 1px solid var(--rule); vertical-align: middle; }
th { font-weight: 550; color: var(--ink-2); font-size: 0.85rem; }
td.num, th.num { text-align: right; font-variant-numeric: tabular-nums; }
.row-actions { display: flex; gap: 0.8rem; align-items: center; flex-wrap: wrap; }
.row-actions select { width: auto; padding: 0.3rem 0.5rem; }

/* Notices carry Bantu's status dot rather than a coloured side bar. */
.notice { position: relative; border: 1px solid var(--rule); background: var(--paper-2); border-radius: 8px; padding: 0.8rem 1rem 0.8rem 2.1rem; margin: 1rem 0; max-width: 44rem; }
.notice::before { content: ""; position: absolute; left: 1rem; top: 1.3rem; width: 7px; height: 7px; border-radius: 50%; background: var(--wait); }
.notice.ok-n::before { background: var(--ok); }
.notice.err::before { background: var(--bad); }
.notice p { margin: 0.3rem 0; }
.secret { display: block; font: 600 1.05rem/1.4 var(--mono); padding: 0.7rem 0.9rem; background: var(--paper); border: 1px solid var(--rule); border-radius: 8px; overflow-wrap: anywhere; user-select: all; }
.code { font: 600 clamp(1.8rem, 6vw, 2.6rem)/1.2 var(--mono); letter-spacing: 0.08em; }
.grid { display: grid; gap: 1rem; grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr)); }
.stat { border: 1px solid var(--rule); border-radius: 10px; padding: 0.8rem 1rem; }
.stat dl { display: grid; grid-template-columns: 1fr auto; gap: 0.25rem 1rem; margin: 0; font-size: 0.93rem; }
.stat dt { color: var(--ink-2); }
.stat dd { margin: 0; text-align: right; font-variant-numeric: tabular-nums; font-weight: 550; }
.pill { display: inline-flex; align-items: center; gap: 0.35rem; font-size: 0.82rem; color: var(--ink-2); }
.pill::before { content: ""; width: 7px; height: 7px; border-radius: 50%; background: var(--ok); }
.pill.run::before { background: var(--run); } .pill.wait::before { background: var(--wait); }
.pill.run, .pill.wait { background: none; }
ul.plain { list-style: none; padding: 0; margin: 0; }
ul.plain li { padding: 0.6rem 0; border-bottom: 1px solid var(--rule); display: flex; justify-content: space-between; gap: 1rem; align-items: center; }
footer { width: var(--page); margin: 0 auto; padding: 2rem 0 3rem; border-top: 1px solid var(--rule); font-size: 0.85rem; color: var(--ink-2); }
@media (prefers-reduced-motion: reduce) { * { transition: none !important; } }
`;
