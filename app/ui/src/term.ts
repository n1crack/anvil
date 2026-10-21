/* Terminal output, shared by the main window's console and the Processes window. */
import { keys } from "./platform";

/** Terminal output as a terminal would leave it: progress bars redraw a line with \r,
 * \b or cursor codes (composer's and npm's ESC[1G ESC[2K); other escape codes don't
 * belong in plain text. With `colors`, SGR codes (colors, bold) stay for `ansi` to draw. */
export function clean(text, colors = false) {
  const rows: (string | Row)[] = [];
  let r = 0; // the row the cursor is on
  for (const l of text.replace(colors ? STRIP_KEEP_SGR : STRIP, "").split("\n")) {
    if (r === rows.length && !CURSOR.test(l)) rows.push(l);
    else r = draw(rows, r, l);
    r++;
  }
  return rows
    .map((row) => (typeof row === "string" ? row : row.cells.map((c) => c[0] + c[1]).join("") + row.tail))
    .map((l) => l.replace(colors ? /[\x00-\x08\x0b-\x1a\x1c-\x1f\x7f]/g : /[\x00-\x08\x0b-\x1f\x7f]/g, ""))
    .join("\n");
}

// Escape codes dropped: all but the cursor moves `draw` follows (and SGR, with colors).
const STRIP = /\x1b\[(?![0-9]*[GKCDA])[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)?/g;
const STRIP_KEEP_SGR = /\x1b\[(?![0-9;]*m|[0-9]*[GKCDA])[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)?/g;
const CURSOR = /[\r\b]|\x1b\[[0-9]*[GKCDA]/;

/** A drawn row: each column's color codes and character, and the codes after the last. */
type Row = { cells: [string, string][]; tail: string };

function toRow(row: string | Row | undefined): Row {
  if (row && typeof row !== "string") return row;
  const out: Row = { cells: [], tail: "" };
  for (const [t] of (row ?? "").matchAll(/\x1b\[[0-9;]*m|[\s\S]/gu)) {
    if (t.length > 1 && t[0] === "\x1b") out.tail += t;
    else {
      out.cells.push([out.tail, t]);
      out.tail = "";
    }
  }
  return out;
}

/** Only the codes after the last reset still matter: redraws don't pile them up. */
const sinceReset = (codes: string) => codes.slice([...codes.matchAll(/\x1b\[0?m/g)].at(-1)?.index ?? 0);

/** One line written onto `rows` from row `r` as a terminal would: \r starts the line
 * over (once something is written), \b and CSI n D move back, CSI n C forward, CSI n G
 * to a column, CSI K / 1K / 2K erase, CSI n A goes up (not above the first row there
 * is). Color codes take no column; an erased cell's codes go to the next character, so
 * no reset is lost: `0%\e[0m\b\b1%` is `1%`. Returns the row it ends on.
 * ponytail: one column per code point; wide characters and wrapping aren't measured. */
function draw(rows: (string | Row)[], r: number, l: string) {
  let row = (rows[r] = toRow(rows[r]));
  let at = 0;
  let fresh = false; // after \r: the next character starts the line over
  let codes = row.tail; // color codes waiting for the next character
  row.tail = "";
  const lose = (from: number, to: number, blank: boolean) => {
    const cut = row.cells.slice(from, to);
    codes = cut.map((c) => c[0]).join("") + codes;
    if (blank) for (let i = from; i < Math.min(to, row.cells.length); i++) row.cells[i] = ["", " "];
    else row.cells.splice(from, to - from);
  };
  for (const [t, arg, op] of l.matchAll(/\x1b\[([0-9;]*)([mGKCDA])|[\s\S]/gu)) {
    const n = Number(arg) || 1;
    if (op === "m") {
      codes += t;
      continue;
    }
    if (t === "\r") {
      [at, fresh] = [0, true];
      continue;
    }
    if (!op && t !== "\b" && fresh) lose(0, row.cells.length, false);
    fresh = false;
    if (t === "\b" || op === "D") at = Math.max(0, at - (op ? n : 1));
    else if (op === "C") at += n;
    else if (op === "G") at = n - 1;
    else if (op === "K") lose(arg === "1" || arg === "2" ? 0 : at, arg === "1" ? at + 1 : row.cells.length, arg === "1");
    else if (op === "A") {
      r = Math.max(0, r - n);
      row = rows[r] = toRow(rows[r]);
      codes = row.tail + codes;
      row.tail = "";
    } else {
      while (row.cells.length < at) row.cells.push(["", " "]);
      row.cells[at] = [sinceReset((row.cells[at]?.[0] ?? "") + codes), t];
      at++;
      codes = "";
    }
  }
  row.tail = sinceReset(codes);
  return r;
}

/** A color: one of the 16 (a class, so the theme picks it) or any other as CSS. */
type Color = number | string | null;
export type Sgr = { fg: Color; bg: Color; bold: boolean; dim: boolean; italic: boolean; under: boolean; inverse: boolean };
export const sgr = (): Sgr => ({ fg: null, bg: null, bold: false, dim: false, italic: false, under: false, inverse: false });

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
    else if (c === 7) s.inverse = true;
    else if (c === 27) s.inverse = false;
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
    // Inverse swaps them; the defaults are the surface's light text on its dark ground.
    const [fg, bg] = s.inverse ? [s.bg ?? 0, s.fg ?? 7] : [s.fg, s.bg];
    const cls = [
      typeof fg === "number" && `ansi${fg}`,
      typeof bg === "number" && `ansi-bg${bg}`,
      s.bold && "ansi-b",
      s.dim && "ansi-dim",
      s.italic && "ansi-i",
      s.under && "ansi-u",
    ].filter(Boolean);
    if (!cls.length && typeof fg !== "string" && typeof bg !== "string") return out.append(part);
    const span = document.createElement("span");
    span.className = cls.join(" ");
    if (typeof fg === "string") span.style.color = fg;
    if (typeof bg === "string") span.style.background = bg;
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
 * drawn by `ansi`, plain ones by `stamped`. `s` carries colors across calls. Links are
 * found line by line: DOM ranges get slower the more siblings their parent has. */
export function colorize(text: string, s: Sgr = sgr()) {
  const out = document.createDocumentFragment();
  text.split("\n").forEach((line, i) => {
    if (i) out.append("\n");
    const styled = Object.values(s).some((v) => v != null && v !== false);
    out.append(linkify(styled || line.includes("\x1b") ? ansi(line, s) : stamped(line)));
  });
  return out;
}

// http(s) up to a space or quote, without the punctuation that ends a sentence.
const URL_RE = /\bhttps?:\/\/[^\s<>"'`]*[^\s<>"'`.,;:!?)\]}]/g;

// One range for every call: WebKit keeps each live range up to date on every DOM change
// until it is collected, so a range per call (per line) made all changes slower.
let linkRange: Range | undefined;

/** Drawn output with its URLs as links, also when colors split one (vite's bold port in
 * http://localhost:5173/). A link opens on ⌘-click (see below); a plain click selects. */
export function linkify<T extends Node>(root: T): T {
  const text = root.textContent ?? "";
  if (!text.includes("http")) return root;
  // The text nodes and where each starts, read once. Links go in from the last, so the
  // nodes before each one keep their place: a node split by a link keeps its first part.
  const texts: Text[] = [];
  const starts: number[] = [];
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = 0; walk.nextNode(); n += (walk.currentNode as Text).length) {
    texts.push(walk.currentNode as Text);
    starts.push(n);
  }
  let i = texts.length - 1; // only moves back, so all the links cost one pass
  const at = (pos: number, end: boolean): [Text, number] => {
    while (i > 0 && (end ? starts[i] >= pos : starts[i] > pos)) i--;
    return [texts[i], pos - starts[i]];
  };
  const range = (linkRange ??= document.createRange());
  for (const m of [...text.matchAll(URL_RE)].reverse()) {
    range.setEnd(...at(m.index + m[0].length, true));
    range.setStart(...at(m.index, false));
    const a = document.createElement("span");
    a.className = "term-link";
    a.dataset.url = m[0];
    a.title = keys("⌘-click to open");
    a.append(range.extractContents());
    range.insertNode(a);
  }
  return root;
}

// ⌘ (Ctrl elsewhere) held: links underline on hover; ⌘-click opens one through open_url,
// which refuses anything but https and local http; its reason shows as the link's tooltip.
const held = (e: KeyboardEvent | MouseEvent) => document.documentElement.classList.toggle("link-mod", e.metaKey || e.ctrlKey);
addEventListener("keydown", held);
addEventListener("keyup", held);
addEventListener("mouseover", held);
addEventListener("blur", () => document.documentElement.classList.remove("link-mod"));
addEventListener(
  "click",
  (e) => {
    const a = (e.target as Element | null)?.closest?.(".term-link") as HTMLElement | null;
    if (!a || !(e.metaKey || e.ctrlKey)) return;
    e.preventDefault();
    e.stopPropagation();
    (window as any).__TAURI__.core.invoke("open_url", { url: a.dataset.url }).catch((err) => (a.title = String(err)));
  },
  true,
);
