// Not from app.ts: the Processes window uses `follower` too, and app.ts is the main window.
const el = (tag: string, cls: string): any => Object.assign(document.createElement(tag), { className: cls });
const invoke = (cmd: string, args: Record<string, unknown>): Promise<any> => window.__TAURI__.core.invoke(cmd, args);

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

export const EDGE = 4; // px from the newest edge that still count as at it

export type Follower = ReturnType<typeof follower>;

/** A growing view that follows its newest edge (the end; the top with `newestFirst`)
 * only while the reader is there. Scrolling away even a little pauses it at once (the
 * wheel says so before the scroll does); new output then shows a pill that jumps back,
 * as does scrolling back to the edge. Only the reader's scrolls change that: the ones
 * made here are told apart by where they left the view. Every log view uses one. */
export function follower(out: HTMLElement, newestFirst = false) {
  const pill = el("button", "follow-pill");
  pill.type = "button";
  // `keep` holds the reader's place itself; WebKit's scroll anchoring would move it twice.
  out.style.setProperty("overflow-anchor", "none");
  pill.dataset.label = newestFirst ? "↑ New entries" : "↓ New output";
  pill.setAttribute("aria-label", newestFirst ? "New entries: show the newest" : "New output: jump to the end");
  let mine = -1; // scrollTop as last set here
  let carry = 0; // the part of a px `keep` could not scroll
  const edge = () => (newestFirst ? 0 : out.scrollHeight);
  const atEdge = () => (newestFirst ? out.scrollTop <= EDGE : out.scrollTop + out.clientHeight >= out.scrollHeight - EDGE);
  const set = (top: number) => {
    out.scrollTop = top;
    mine = out.scrollTop;
    t.top = mine;
  };
  const t = {
    out: out as any, // as el() gave it before
    atEnd: true, // following
    top: 0, // where the reader left it
    /** Back where the reader was, after a redraw or a re-attach lost it. */
    restore: () => out.clientHeight && set(t.atEnd ? edge() : t.top),
    /** After new output: to the newest edge while following, else the pill. */
    follow: () => {
      if (t.atEnd) return set(edge());
      if (newestFirst) out.prepend(pill);
      else out.append(pill);
    },
    /** Content added or removed before what the reader sees (older lines trimmed, newer
     * rows put on top): paused, those lines stay where they were. */
    keep: (change: () => void) => {
      if (t.atEnd) return change();
      // The last line stays: how far it moved is what was added or removed, in exact
      // (fractional) px, so ticks of trimming add up to no drift.
      const last = out.lastElementChild === pill ? pill.previousElementSibling : out.lastElementChild;
      const y = last?.getBoundingClientRect().top;
      const h = out.scrollHeight;
      change();
      const moved = last?.isConnected ? last.getBoundingClientRect().top - y! : out.scrollHeight - h;
      // scrollTop takes whole px: the rest is carried to the next time.
      const want = out.scrollTop + moved + carry;
      set(want);
      carry = want - out.scrollTop;
    },
    /** Follow again, from the newest edge. */
    end: () => {
      t.atEnd = true;
      pill.remove();
      set(edge());
    },
  };
  pill.onclick = (e) => {
    e.stopPropagation();
    t.end();
  };
  out.addEventListener("scroll", () => {
    if (!out.clientHeight) return; // detached by a redraw, not scrolled
    if (Math.abs(out.scrollTop - mine) < 1) return; // a scroll made here
    mine = -1;
    carry = 0;
    t.top = out.scrollTop;
    t.atEnd = atEdge();
    if (t.atEnd) pill.remove();
  });
  out.addEventListener(
    "wheel",
    (e) => {
      if ((newestFirst ? e.deltaY > 0 : e.deltaY < 0) && out.scrollHeight > out.clientHeight + EDGE) t.atEnd = false;
    },
    { passive: true },
  );
  // Output that arrives before the pane has its height (a tab still opening) would
  // stay at the top: once it gets its size, or changes it, keep following the edge.
  new ResizeObserver(() => t.atEnd && out.clientHeight && set(edge())).observe(out);
  return t;
}

/** A terminal's <pre>, following its end (see `follower`). */
export const termOut = (cls: string) => follower(el("pre", cls));
