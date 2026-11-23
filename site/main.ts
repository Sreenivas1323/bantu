// Four small jobs: draw the dot-matrix scenes, draw the dot glyphs that mark
// the feature lists, make the copy buttons work, and point the download buttons
// at this device's file. The page reads the same without any of it.
import { glyphSvg } from "./glyphs.ts";
import { draw, paint, SCENES, type Cell } from "./scene.ts";

// ---- Glyphs ------------------------------------------------------------------

for (const el of document.querySelectorAll<HTMLElement>("[data-glyph]")) {
  el.innerHTML = glyphSvg(el.dataset.glyph ?? "");
}

// ---- Copy buttons ------------------------------------------------------------

for (const button of document.querySelectorAll<HTMLButtonElement>("button[data-copy]")) {
  const label = button.getAttribute("aria-label") ?? "Copy";
  let reset: ReturnType<typeof setTimeout> | undefined;
  button.addEventListener("click", async () => {
    const text = document.getElementById(button.dataset.copy ?? "")?.textContent ?? "";
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      return;
    }
    button.dataset.done = "";
    button.setAttribute("aria-label", "Copied");
    clearTimeout(reset);
    reset = setTimeout(() => {
      delete button.dataset.done;
      button.setAttribute("aria-label", label);
    }, 1600);
  });
}

// ---- Download buttons --------------------------------------------------------
// Every release so far is a pre-release, which GitHub's /releases/latest skips,
// so ask the API for the newest one and link its file for this device. Until
// that answers (or if it fails), the buttons open the releases page.

const REPO = "Sreenivas1323/bantu";

type Release = { draft: boolean; html_url: string; assets: { name: string; browser_download_url: string }[] };

/** The asset to offer, or null to show the release page (e.g. a Mac of unknown chip). */
async function wantedAsset(os: string): Promise<string | null> {
  if (os === "windows") return "Bantu-Windows-x64-setup.exe";
  // Chromium says which chip; Safari and Firefox don't, so they get the page with both.
  const ua = (navigator as Navigator & { userAgentData?: { getHighEntropyValues(h: string[]): Promise<{ architecture?: string }> } })
    .userAgentData;
  const arch = await ua?.getHighEntropyValues(["architecture"]).then((v) => v.architecture, () => undefined);
  if (arch === "arm") return "Bantu-macOS-apple-silicon.dmg";
  if (arch === "x86") return "Bantu-macOS-intel.dmg";
  return null;
}

async function latestRelease(): Promise<Release | undefined> {
  const key = "bantu-release";
  try {
    const cached = sessionStorage.getItem(key);
    if (cached) return JSON.parse(cached) as Release;
  } catch {}
  const response = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=5`, {
    headers: { Accept: "application/vnd.github+json" },
  });
  if (!response.ok) return undefined;
  const release = ((await response.json()) as Release[]).find((r) => !r.draft);
  if (!release) return undefined;
  const slim: Release = {
    draft: false,
    html_url: release.html_url,
    assets: release.assets.map(({ name, browser_download_url }) => ({ name, browser_download_url })),
  };
  try {
    sessionStorage.setItem(key, JSON.stringify(slim));
  } catch {}
  return slim;
}

async function linkDownloads() {
  const os = document.documentElement.dataset.os;
  const buttons = document.querySelectorAll<HTMLAnchorElement>("a[data-download]");
  if (!os || !buttons.length) return;
  const [release, name] = await Promise.all([latestRelease(), wantedAsset(os)]);
  if (!release) return;
  const asset = release.assets.find((a) => a.name === name);
  for (const button of buttons) button.href = asset?.browser_download_url ?? release.html_url;
}

linkDownloads().catch(() => {});

// ---- Scenes ------------------------------------------------------------------

const still = matchMedia("(prefers-reduced-motion: reduce)");
const dark = matchMedia("(prefers-color-scheme: dark)");
const firstVisit = document.documentElement.classList.contains("intro");

/**
 * Wires up one <canvas class="dots" data-scene="…">.
 * - `data-pitch` is the distance between dots in CSS pixels (default 8).
 * - `data-enter` lights the picture up outwards from its sun, once per visit.
 * - `data-lens` lets the pointer swell the dots under it.
 */
function mount(canvas: HTMLCanvasElement) {
  const scene = SCENES[canvas.dataset.scene ?? ""];
  const ctx = canvas.getContext("2d");
  if (!scene || !ctx) return;
  const pitch = Number(canvas.dataset.pitch) || 8;
  let cells: Cell[] = [];
  let size = { width: 0, height: 0 };
  let progress = 1;
  let lens = { x: 0, y: 0, strength: 0, target: 0 };
  let frame = 0;

  const colours = () => {
    const style = getComputedStyle(canvas);
    return { ink: style.getPropertyValue("--dot-ink").trim(), sun: style.getPropertyValue("--dot-sun").trim() };
  };

  const render = () => draw(ctx, cells, pitch, size.width, size.height, { colours: colours(), progress, lens });

  const layout = () => {
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    if (!width || !height || (width === size.width && height === size.height)) return;
    size = { width, height };
    const ratio = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    cells = paint(scene, width, height, pitch);
    render();
  };

  /** Runs while anything is still moving, then stops. */
  const tick = (now: number) => {
    let moving = false;
    if (progress < 1) {
      progress = Math.min(1, (now - started) / 1600);
      moving = true;
    }
    const gap = lens.target - lens.strength;
    if (Math.abs(gap) > 0.01) {
      lens.strength += gap * 0.18;
      moving = true;
    } else {
      lens.strength = lens.target;
    }
    render();
    frame = moving ? requestAnimationFrame(tick) : 0;
  };
  const wake = () => {
    if (!frame) frame = requestAnimationFrame(tick);
  };

  let started = 0;
  new ResizeObserver(layout).observe(canvas);
  layout();
  canvas.dataset.ready = "";

  if ("enter" in canvas.dataset && firstVisit && !still.matches) {
    progress = 0;
    started = performance.now();
    wake();
  }

  if ("lens" in canvas.dataset && matchMedia("(hover: hover)").matches) {
    canvas.addEventListener("pointermove", (event) => {
      const box = canvas.getBoundingClientRect();
      lens.x = event.clientX - box.left;
      lens.y = event.clientY - box.top;
      lens.target = 1;
      if (still.matches) {
        lens.strength = 1;
        render();
      } else wake();
    });
    canvas.addEventListener("pointerleave", () => {
      lens.target = 0;
      if (still.matches) {
        lens.strength = 0;
        render();
      } else wake();
    });
  }

  dark.addEventListener("change", render);
}

for (const canvas of document.querySelectorAll<HTMLCanvasElement>("canvas.dots")) mount(canvas);
