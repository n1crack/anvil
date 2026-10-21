import { el, mode, selected, tab } from "./app";
import { termOut } from "./console";
import { icon } from "./icons";
import { shown } from "./live";
import { loadProcs, procBtn, procLoading } from "./processes";
import { listSplitter } from "./split";
import { type Sgr, clean, colorize } from "./term";

/* The pane the Dev and Containers tabs share: a toolbar, a list of entries on the left
 * (All first) and the chosen one's output live on the right, with its header, an empty
 * state, the keys hint and a splitter between them. Each tab brings its entries, how
 * they are drawn and read, and what its keys do. */

const MAX_LINES = 5000; // lines kept on screen

export function streamPane(o: {
  /** The box's extra class ("containers"), the list's and its label. */
  cls?: string;
  listCls: string;
  label: string;
  /** Where the list's width is kept, and its least. */
  widthKey: string;
  minWidth: number;
  /** Which entry each app shows, and how the tab draws and reads it. */
  sel: Map<string, string>;
  draw: (name: string) => void;
  tick: () => void;
  /** The keys other than ↑↓/j/k, for the site shown; true when one was used. */
  keys: (k: string, site: string) => boolean;
  /** The site and entry shown, and every entry's key after All. */
  current: () => { site: string; key: string } | null;
  entries: (site: string) => string[];
}) {
  const box = el("div", `dev ${o.cls ?? ""}`.trim());
  const bar = el("div", "dev-bar");
  const main = el("div", "dev-main");
  const list = el("div", o.listCls);
  list.tabIndex = 0;
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-label", o.label);
  const pane = el("section", "console dev-pane");
  const head = el("header", "console-head dev-head");
  const term = termOut("console-out dev-out");
  const empty = el("div", "dev-empty");
  term.out.tabIndex = 0;
  // The keys (see main.onkeydown), said once; a narrow pane shows a Keys button, which opens
  // them as a small card (again, outside or Esc closes it).
  const KEYS: [string, string][] = [
    ["↑↓", "select"],
    ["s", "start"],
    ["x", "stop"],
    ["r", "restart"],
    ["q", "stop all"],
    ["c", "clear"],
  ];
  const keysHint = el("p", "dev-keys");
  const keysBtn = el("button", "dev-keys-short");
  keysBtn.append(icon("keys"), "Keys");
  keysBtn.setAttribute("aria-expanded", "false");
  const card = el("div", "dev-keys-card");
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-label", "Keys");
  card.hidden = true;
  for (const [k, what] of KEYS) card.append(el("kbd", null, k), el("span", null, what));
  const toggle = (open = card.hidden) => {
    card.hidden = !open;
    keysBtn.setAttribute("aria-expanded", String(open));
  };
  keysBtn.onclick = (e) => {
    e.stopPropagation();
    toggle();
  };
  document.addEventListener("mousedown", (e) => {
    if (!card.hidden && !card.contains(e.target as Node) && e.target !== keysBtn) toggle(false);
  });
  card.addEventListener("keydown", (e) => e.key === "Escape" && toggle(false));
  keysHint.append(el("span", "dev-keys-full", KEYS.map(([k, w]) => `${k} ${w}`).join(" · ")), keysBtn, card);
  pane.append(head, term.out, empty, keysHint);
  main.append(list, listSplitter(main, o.widthKey, o.minWidth), pane);
  box.append(bar, main);

  function syncEmpty() {
    const none = !term.out.querySelector(".dev-line");
    empty.hidden = !none;
    pane.classList.toggle("is-empty", none);
  }

  function pick(name: string, key: string) {
    o.sel.set(name, key);
    o.draw(name);
    o.tick();
  }

  /* Keys, like mprocs, while the focus is in the list or the output; never while typing
   * nor with a modifier (those are the menu's). What changes on screen is the feedback. */
  main.onkeydown = (e) => {
    const v = o.current();
    if (!v || e.metaKey || e.ctrlKey || e.altKey || (e.target as HTMLElement).closest("input, textarea, select")) return;
    const k = e.key;
    if (k === "ArrowDown" || k === "ArrowUp" || k === "j" || k === "k") {
      const keys = ["all", ...o.entries(v.site)];
      const i = keys.indexOf(v.key) + (k === "ArrowDown" || k === "j" ? 1 : -1);
      if (i >= 0 && i < keys.length) pick(v.site, keys[i]);
    } else if (!o.keys(k, v.site)) return;
    e.preventDefault();
  };

  /** An entry in the list: its parts after the status; a click picks it, a right-click
   * opens `menu`. */
  function entry(name: string, key: string, parts: HTMLElement[], menu?: (it: HTMLElement) => void) {
    const it = el("div", "dev-item");
    it.setAttribute("role", "option");
    it.setAttribute("aria-selected", String(key === o.current()?.key));
    it.dataset.key = key;
    it.append(...parts);
    it.onclick = () => {
      pick(name, key);
      list.focus();
    };
    if (menu)
      it.oncontextmenu = (e) => {
        e.preventDefault();
        menu(it);
      };
    return it;
  }

  /** The tab's box in its panel; `fresh` when another app or entry is shown. */
  function attach(panel: HTMLElement, fresh: boolean) {
    if (fresh) {
      term.out.replaceChildren();
      term.atEnd = true;
    }
    if (box.parentNode !== panel) {
      panel.replaceChildren(box);
      term.restore();
    }
  }

  /** The header: a title and a line, then what `extra` adds; returns the actions' place. */
  function heading(title: string, sub: string, ...extra: HTMLElement[]) {
    const actions = el("span", "proc-actions");
    head.replaceChildren(el("span", "console-title", title), el("span", "dev-cmd", sub), ...extra, actions);
    return actions;
  }

  /** Nothing on screen yet: `text`, and ▶ when `start` is given. */
  function notice(text: string, start?: { name: string; run: () => void }) {
    empty.replaceChildren(el("span", null, text));
    if (start) {
      const go = procBtn(icon("start"), `Start ${start.name}`, start.run);
      go.title = `Start ${start.name}`;
      empty.append(go);
    }
    syncEmpty();
  }

  /** New lines at the end: the oldest past the cap go, and the view follows. */
  function push(lines: Node) {
    term.out.append(lines);
    term.keep(() => trimLines(term.out, MAX_LINES));
    term.follow();
    syncEmpty();
  }

  /** The tab shown: drawn, read, the list focused, and the reader's place put back. */
  function shownTab() {
    if (!selected) return;
    o.draw(selected);
    o.tick();
    if (!main.hidden && box.isConnected) list.focus({ preventScroll: true });
    // WebKit forgets a hidden pane's scroll position: put it back once it is laid out.
    requestAnimationFrame(term.restore);
  }

  /** Only while `tabName` is shown: read every `ms`, the processes every `procsEvery` reads.
   * `each` hears every beat, shown or not. */
  function poll(tabName: string, ms: number, procsEvery: number, each?: (visible: boolean) => void) {
    let ticks = 0;
    setInterval(() => {
      const visible = shown() && mode === "apps" && !!selected && tab === tabName;
      each?.(visible);
      if (!visible || !selected) return;
      if (o.current()?.site === selected) o.tick();
      if (++ticks % procsEvery === 0 && !procLoading.has(selected)) loadProcs(selected);
    }, ms);
  }

  return { box, bar, main, list, head, term, empty, syncEmpty, pick, entry, attach, heading, notice, push, shownTab, poll };
}

/** One line of output, its style carried on in `s`; in All (`width` ≥ 0), tagged with its
 * entry's name in its color. */
export function line(x: { key: string; name: string; color: string }, text: string, s: Sgr, width: number) {
  const d = el("div", "dev-line");
  d.dataset.key = x.key;
  // The text in its own column, so a wrapped line stays right of the tag.
  const body = el("span", "dev-text");
  body.append(colorize(clean(text, true), s));
  if (width >= 0) d.append(el("span", `dev-tag ${x.color}`.trim(), `${x.name.padEnd(width)} │ `));
  d.append(body);
  return d;
}

/** The oldest lines over `max` out in one removal, so the pane changes once, not once a line. */
export function trimLines(out: HTMLElement, max: number) {
  const over = out.childElementCount - max;
  if (over <= 0) return;
  const r = document.createRange();
  r.setStartBefore(out.firstElementChild!);
  r.setEndBefore(out.children[over]);
  r.deleteContents();
}
