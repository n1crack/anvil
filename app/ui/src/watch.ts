import { $, call, el, mode, selected, showTab, tab } from "./app";
import { type Tail, readTail, tail, termOut } from "./console";
import { late } from "./late";
import { loadProcs, procBtn, procCache, procLoading } from "./processes";
import { ansi, clean } from "./term";

/* Watch tab: the app's processes on the left, the chosen one's live output on the right,
 * as `php artisan dev` shows them. "All" (the default) merges every process's output,
 * each line tagged with its process's name in its own color. */

export const watchSel = new Map<string, string>(); // site name -> "all", "app" or "proc:<id>"
const COLORS = [4, 5, 9, 3, 6, 2, 13, 11]; // ansi color classes, one per process in list order
const MAX_LINES = 2000; // merged lines kept in All

/** The app's own server when it is a process, then every other process row. */
const entries = (name: string) =>
  (procCache.get(name)?.rows ?? [])
    .filter((r) => r.kind !== "site")
    .map((r, i) => ({ key: r.main ? "app" : `proc:${r.id}`, row: r, color: COLORS[i % COLORS.length] }));
const stateOf = (r) => (r.running ? "on" : r.crashed ? "bad" : "");

const box = el("div", "watch");
const list = el("div", "group procs-list watch-list");
list.tabIndex = 0;
list.setAttribute("role", "listbox");
list.setAttribute("aria-label", "Processes");
const pane = el("section", "console watch-pane");
const head = el("header", "console-head watch-head");
const term = termOut("console-out watch-out");
pane.append(head, term.out);
box.append(list, pane);

// The output shown: which app and entry, each source's reader and, for All, its unfinished last line.
let view: { site: string; key: string; tails: Map<string, Tail>; pending: Map<string, string> } | null = null;

function pick(name: string, key: string) {
  watchSel.set(name, key);
  drawWatch(name);
  watchTick();
}

list.onkeydown = (e) => {
  if ((e.key !== "ArrowDown" && e.key !== "ArrowUp") || !view) return;
  e.preventDefault();
  const keys = ["all", ...entries(view.site).map((x) => x.key)];
  const i = keys.indexOf(view.key) + (e.key === "ArrowDown" ? 1 : -1);
  if (i >= 0 && i < keys.length) pick(view.site, keys[i]);
};

function item(name: string, key: string, label: string, dot: string) {
  const it = el("div", "watch-item");
  it.setAttribute("role", "option");
  it.setAttribute("aria-selected", String(key === view?.key));
  it.dataset.key = key;
  it.append(el("span", `dot ${dot}`), el("span", "watch-name", label));
  it.onclick = () => {
    pick(name, key);
    list.focus();
  };
  return it;
}

/** Called whenever the Overview's processes are drawn, and when the tab is shown. */
export function drawWatch(name: string) {
  const root = $("#site");
  const panel = $('[data-panel="watch"]', root);
  if (!panel) return;
  const all = entries(name);
  // Like Git: no tab for an app without processes, once that is known.
  $('.tabs [data-tab="watch"]', root).hidden = !all.length;
  if (procCache.has(name) && !all.length && tab === "watch") showTab(root, "overview");
  if (!all.length) return panel.replaceChildren();

  let key = watchSel.get(name) ?? "all";
  if (key !== "all" && !all.some((x) => x.key === key)) key = "all";
  if (view?.site !== name || view.key !== key) {
    view = { site: name, key, tails: new Map(), pending: new Map() };
    term.out.replaceChildren();
    term.atEnd = true;
  }
  if (box.parentNode !== panel) {
    panel.replaceChildren(box);
    term.restore();
  }

  const running = all.filter((x) => x.row.running).length;
  const crashed = all.some((x) => x.row.crashed && !x.row.running);
  list.replaceChildren(
    item(name, "all", "All", crashed ? "bad" : running ? "on" : ""),
    ...all.map((x) => item(name, x.key, x.row.name, stateOf(x.row))),
  );

  head.replaceChildren();
  const actions = el("span", "proc-actions");
  const one = all.find((x) => x.key === key);
  if (!one) {
    head.append(el("span", "console-title", "All"), el("span", "watch-cmd", `${running} of ${all.length} running`));
    // As the Overview's header: name null runs every process and the app's own server.
    const run = (action) => loadProcs(name, "process_run", { name: null, action });
    if (running < all.length) actions.append(procBtn("▶ Start all", "Start all processes", () => run("start")));
    if (running) actions.append(procBtn("■ Stop all", "Stop all processes", () => run("stop")));
  } else {
    const r = one.row;
    head.append(el("span", "console-title", r.name), el("span", "watch-cmd", r.command));
    const run = r.main
      ? (action) => call("app_run", { site: name, action })
      : (action) => loadProcs(name, "process_run", { name: r.id, action });
    if (r.running || r.crashed) {
      actions.append(procBtn("Restart", `Restart ${r.name}`, () => run("restart")));
      actions.append(procBtn("■", r.running ? `Stop ${r.name}` : `Dismiss ${r.name}`, () => run("stop")));
    } else actions.append(procBtn("▶ Start", `Start ${r.name}`, () => run("start")));
  }
  head.append(actions);
  term.out.dataset.empty = (one ? one.row.running : running) ? "No output yet" : "Not running";
}

/** New output for what is shown: one process redrawn whole (so \r and colors come out
 * right), or All's new complete lines appended in the order they arrive. */
export async function watchTick() {
  const v = view;
  if (!v) return;
  const all = entries(v.site);
  const shown = v.key === "all" ? all : all.filter((x) => x.key === v.key);
  const width = Math.max(...shown.map((x) => x.row.name.length));
  await Promise.all(
    shown.map(async (x) => {
      if (!v.tails.has(x.key)) v.tails.set(x.key, tail(v.site, x.key));
      const t = v.tails.get(x.key)!;
      const text = await readTail(t);
      if (!text || v !== view) return;
      if (v.key !== "all") {
        term.out.replaceChildren(ansi(clean(t.raw, true)));
        return term.follow();
      }
      // ponytail: a line shows once it ends; a progress bar without \n waits for its last redraw.
      const lines = ((v.pending.get(x.key) ?? "") + text).split("\n");
      v.pending.set(x.key, lines.pop()!);
      for (const l of lines) {
        const line = el("div", "watch-line");
        line.append(el("span", `watch-tag ansi${x.color}`, `${x.row.name.padEnd(width)} │ `), ansi(clean(l, true)));
        term.out.append(line);
      }
      while (term.out.childElementCount > MAX_LINES) term.out.firstElementChild.remove();
      term.follow();
    }),
  );
}

/** Open the tab on one process (a row's Watch button). */
export function openWatch(name: string, key: string) {
  watchSel.set(name, key);
  showTab($("#site"), "watch");
}

late.drawWatch = drawWatch;
late.watchTab = () => {
  if (!selected) return;
  drawWatch(selected);
  watchTick();
};
late.openWatch = openWatch;

// Only while the tab is shown: output every second, the processes' states every two.
let ticks = 0;
setInterval(() => {
  if (document.hidden || mode !== "apps" || !selected || tab !== "watch") return;
  if (view?.site === selected) watchTick();
  if (++ticks % 2 === 0 && !procLoading.has(selected)) loadProcs(selected);
}, 1000);
