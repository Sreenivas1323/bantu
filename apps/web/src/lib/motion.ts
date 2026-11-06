/** How long a closing dialog takes to fade out. Matches `--dur-exit` in index.css. */
const EXIT_MS = 160;

/**
 * Runs `done` once a closing element has finished its exit, so React doesn't
 * remove it mid-fade. Immediate when the user prefers reduced motion.
 */
export function afterExit(done: () => void) {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) done();
  else setTimeout(done, EXIT_MS);
}
