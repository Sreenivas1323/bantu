// Small pictures on the Bantu dot grid, seven by seven, used where a list
// needs a mark. They are drawn from these rows the same way the logo is drawn
// from its grid, so the dots are the same size and spacing everywhere.

export const GLYPHS: Record<string, string[]> = {
  // A branch leaving the trunk: one worktree per task.
  branch: ["#...#..", "#...#..", "#..#...", "###....", "#......", "#......", "#......"],
  // A prompt: any agent's command line.
  agent: [".......", "#......", ".#.....", "..#....", ".#.....", "#..####", "......."],
  // One box feeding two: agents that start agents.
  split: ["..###..", "..###..", "...#...", ".#####.", ".#...#.", "###.###", "###.###"],
  // Rings around a point: the conflict radar.
  radar: ["..###..", ".#...#.", "#..#..#", "#.###.#", "#..#..#", ".#...#.", "..###.."],
  // Lanes of different lengths: the board.
  board: ["#.#.#.#", "#.#.#.#", "#.#.#.#", "#.#...#", "#.#...#", "#.....#", "#......"],
  // A tick: the definition of done.
  check: [".......", "......#", ".....#.", "#...#..", ".#.#...", "..#....", "......."],
  // Two overlapping frames: a second reviewer.
  review: ["#####..", "#...#..", "#.#####", "#.#.#.#", "###.#.#", "..#...#", "..#####"],
  // An arrow turning back: undo by turn.
  undo: ["..#....", ".#.....", "######.", ".#....#", "..#...#", "......#", "..####."],
  // A clock: loops.
  loop: ["..###..", ".#.#.#.", "#..#..#", "#..##.#", "#.....#", ".#...#.", "..###.."],
  // A handset: answers from your phone.
  phone: [".#####.", ".#...#.", ".#...#.", ".#...#.", ".#...#.", ".#.#.#.", ".#####."],
  // Rows with a marker each: tasks from a tracker.
  tasks: ["#.#####", ".......", "#.####.", ".......", "#.#####", ".......", "#.###.."],
  // An arrow up out of a tray: the pull request.
  ship: ["...#...", "..###..", ".#.#.#.", "...#...", "...#...", "#.....#", "#######"],
  // A cloud: your own.
  cloud: [".......", "..##...", ".####..", ".#####.", "#######", "#######", "......."],
  // A beam widening: spotlight.
  spot: ["...#...", "...#...", "..###..", "..###..", ".#####.", ".#####.", "#######"],
  // An arrow down into a tray: import.
  import: ["...#...", "...#...", ".#.#.#.", "..###..", "...#...", "#.....#", "#######"],
  // A page with a corner: a browser for the agent.
  page: ["#####..", "#...##.", "#....##", "#.....#", "#.###.#", "#.....#", "#######"],
};

/** Inline SVG for a glyph; the dots take the current text colour. */
export function glyphSvg(name: string): string {
  const rows = GLYPHS[name];
  if (!rows) return "";
  const dots = rows
    .flatMap((row, y) => [...row].map((cell, x) => (cell === "#" ? `<circle cx="${x + 0.5}" cy="${y + 0.5}" r="0.36"/>` : "")))
    .join("");
  return `<svg viewBox="0 0 7 7" aria-hidden="true" fill="currentColor">${dots}</svg>`;
}
