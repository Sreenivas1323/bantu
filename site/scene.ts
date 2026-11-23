// Pictures drawn on the Bantu dot grid. A scene is a pure function of
// position with a fixed seed, so it is the same picture on every visit and at
// every size; only the number of dots changes. Every dot sits on the grid and
// only its size and strength vary, the way a dot-matrix print builds an image.
//
// The same renderer draws the print in the hero and the ridge that closes the
// page. Add a scene to SCENES and put <canvas class="dots" data-scene="name">
// wherever it should appear.

const SEED = 20261120;

/** A repeatable 0..1 value for an integer position. */
function hash(n: number, salt: number): number {
  let h = (Math.imul(n | 0, 374761393) + Math.imul(salt | 0, 668265263) + SEED) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967295;
}

const smooth = (t: number) => t * t * (3 - 2 * t);
const clamp = (v: number, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, v));
const mix = (a: number, b: number, t: number) => a + (b - a) * t;

/** Smooth 1-D noise, 0..1. */
function noise(x: number, salt: number): number {
  const i = Math.floor(x);
  return mix(hash(i, salt), hash(i + 1, salt), smooth(x - i));
}

/** Layered noise, 0..1, for rolling hills. */
function hills(x: number, salt: number): number {
  let sum = 0;
  let amp = 0.5;
  let freq = 1;
  let total = 0;
  for (let o = 0; o < 3; o++) {
    sum += amp * noise(x * freq, salt + o * 17);
    total += amp;
    amp *= 0.5;
    freq *= 2.03;
  }
  // Noise bunches around the middle; stretch it so hills have real height.
  return clamp((sum / total - 0.28) / 0.44);
}

/** A row of sharp peaks, 0..1: one straight-sided summit per unit, each with its own height, width and place. */
function peaks(x: number, salt: number): number {
  const i = Math.floor(x);
  let top = 0;
  for (let k = i - 1; k <= i + 1; k++) {
    const centre = k + 0.5 + (hash(k, salt) - 0.5) * 0.5;
    const height = 0.5 + 0.5 * hash(k, salt + 1);
    const half = 0.6 + 0.45 * hash(k, salt + 2);
    top = Math.max(top, height * (1 - Math.abs(x - centre) / half));
  }
  // A little roughness, so the sides are not ruled lines.
  return clamp(top + 0.1 * (noise(x * 5, salt + 3) - 0.5));
}

/** One mountain range. `base` and `amp` are fractions of the picture's height; `freq` is peaks per picture width. */
type Range = { base: number; amp: number; freq: number; tone: number; ridged: boolean; salt: number; firs?: boolean };

export type Scene = {
  /** Far to near. */
  ranges: Range[];
  /** Centre as fractions of width and height; radius as a fraction of the shorter side. */
  sun?: { u: number; v: number; r: number };
  /** Lead bird of each flock, as fractions of width and height. */
  flocks: { u: number; v: number }[];
  /** How heavy the sky is at the very top, 0..1. It clears towards the horizon. */
  sky: number;
};

export const SCENES: Record<string, Scene> = {
  // The hero: a low sun over five Himalayan ranges, firs along the nearest slope.
  print: {
    sun: { u: 0.66, v: 0.21, r: 0.15 },
    flocks: [
      { u: 0.3, v: 0.16 },
      { u: 0.2, v: 0.25 },
      { u: 0.4, v: 0.1 },
    ],
    sky: 0.08,
    ranges: [
      { base: 0.6, amp: 0.3, freq: 2.1, tone: 0.34, ridged: true, salt: 11 },
      { base: 0.68, amp: 0.22, freq: 2.7, tone: 0.48, ridged: true, salt: 23 },
      { base: 0.78, amp: 0.17, freq: 2.2, tone: 0.62, ridged: false, salt: 37 },
      { base: 0.88, amp: 0.14, freq: 2.6, tone: 0.78, ridged: false, salt: 53 },
      { base: 0.97, amp: 0.07, freq: 2.8, tone: 1, ridged: false, salt: 71, firs: true },
    ],
  },
  // The stage behind the product shot: a wide sky with the sun to the right,
  // and peaks that stand clear of the window set in front of them.
  panorama: {
    // The window covers everything below the top fifth, so the whole
    // picture is composed there; under it the ground runs on down the sides.
    sun: { u: 0.8, v: 0.058, r: 0.045 },
    flocks: [
      { u: 0.1, v: 0.06 },
      { u: 0.16, v: 0.1 },
      { u: 0.6, v: 0.045 },
    ],
    sky: 0.04,
    ranges: [
      { base: 0.2, amp: 0.1, freq: 4.3, tone: 0.3, ridged: true, salt: 211 },
      { base: 0.235, amp: 0.11, freq: 5.3, tone: 0.44, ridged: true, salt: 223 },
      { base: 0.275, amp: 0.08, freq: 4.6, tone: 0.6, ridged: false, salt: 237 },
      { base: 0.33, amp: 0.06, freq: 5.4, tone: 0.85, ridged: false, salt: 251, firs: true },
    ],
  },
  // The page's last line: three ranges and a tree line, wide and low.
  ridge: {
    flocks: [{ u: 0.82, v: 0.2 }],
    sky: 0,
    ranges: [
      { base: 0.7, amp: 0.62, freq: 7, tone: 0.34, ridged: true, salt: 113 },
      { base: 0.84, amp: 0.4, freq: 5, tone: 0.6, ridged: false, salt: 127 },
      { base: 0.98, amp: 0.26, freq: 6, tone: 1, ridged: false, salt: 139, firs: true },
    ],
  },
};

/** The top edge of a range at horizontal position u (0..1). */
function crest(r: Range, u: number, firsPerWidth: number): number {
  let y = r.base - r.amp * (r.ridged ? peaks(u * r.freq, r.salt) : hills(u * r.freq, r.salt));
  if (r.firs) {
    const t = u * firsPerWidth;
    const i = Math.floor(t);
    y -= r.amp * (0.5 + 0.9 * hash(i, 97)) * (1 - Math.abs(2 * (t - i) - 1));
  }
  return y;
}

const BIRD = [
  [0, 0],
  [-1, -1],
  [1, -1],
  [-2, -2],
  [2, -2],
];

export type Cell = { x: number; y: number; tone: number; sun: boolean; delay: number };

/** Every grid position of a scene, for a canvas of this size and dot pitch. */
export function paint(scene: Scene, width: number, height: number, pitch: number): Cell[] {
  const cols = Math.floor(width / pitch);
  const rows = Math.floor(height / pitch);
  // Centre the grid, so the margin of bare paper is the same on every side.
  const ox = (width - (cols - 1) * pitch) / 2;
  const oy = (height - (rows - 1) * pitch) / 2;
  const short = Math.min(width, height);
  // A fir is about four dots wide whatever the size of the picture.
  const firs = cols / 4;

  const birds = new Set<string>();
  for (const flock of scene.flocks) {
    const ci = Math.round(flock.u * (cols - 1));
    const cj = Math.round(flock.v * (rows - 1));
    for (const [bx, by] of BIRD) birds.add(`${ci + bx},${cj + by}`);
  }

  const sun = scene.sun && { x: scene.sun.u * width, y: scene.sun.v * height, r: scene.sun.r * short };
  const origin = sun ?? { x: width / 2, y: height };
  const far = Math.hypot(Math.max(origin.x, width - origin.x), Math.max(origin.y, height - origin.y));

  const cells: Cell[] = [];
  for (let j = 0; j < rows; j++) {
    for (let i = 0; i < cols; i++) {
      const x = ox + i * pitch;
      const y = oy + j * pitch;
      const u = x / width;
      const v = y / height;
      let tone = -1;
      let isSun = false;

      // The nearest range that covers this point wins.
      for (let k = scene.ranges.length - 1; k >= 0 && tone < 0; k--) {
        const r = scene.ranges[k];
        const top = crest(r, u, firs);
        if (v < top) continue;
        const depth = v - top;
        // Each range is strongest along its crest and thins into haze below it.
        tone = r.tone * mix(1, 0.62, clamp(depth / 0.14));
        // A firmer line along the crest itself, the way a woodblock print outlines a ridge.
        if (depth < pitch / height) tone *= 1.45;
        // The sun is to the right: slopes falling that way are lit, the others are in shade.
        const slope = (crest(r, u + 0.012, firs) - crest(r, u - 0.012, firs)) / 0.024;
        const lit = r.firs ? 0.5 : clamp(0.5 + slope * 1.4);
        tone *= mix(1.22, 0.72, lit);
        // Snow: bare paper just under the highest crests of the far ranges.
        if (r.ridged) {
          const high = clamp((r.base - 0.5 * r.amp - top) / (0.4 * r.amp));
          const line = high * (0.03 + 0.05 * noise(u * 30, 131 + k));
          if (depth >= pitch / height && depth < line) tone *= lit > 0.5 ? 0.12 : 0.45;
        }
      }

      if (tone < 0) {
        tone = mix(scene.sky, 0, clamp(v / 0.45));
        if (sun) {
          const d = Math.hypot(x - sun.x, y - sun.y);
          if (d < sun.r) {
            tone = 1;
            isSun = true;
          } else if (d - sun.r < short * 0.32) {
            // Rings of light around the sun, fading out.
            const reach = (d - sun.r) / short;
            tone += 0.2 * (0.5 + 0.5 * Math.cos((reach / 0.085) * Math.PI * 2)) * (1 - reach / 0.32) ** 1.5;
          }
        }
        if (birds.has(`${i},${j}`)) tone = 0.8;
      }

      cells.push({ x, y, tone: clamp(tone), sun: isSun, delay: Math.hypot(x - origin.x, y - origin.y) / far });
    }
  }
  return cells;
}

const STEPS = 10;

export type DrawOptions = {
  colours: { ink: string; sun: string };
  /** 0..1 for the entrance, where dots light up outwards from the sun; 1 is the finished picture. */
  progress?: number;
  /** A lens under the pointer: dots near it swell, and bare grid lights up. */
  lens?: { x: number; y: number; strength: number } | null;
};

/** Draws a scene's cells. The canvas context must already be scaled to CSS pixels. */
export function draw(ctx: CanvasRenderingContext2D, cells: Cell[], pitch: number, width: number, height: number, options: DrawOptions) {
  const { colours, progress = 1, lens } = options;
  ctx.clearRect(0, 0, width, height);

  // One path per strength, so the canvas changes colour ten times, not once per dot.
  const buckets: Path2D[] = Array.from({ length: STEPS + 1 }, () => new Path2D());
  const sun = new Path2D();
  const reach = pitch * 9;
  for (const c of cells) {
    // Each dot takes the last 35% of its own slice of the entrance to grow in.
    const grown = progress >= 1 ? 1 : clamp((progress * 1.35 - c.delay) / 0.35);
    if (grown <= 0) continue;
    let tone = c.tone;
    let swell = 1;
    if (lens && lens.strength > 0) {
      const d = Math.hypot(c.x - lens.x, c.y - lens.y);
      if (d < reach) {
        const near = (1 - d / reach) ** 2 * lens.strength;
        tone = Math.max(tone, 0.45 * near);
        swell += 0.5 * near;
      }
    }
    if (tone < 0.03) continue;
    const eased = 1 - (1 - grown) ** 3;
    const r = Math.min(pitch * 0.49, pitch * (0.08 + 0.38 * tone ** 0.8) * swell) * eased;
    const path = c.sun ? sun : buckets[Math.round(tone * STEPS)];
    path.moveTo(c.x + r, c.y);
    path.arc(c.x, c.y, r, 0, Math.PI * 2);
  }
  ctx.fillStyle = colours.ink;
  for (let s = 1; s <= STEPS; s++) {
    ctx.globalAlpha = 0.3 + 0.7 * (s / STEPS);
    ctx.fill(buckets[s]);
  }
  ctx.globalAlpha = 1;
  ctx.fillStyle = colours.sun;
  ctx.fill(sun);
}
