// @ts-nocheck

/* Terminal output, shared by the main window's console and the Processes window. */

/** Terminal output as a terminal would leave it: progress bars redraw a line with \r
 * or \b; escape codes don't belong in plain text. With `colors`, SGR codes (colors,
 * bold) stay for `ansi` to draw. */
export function clean(text, colors = false) {
  return text
    .replace(colors ? /\x1b\[(?![0-9;]*m)[0-9;?]*[ -\/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)?/g : /\x1b\[[0-9;?]*[ -\/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)?/g, "")
    .split("\n")
    .map((l) => {
      l = l.slice(l.lastIndexOf("\r", l.length - 2) + 1).replace(/\r$/, "");
      while (/[^\x08]\x08/.test(l)) l = l.replace(/[^\x08]\x08/, "");
      return l.replace(colors ? /[\x00-\x08\x0b-\x1a\x1c-\x1f\x7f]/g : /[\x00-\x08\x0b-\x1f\x7f]/g, "");
    })
    .join("\n");
}

/** `clean(text, true)` as nodes: colored and bold runs become spans (30–37, 90–97, 1). */
export function ansi(text) {
  const out = document.createDocumentFragment();
  let fg = null;
  let bold = false;
  text.split(/\x1b\[([0-9;]*)m/).forEach((part, i) => {
    if (i % 2) {
      for (const c of (part || "0").split(";").map(Number)) {
        if (c === 0) [fg, bold] = [null, false];
        else if (c === 1) bold = true;
        else if (c === 22) bold = false;
        else if (c === 39) fg = null;
        else if (c >= 30 && c <= 37) fg = c - 30;
        else if (c >= 90 && c <= 97) fg = c - 82;
        else if (c === 38 || c === 48) break; // 256 or RGB colors: their numbers aren't codes
      }
      return;
    }
    part = part.replace(/\x1b/g, "");
    if (!part) return;
    if (fg == null && !bold) return out.append(part);
    const span = document.createElement("span");
    span.className = [fg != null && `ansi${fg}`, bold && "ansi-b"].filter(Boolean).join(" ");
    span.textContent = part;
    out.append(span);
  });
  return out;
}
