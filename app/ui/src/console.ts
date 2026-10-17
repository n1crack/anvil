import { el, invoke } from "./app";

/* Reading a process's or command's output (`log_text`) and drawing it as a terminal
 * shows it; shared by the Dev tab and the Logs tab. */

export type Tail = { site: string; source: string; from: number; raw: string; busy: boolean; [k: string]: any };

export const tail = (site: string, source: string, extra = {}): Tail => ({ site, source, from: 0, raw: "", busy: false, ...extra });

/** New output since the last look, also added to `t.raw`; null when there is none. */
export async function readTail(t: Tail): Promise<string | null> {
  if (t.busy) return null;
  t.busy = true;
  let page;
  try {
    page = await invoke("log_text", { site: t.site, source: t.source, from: t.from });
  } catch {
    return null; // no output yet
  } finally {
    t.busy = false;
  }
  // Cleared (a restart, Clear): start over, and say so for views that append.
  if (page.end_offset < t.from) [t.raw, t.cleared] = ["", true];
  t.from = page.end_offset;
  if (!page.text) return null;
  t.raw += page.text;
  // ponytail: keeps the newest ~512 KB and redraws all of it; a ring of lines if that gets slow.
  if (t.raw.length > 524288) t.raw = t.raw.slice(t.raw.indexOf("\n", t.raw.length - 262144) + 1);
  return page.text;
}

/** A terminal's <pre>: it follows new output only while the reader is at the bottom,
 * and comes back where the reader was after a redraw detached it (`restore`). */
export function termOut(cls: string) {
  const out = el("pre", cls);
  const t = {
    out,
    atEnd: true,
    top: 0,
    restore: () => (out.scrollTop = t.atEnd ? out.scrollHeight : t.top),
    follow: () => t.atEnd && (out.scrollTop = out.scrollHeight),
  };
  out.onscroll = () => {
    if (!out.clientHeight) return; // detached by a redraw, not scrolled
    t.top = out.scrollTop;
    t.atEnd = t.top + out.clientHeight >= out.scrollHeight - 4;
  };
  // Output that arrives before the pane has its height (a tab still opening) would
  // stay at the top: once it gets its size, or changes it, keep following the end.
  new ResizeObserver(() => t.atEnd && out.clientHeight && (out.scrollTop = out.scrollHeight)).observe(out);
  return t;
}
