// Silk: sheets of fine glowing threads behind every app's header. Each sheet is a
// bundle of threads around a slow wave; its width swings through zero, so the sheet twists and the
// threads bunch up where it folds. Colours and strength come from CSS (--silk-*), per theme.

const SHEETS = [
  { y: 0.58, amp: 0.3, f1: 1.6, f2: 3.4, s1: 0.22, s2: -0.31, tw: 1.9, ts: 0.27, w: 0.55, ph: 0 },
  { y: 0.4, amp: 0.32, f1: 1.2, f2: 2.7, s1: -0.18, s2: 0.26, tw: 1.4, ts: -0.21, w: 0.48, ph: 2.1 },
  { y: 0.7, amp: 0.16, f1: 2.1, f2: 4.1, s1: 0.15, s2: 0.37, tw: 2.6, ts: 0.33, w: 0.36, ph: 4.4 },
];
const THREADS = 56;
const BAND = 150; // css px the waves are laid out in, whatever the header's height
const STEP = 8; // css px between points along a thread
const FRAME = 1000 / 30; // ponytail: fixed 30 fps cap; tune if Activity Monitor shows a cost

const still = matchMedia("(prefers-reduced-motion: reduce)");
// One canvas for the app's life: every render clones a fresh header, and a fresh canvas would
// start blank; this one moves into it with its last frame still drawn.
let mine: HTMLCanvasElement | null = null;
let raf = 0;
let last = 0;
let look: { colors: string[]; k: number; blend: GlobalCompositeOperation } | null = null;

function readLook(cv: HTMLCanvasElement) {
  const css = getComputedStyle(cv);
  look = {
    colors: [1, 2, 3].map((i) => css.getPropertyValue(`--silk-${i}`).trim()),
    k: parseFloat(css.getPropertyValue("--silk-k")) || 0,
    blend: (css.getPropertyValue("--silk-blend").trim() || "lighter") as GlobalCompositeOperation,
  };
}
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  look = null;
  silkSync();
});

function draw(cv: HTMLCanvasElement, now: number) {
  const dpr = devicePixelRatio || 1;
  const w = cv.clientWidth;
  const h = cv.clientHeight;
  if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) {
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(h * dpr);
  }
  if (!look) readLook(cv);
  const ctx = cv.getContext("2d");
  if (!ctx || !look) return;
  const t = now / 1000;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.globalCompositeOperation = "source-over";
  ctx.clearRect(0, 0, w, h);
  ctx.globalCompositeOperation = look.blend;
  ctx.lineWidth = 1 / dpr;
  SHEETS.forEach((S, si) => {
    for (let i = 0; i < THREADS; i++) {
      const v = i / (THREADS - 1) - 0.5; // place across the sheet
      ctx.beginPath();
      for (let px = -STEP; px <= w + STEP; px += STEP) {
        const x = px / w;
        const c =
          S.y +
          S.amp * Math.sin(x * S.f1 * Math.PI + t * S.s1 + S.ph) * 0.8 +
          S.amp * 0.35 * Math.sin(x * S.f2 * Math.PI + t * S.s2 + S.ph * 1.7);
        const width = S.w * Math.sin(x * S.tw * Math.PI + t * S.ts + S.ph);
        const y = (c + v * width + 0.012 * Math.sin(x * 9 + t * 0.8 + i * 0.3)) * BAND;
        if (px === -STEP) ctx.moveTo(px, y);
        else ctx.lineTo(px, y);
      }
      ctx.globalAlpha = 0.17 * Math.max(0.15, 1 - Math.abs(v) * 1.6) * look!.k; // outer threads fainter
      ctx.strokeStyle = look!.colors[si];
      ctx.stroke();
    }
  });
  ctx.globalAlpha = 1;
}

function tick(now: number) {
  raf = 0;
  const slot = document.querySelector<HTMLCanvasElement>("#site .silk canvas");
  if (!slot || document.hidden) return;
  if (mine && slot !== mine) slot.replaceWith(mine);
  else mine = slot;
  const cv = mine;
  if (now - last >= FRAME) {
    last = now;
    draw(cv, now);
  }
  if (!still.matches) raf = requestAnimationFrame(tick);
}

/** Starts or stops the threads to match the header: they move while an app is shown, the window is
 * visible and motion is allowed (otherwise one still frame). */
export function silkSync() {
  if (raf) cancelAnimationFrame(raf);
  raf = 0;
  last = 0;
  tick(performance.now());
}

document.addEventListener("visibilitychange", silkSync);
still.addEventListener("change", silkSync);
