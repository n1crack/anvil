/* Terminal output, shared by the main window's console and the Processes window. */

/** Terminal output as a terminal would leave it: progress bars redraw a line with \r
 * or \b; escape codes don't belong in plain text. With `colors`, SGR codes (colors,
 * bold) stay for `ansi` to draw. */
export function clean(text, colors = false) {
  return text
    .replace(
      colors
        ? /\x1b\[(?![0-9;]*m)[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)?/g
        : /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)?/g,
      "",
    )
    .split("\n")
    .map((l) => {
      l = l.slice(l.lastIndexOf("\r", l.length - 2) + 1).replace(/\r$/, "");
      while (/[^\x08]\x08/.test(l)) l = l.replace(/[^\x08]\x08/, "");
      return l.replace(colors ? /[\x00-\x08\x0b-\x1a\x1c-\x1f\x7f]/g : /[\x00-\x08\x0b-\x1f\x7f]/g, "");
    })
    .join("\n");
}

/** A color: one of the 16 (a class, so the theme picks it) or any other as CSS. */
type Color = number | string | null;
export type Sgr = { fg: Color; bg: Color; bold: boolean; dim: boolean; italic: boolean; under: boolean };
export const sgr = (): Sgr => ({ fg: null, bg: null, bold: false, dim: false, italic: false, under: false });

/** 38;5;n and 38;2;r;g;b (48 for the background): the color and how many numbers it took. */
function extended(codes: number[], i: number): [Color, number] {
  if (codes[i + 1] === 2) return [`rgb(${codes[i + 2] ?? 0},${codes[i + 3] ?? 0},${codes[i + 4] ?? 0})`, 4];
  if (codes[i + 1] !== 5) return [null, 0];
  const n = codes[i + 2] ?? 0;
  if (n < 16) return [n, 2];
  if (n >= 232) {
    const v = 8 + (n - 232) * 10;
    return [`rgb(${v},${v},${v})`, 2];
  }
  const c = (k: number) => (k ? 55 + k * 40 : 0);
  const m = n - 16;
  return [`rgb(${c(Math.floor(m / 36))},${c(Math.floor(m / 6) % 6)},${c(m % 6)})`, 2];
}

function apply(s: Sgr, part: string) {
  const codes = (part || "0").split(";").map(Number);
  for (let i = 0; i < codes.length; i++) {
    const c = codes[i];
    if (c === 0) Object.assign(s, sgr());
    else if (c === 1) s.bold = true;
    else if (c === 2) s.dim = true;
    else if (c === 3) s.italic = true;
    else if (c === 4) s.under = true;
    else if (c === 22) s.bold = s.dim = false;
    else if (c === 23) s.italic = false;
    else if (c === 24) s.under = false;
    else if (c >= 30 && c <= 37) s.fg = c - 30;
    else if (c >= 90 && c <= 97) s.fg = c - 82;
    else if (c === 39) s.fg = null;
    else if (c >= 40 && c <= 47) s.bg = c - 40;
    else if (c >= 100 && c <= 107) s.bg = c - 92;
    else if (c === 49) s.bg = null;
    else if (c === 38 || c === 48) {
      const [color, used] = extended(codes, i);
      s[c === 38 ? "fg" : "bg"] = color;
      i += used;
    }
  }
}

/** `clean(text, true)` as nodes: styled runs become spans (16 colors as classes, 256 and
 * RGB colors inline, backgrounds, bold, dim, italic, underline). Pass `s` to carry the
 * style from one call to the next. */
export function ansi(text, s: Sgr = sgr()) {
  const out = document.createDocumentFragment();
  text.split(/\x1b\[([0-9;]*)m/).forEach((part, i) => {
    if (i % 2) return apply(s, part);
    part = part.replace(/\x1b/g, "");
    if (!part) return;
    const cls = [
      typeof s.fg === "number" && `ansi${s.fg}`,
      typeof s.bg === "number" && `ansi-bg${s.bg}`,
      s.bold && "ansi-b",
      s.dim && "ansi-dim",
      s.italic && "ansi-i",
      s.under && "ansi-u",
    ].filter(Boolean);
    if (!cls.length && typeof s.fg !== "string" && typeof s.bg !== "string") return out.append(part);
    const span = document.createElement("span");
    span.className = cls.join(" ");
    if (typeof s.fg === "string") span.style.color = s.fg;
    if (typeof s.bg === "string") span.style.background = s.bg;
    span.textContent = part;
    out.append(span);
  });
  return out;
}

// A leading date and/or time: 2026-10-17 10:00:01(.123Z), [17-Oct-2026 10:00:01], 10:00:01 PM.
const STAMP =
  /^\[?(?:\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}|\d{2}-[A-Z][a-z]{2}-\d{4} \d{2}:\d{2}:\d{2}|\d{1,2}:\d{2}:\d{2})[^\s\]]*\]?(?: [AP]M)?/;
const LEVELS = { ERROR: "ansi1", WARN: "ansi3", WARNING: "ansi3", NOTICE: "ansi6", INFO: "ansi4" };

/** A line without escape codes, lightly: its leading time muted, level words in their colors. */
function stamped(line: string) {
  const out = document.createDocumentFragment();
  const span = (cls: string, text: string) => {
    const s = document.createElement("span");
    s.className = cls;
    s.textContent = text;
    out.append(s);
  };
  const time = STAMP.exec(line)?.[0] ?? "";
  if (time) span("term-time", time);
  let at = time.length;
  for (const m of line.slice(at).matchAll(/\b(ERROR|WARNING|WARN|NOTICE|INFO)\b/g)) {
    out.append(line.slice(at, time.length + m.index));
    span(LEVELS[m[1]], m[1]);
    at = time.length + m.index + m[1].length;
  }
  out.append(line.slice(at));
  return out;
}

/** A log as a terminal shows it (`clean(text, true)` first): lines with escape codes
 * drawn by `ansi`, plain ones by `stamped`. `s` carries colors across calls. */
export function colorize(text: string, s: Sgr = sgr()) {
  const out = document.createDocumentFragment();
  text.split("\n").forEach((line, i) => {
    if (i) out.append("\n");
    const styled = Object.values(s).some((v) => v != null && v !== false);
    out.append(styled || line.includes("\x1b") ? ansi(line, s) : stamped(line));
  });
  return out;
}
