import type { CommandChip, ProcessRow } from "./types";
import { $, M, PHP_KINDS, PROCESS_KINDS, ask, call, el, invoke, mode, popAt, selected, showError, showTab, snap, tab } from "./app";
import { type Tail, readTail, tail, termOut } from "./console";
import { late } from "./late";
import { askProcs, confirmPop, devAll, devRows, drawProcs, loadProcs, procBtn, procCache, procLoading } from "./processes";
import { shown } from "./live";
import { type Sgr, clean, colorize, sgr } from "./term";

/* Dev tab: the app's dev processes on the left, the chosen one's output live on the right,
 * as `php artisan dev` shows them in a terminal. "All" (the default) merges every
 * process's output, each line tagged with its name in its color. Run ▾ runs a one-off
 * script, listed below the processes until dismissed; Configure edits the processes. */

export const devSel = new Map<string, string>(); // site name -> "all", "app", "proc:<id>" or "once:<id>"
export const COLORS = 6; // name colors pc0…pc5 (blue, purple, pink, orange, green, cyan), as concurrently gives them
const MAX_LINES = 5000; // lines kept on screen

type Item = { key: string; name: string; color: string; running: boolean; crashed: boolean; row?: ProcessRow; run?: CommandChip };

/** The app's own server when it is a process, then every other process, each in its color. */
const procItems = (name: string): Item[] =>
  devRows(procCache.get(name)).map((r, i) => ({
    key: r.main ? "app" : `proc:${r.id}`,
    name: r.name,
    color: `pc${i % COLORS}`,
    running: r.running,
    crashed: !!r.crashed && !r.running,
    row: r,
  }));
/** Scripts run from Run ▾ that are running or ended and not yet dismissed. */
const runItems = (name: string): Item[] =>
  (procCache.get(name)?.commands ?? [])
    .filter((c) => c.running || c.exit)
    .map((c) => ({ key: `once:${c.id}`, name: c.name, color: "", running: c.running, crashed: !c.running && c.exit?.code !== 0, run: c }));
const scriptLabel = (c: CommandChip) => (c.source === "composer.json" ? `composer ${c.name}` : `npm run ${c.name}`);
const exitText = (c: CommandChip) => (c.exit?.code == null ? "Ended by a signal" : `Exited with code ${c.exit.code}`);

const box = el("div", "dev");
const bar = el("div", "dev-bar");
const main = el("div", "dev-main");
const list = el("div", "group procs-list dev-list");
list.tabIndex = 0;
list.setAttribute("role", "listbox");
list.setAttribute("aria-label", "Processes");
const pane = el("section", "console dev-pane");
const head = el("header", "console-head dev-head");
const term = termOut("console-out dev-out");
const empty = el("div", "dev-empty");
term.out.tabIndex = 0;
// The keys (see main.onkeydown), said once; hidden when the pane is too narrow for it.
const keysHint = el("p", "dev-keys", "↑↓ select · s start · x stop · r restart · q stop all · c clear");
pane.append(head, term.out, empty, keysHint);
main.append(list, pane);
const conf = el("div", "dev-conf");
box.append(bar, main, conf);

// What is shown: which app and entry, each source's reader, its unfinished last line and
// the style it ends in, whether each entry ran when last drawn, and the live last line.
type View = {
  site: string;
  key: string;
  tails: Map<string, Tail>;
  pending: Map<string, string>;
  styles: Map<string, Sgr>;
  ran: Map<string, boolean>;
  live: HTMLElement | null;
};
let view: View | null = null;
let configuring: string | null = null; // the site whose processes are being configured
let draft: { name: string; command: string } | null = null; // a process being added
let confError = "";

function syncEmpty() {
  const none = !term.out.querySelector(".dev-line");
  empty.hidden = !none;
  pane.classList.toggle("is-empty", none);
}

/** An entry's output began again (a restart, Clear): its lines go, and so does its reader. */
function forget(v: View, key: string) {
  v.tails.delete(key);
  v.pending.delete(key);
  v.styles.delete(key);
  if (v.live?.dataset.key === key) v.live = null;
  for (const l of term.out.querySelectorAll(`[data-key="${key}"]`)) l.remove();
  syncEmpty();
}

function pick(name: string, key: string) {
  devSel.set(name, key);
  drawDev(name);
  devTick();
}

/** Dev start or stop (the toolbar's Start/Stop), when it is offered. */
function headerAll(action: "start" | "stop") {
  if (devAll.site === view?.site && devAll[action] && !devAll.busy) devAll.run(action);
}

/* Keys, like mprocs, while the focus is in the list or the output; never while typing
 * nor with a modifier (those are the menu's). What changes on screen is the feedback. */
main.onkeydown = (e) => {
  const v = view;
  if (!v || e.metaKey || e.ctrlKey || e.altKey || (e.target as HTMLElement).closest("input, textarea, select")) return;
  const name = v.site;
  const procs = procItems(name);
  const items = [...procs, ...runItems(name)];
  const one = items.find((x) => x.key === v.key);
  const k = e.key;
  if (k === "ArrowDown" || k === "ArrowUp" || k === "j" || k === "k") {
    const keys = ["all", ...items.map((x) => x.key)];
    const i = keys.indexOf(v.key) + (k === "ArrowDown" || k === "j" ? 1 : -1);
    if (i >= 0 && i < keys.length) pick(name, keys[i]);
  } else if (k === "s") {
    if (!one) headerAll("start");
    else if (one.row && !one.running) runProc(name, one, "start");
  } else if (k === "x") {
    if (!one) headerAll("stop");
    else if (one.row && one.running) runProc(name, one, "stop");
  } else if (k === "r") {
    for (const x of one ? [one] : procs.filter((p) => p.running)) if (x.row && (x.running || x.crashed)) runProc(name, x, "restart");
  } else if (k === "q") headerAll("stop");
  else if (k === "c") {
    if (one) clearOutput(name, one);
    else {
      // All: the screen only; each process's own output stays.
      term.out.replaceChildren();
      v.live = null;
      syncEmpty();
    }
  } else return;
  e.preventDefault();
};

/** Start, stop or restart a process; a restart starts its output over. */
async function runProc(name: string, x: Item, action: string) {
  const r = x.row!;
  if (r.main) {
    await call("app_run", { site: name, action });
    await loadProcs(name);
  } else await loadProcs(name, "process_run", { name: r.id, action });
  if (action === "restart" && view?.site === name) forget(view, x.key);
}

async function runScript(name: string, c: CommandChip) {
  if (await loadProcs(name, "command_run", { id: c.id })) pick(name, `once:${c.id}`);
}

const dismiss = (name: string, c: CommandChip) =>
  invoke("once_stop", { site: name, id: c.id }).then(
    () => loadProcs(name),
    (e) => showError(String(e)),
  );

async function clearOutput(name: string, x: Item) {
  // A process that never wrote has no log to empty.
  await invoke("log_clear", { site: name, source: x.key }).catch(() => {});
  if (view?.site === name) forget(view, x.key);
}

async function editAddress(name: string, r: ProcessRow) {
  const host = new URL(r.address!).hostname;
  const rest = host.slice(host.indexOf("."));
  const first = await ask(`Address of ${r.name}`, host.slice(0, host.indexOf(".")), (v) => `${v.trim() || "…"}${rest}`);
  if (typeof first !== "string") return;
  const v = first.trim();
  await loadProcs(name, "set_process", { id: r.id, ...(v ? { host: v } : { resetHost: true }) });
}

/** Right-click on an entry: what can be done to it. */
async function itemMenu(name: string, x: Item, at: HTMLElement) {
  const item = (text, action) => M.MenuItem.new({ text, action });
  const items: Promise<any>[] = [];
  if (x.run) {
    const c = x.run;
    if (c.running) items.push(item("Stop", () => dismiss(name, c)));
    else items.push(item("Run Again…", () => confirmPop(at, `Run ${scriptLabel(c)}?`, "Run", () => runScript(name, c))));
    items.push(item("Clear", () => clearOutput(name, x)));
    if (!c.running) items.push(item("Dismiss", () => dismiss(name, c)));
  } else {
    const r = x.row!;
    if (r.running)
      items.push(
        item("Stop", () => runProc(name, x, "stop")),
        item("Restart", () => runProc(name, x, "restart")),
      );
    else items.push(item("Start", () => runProc(name, x, "start")));
    const web = r.web ?? r.address;
    if (web) items.push(item("Open in Browser", () => invoke("open_url", { url: web }).catch((e) => showError(String(e)))));
    // An extra dev server's own address, <first part>.<app>.<tld>: the first part is editable.
    if (r.address && !r.main) items.push(item("Edit Address…", () => editAddress(name, r)));
    items.push(
      M.PredefinedMenuItem.new({ item: "Separator" }),
      item("Clear", () => clearOutput(name, x)),
    );
  }
  (await M.Menu.new({ items: await Promise.all(items) })).popup();
}

function entry(name: string, x: Item) {
  const it = el("div", "dev-item");
  it.setAttribute("role", "option");
  it.setAttribute("aria-selected", String(x.key === view?.key));
  it.dataset.key = x.key;
  // The status column: a process's dot; a run's spinner, then ✓ or ✕.
  let status: HTMLElement;
  if (!x.run) status = el("span", `dot ${x.running ? "on" : x.crashed ? "bad" : ""}`);
  else if (x.running) {
    status = el("span", "dev-status");
    status.append(el("span", "spin-dot"));
    status.title = "Running";
  } else {
    const ok = x.run.exit?.code === 0;
    status = el("span", `dev-status ${ok ? "ok" : "bad"}`, ok ? "✓" : "✕");
    status.title = ok ? "Succeeded" : exitText(x.run);
  }
  const label =
    x.run?.source === "composer.json" && runItems(name).some((y) => y.key !== x.key && y.name === x.name) ? `composer ${x.name}` : x.name;
  it.append(status, el("span", `dev-name ${x.color}`.trim(), label));
  if (x.run) {
    const c = x.run;
    const x2 = procBtn("×", c.running ? `Stop and dismiss ${c.name}` : `Dismiss ${c.name}`, () => dismiss(name, c));
    x2.className = "dev-dismiss";
    x2.title = c.running ? "Stop and dismiss" : "Dismiss";
    it.append(x2);
  }
  it.onclick = () => {
    pick(name, x.key);
    list.focus();
  };
  if (x.key !== "all")
    it.oncontextmenu = (e) => {
      e.preventDefault();
      itemMenu(name, x, it);
    };
  return it;
}

/** Run ▾: the app's one-off scripts, each with its script muted; choosing one asks first. */
function runMenu(name: string, btn: HTMLElement) {
  const cmds = procCache.get(name)?.commands ?? [];
  const pop = el("div", "dev-run-menu");
  pop.setAttribute("role", "menu");
  const sources = [...new Set(cmds.map((c) => c.source))];
  for (const src of sources) {
    if (sources.length > 1) pop.append(el("div", "dev-run-group", src));
    for (const c of cmds.filter((c) => c.source === src)) {
      const b = el("button", "dev-run-item");
      b.setAttribute("role", "menuitem");
      b.append(el("span", "dev-run-name", c.name), el("span", "dev-run-cmd", c.command));
      b.title = c.command;
      b.onclick = () => {
        close();
        confirmPop(btn, `Run ${scriptLabel(c)}?`, "Run", () => runScript(name, c));
      };
      pop.append(b);
    }
  }
  pop.addEventListener("keydown", (e) => {
    const items = [...pop.querySelectorAll("button")] as HTMLElement[];
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown") items[Math.min(i + 1, items.length - 1)]?.focus();
    else if (e.key === "ArrowUp") items[Math.max(i - 1, 0)]?.focus();
    else return;
    e.preventDefault();
  });
  const close = popAt(btn, pop, pop.querySelector("button"));
}

// Outline icons, 14px, drawn in the label's color.
const ICONS = {
  run: '<path d="M2.5 4.5 6 8l-3.5 3.5M7.5 12h6"/>',
  configure: '<path d="M2 4.5h7M12 4.5h2M2 11.5h2M7 11.5h7"/><circle cx="10.5" cy="4.5" r="1.5"/><circle cx="5.5" cy="11.5" r="1.5"/>',
  start: '<path d="M4.5 3v10l8.5-5z"/>',
  stop: '<rect x="4" y="4" width="8" height="8" rx="1"/>',
};
export function icon(name: keyof typeof ICONS) {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 16 16");
  s.setAttribute("aria-hidden", "true");
  s.innerHTML = ICONS[name];
  return s;
}
export const barBtn = (cls: string, ico: Element, label: string) => {
  const b = el("button", cls);
  b.append(ico, el("span", null, label));
  return b;
};

/** Dev start/stop: Start (green) while something in Dev start is stopped, else Stop (red)
 * while anything runs; a spinner and "Starting…"/"Stopping…" while it works. */
function devButton(name: string) {
  if (devAll.site !== name) return null;
  const busy = devAll.busy;
  const action = busy || (devAll.stop ? "stop" : devAll.start ? "start" : "");
  if (!action) return null;
  const label = busy ? (busy === "start" ? "Starting…" : "Stopping…") : action === "start" ? "Start" : "Stop";
  const b = barBtn(`dev-all dev-${action}`, busy ? el("span", "spin-dot") : icon(action), label);
  b.title = action === "start" ? "Start the app and every process in Dev start" : "Stop the app and all its processes";
  b.disabled = !!busy;
  b.onclick = () => devAll.run(action);
  return b;
}

function drawBar(name: string) {
  bar.replaceChildren();
  const cmds = procCache.get(name)?.commands ?? [];
  if (cmds.length && configuring !== name) {
    const run = barBtn("dev-run", icon("run"), "Run");
    run.append(el("span", "dev-chevron", "▾"));
    run.title = "Run one of the app’s scripts";
    run.setAttribute("aria-haspopup", "menu");
    run.onclick = () => runMenu(name, run);
    bar.append(run);
  }
  const cfg = barBtn("dev-configure", icon("configure"), "Configure");
  cfg.setAttribute("aria-pressed", String(configuring === name));
  cfg.title = "Choose the processes Dev runs and their commands";
  cfg.onclick = () => {
    configuring = configuring === name ? null : name;
    draft = null;
    confError = "";
    drawDev(name);
    devTick();
  };
  bar.append(cfg);
  const all = configuring === name ? null : devButton(name);
  if (all) bar.append(all);
}

/** Called whenever the app's processes are drawn, and when the tab is shown. */
export function drawDev(name: string) {
  const root = $("#site");
  const panel = $('[data-panel="dev"]', root);
  if (!panel) return;
  const o = procCache.get(name);
  const procs = procItems(name);
  const runs = runItems(name);
  const kind = snap?.sites.find((s) => s.name === name)?.kind ?? "";
  // Apps that run processes, or could; a plain folder has nothing to run.
  const show = !!(procs.length || o?.commands.length || PHP_KINDS.has(kind) || PROCESS_KINDS.has(kind));
  $('.tabs [data-tab="dev"]', root).hidden = !show;
  if (o && !show && tab === "dev") showTab(root, "overview");
  if (!show) return panel.replaceChildren();

  if (configuring && configuring !== name) configuring = null;
  const items = [...procs, ...runs];
  let key = devSel.get(name) ?? "all";
  if (key !== "all" && !items.some((x) => x.key === key)) key = "all";
  if (view?.site !== name || view.key !== key) {
    view = { site: name, key, tails: new Map(), pending: new Map(), styles: new Map(), ran: new Map(), live: null };
    term.out.replaceChildren();
    term.atEnd = true;
  }
  if (box.parentNode !== panel) {
    panel.replaceChildren(box);
    term.restore();
  }
  const v = view;
  for (const x of items) {
    if (v.ran.get(x.key) === false && x.running) forget(v, x.key);
    v.ran.set(x.key, x.running);
  }

  drawBar(name);
  main.hidden = configuring === name;
  conf.hidden = configuring !== name;
  if (configuring === name) return drawConf(name);

  const running = procs.filter((x) => x.running).length;
  const anyBad = procs.some((x) => x.crashed);
  list.replaceChildren(
    entry(name, { key: "all", name: "All", color: "", running: !!running, crashed: anyBad }),
    ...procs.map((x) => entry(name, x)),
  );
  if (!procs.length) list.append(el("p", "dev-none", "No processes yet. Configure adds one."));
  if (runs.length) list.append(el("div", "dev-divider"), ...runs.map((x) => entry(name, x)));

  head.replaceChildren();
  const actions = el("span", "proc-actions");
  const one = items.find((x) => x.key === key);
  if (!one) {
    head.append(el("span", "console-title", "All"), el("span", "dev-cmd", `${running} of ${procs.length} running`));
  } else if (one.run) {
    const c = one.run;
    head.append(el("span", "console-title", c.name), el("span", "dev-cmd", scriptLabel(c)));
    if (c.exit && !c.running && c.exit.code !== 0) head.append(el("span", "dev-exit bad", exitText(c)));
    if (c.running) actions.append(procBtn("■ Stop", `Stop ${scriptLabel(c)}`, () => dismiss(name, c)));
    else {
      const again = procBtn("↻ Run again", `Run ${scriptLabel(c)} again`, () =>
        confirmPop(again, `Run ${scriptLabel(c)}?`, "Run", () => runScript(name, c)),
      );
      actions.append(again);
    }
  } else {
    const r = one.row!;
    head.append(el("span", "console-title", r.name), el("span", "dev-cmd", r.command));
    if (r.running || r.crashed) {
      actions.append(procBtn("Restart", `Restart ${r.name}`, () => runProc(name, one, "restart")));
      actions.append(procBtn("■", r.running ? `Stop ${r.name}` : `Dismiss ${r.name}`, () => runProc(name, one, "stop")));
    } else {
      const start = procBtn("▶ Start", `Start ${r.name}`, () => runProc(name, one, "start"));
      start.classList.add("dev-head-start");
      actions.append(start);
    }
  }
  head.append(actions);

  // Nothing on screen yet: whether it runs, and ▶ for a stopped process.
  empty.replaceChildren();
  const stopped = one ? !one.running : !running;
  if (!stopped) empty.append(el("span", null, "No output yet"));
  else if (one?.run) empty.append(el("span", null, "No output"));
  else {
    empty.append(el("span", null, "Not running"));
    // All has the toolbar's Start; one process gets its own ▶.
    if (one) {
      const go = procBtn("▶", `Start ${one.name}`, () => runProc(name, one, "start"));
      go.title = `Start ${one.name}`;
      empty.append(go);
    }
  }
  syncEmpty();
}

/** One line of output, its style carried on in `s`; in All (`width` ≥ 0), tagged with its
 * process's name in its color. Also the Containers tab's. */
export function line(x: Pick<Item, "key" | "name" | "color">, text: string, s: Sgr, width: number) {
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

/** New output appended as it comes, nothing drawn twice. One process's unfinished last
 * line (a progress bar redrawn with \r) is drawn live; All shows a line once it ends. */
function append(v: View, x: Item, text: string, width: number) {
  const lines = ((v.pending.get(x.key) ?? "") + text).split("\n");
  const rest = lines.pop()!;
  v.pending.set(x.key, rest);
  if (!v.styles.has(x.key)) v.styles.set(x.key, sgr());
  const s = v.styles.get(x.key)!;
  const out = document.createDocumentFragment();
  for (const l of lines) out.append(line(x, l, s, width));
  v.live?.remove();
  v.live = null;
  if (width < 0 && rest) {
    const live = line(x, rest, { ...s }, width);
    v.live = live;
    out.append(live);
  }
  term.out.append(out);
  term.keep(() => trimLines(term.out, MAX_LINES));
  term.follow();
  syncEmpty();
}

/** New output for what is shown. */
export async function devTick() {
  const v = view;
  if (!v || configuring === v.site) return;
  const procs = procItems(v.site);
  const shown = v.key === "all" ? procs : [...procs, ...runItems(v.site)].filter((x) => x.key === v.key);
  const width = v.key === "all" ? Math.max(0, ...shown.map((x) => x.name.length)) : -1;
  await Promise.all(
    shown.map(async (x) => {
      if (!v.tails.has(x.key)) v.tails.set(x.key, tail(v.site, x.key));
      const t = v.tails.get(x.key)!;
      const text = await readTail(t);
      if (v !== view || v.tails.get(x.key) !== t) return;
      if (t.cleared) {
        t.cleared = false;
        forget(v, x.key);
        v.tails.set(x.key, t);
      }
      if (text) append(v, x, text, width);
    }),
  );
}

/* ---------- Configure: the processes as a table ---------- */

/** Save one change to a process; a failure shows below the table. */
async function save(name: string, args: Record<string, unknown>, cmd = "set_process") {
  try {
    await askProcs(name, cmd, args);
    confError = "";
    return true;
  } catch (e) {
    confError = String(e);
    return false;
  } finally {
    if (selected === name) {
      drawProcs(name);
      drawConf(name, true);
    }
  }
}

function check(checked: boolean, aria: string, onchange: (on: boolean) => void) {
  const c = el("input");
  c.type = "checkbox";
  c.setAttribute("role", "switch");
  c.checked = checked;
  c.setAttribute("aria-label", aria);
  c.onchange = () => onchange(c.checked);
  return c;
}

function cell(...kids: (Node | string)[]) {
  const td = el("td");
  td.append(...kids);
  return td;
}

/** A one-line field: Return saves, Esc puts back what it was. */
function field(value: string, aria: string, placeholder: string, onsave: ((v: string) => void) | null) {
  const input = el("input", "dev-conf-input");
  input.value = input.defaultValue = value;
  input.spellcheck = false;
  input.placeholder = placeholder;
  input.setAttribute("aria-label", aria);
  input.onkeydown = (e) => {
    if (e.key === "Escape") input.value = input.defaultValue;
    if (onsave && (e.key === "Enter" || e.key === "Escape")) input.blur();
  };
  if (onsave) input.onchange = () => input.value.trim() !== input.defaultValue && onsave(input.value.trim());
  return input;
}

function drawConf(name: string, force = false) {
  // Someone is typing: a poll doesn't wipe it.
  const active = document.activeElement as HTMLInputElement | null;
  if (!force && active && conf.contains(active) && active.value !== active.defaultValue) return;
  const rows = devRows(procCache.get(name));
  const table = el("table", "dev-conf-table");
  const headRow = el("tr");
  for (const [h, tip] of [
    ["Name", ""],
    ["Command", "Runs in the app’s folder"],
    ["Dev start", "Started by Start in this toolbar"],
    ["With Anvil", "Started when Anvil starts"],
    ["", ""],
  ]) {
    const th = el("th", null, h);
    if (tip) th.title = tip;
    headRow.append(th);
  }
  const thead = el("thead");
  thead.append(headRow);
  const body = el("tbody");
  for (const r of rows) {
    const tr = el("tr");
    const id = r.main ? "" : r.id;
    const dev = check(r.main || !r.manual, `${r.name} in Dev start`, (on) => save(name, { id, manual: !on }));
    if (r.main) {
      dev.disabled = true;
      dev.title = "The app itself always starts with Dev";
    }
    const remove = r.main ? "" : procBtn("×", `Remove ${r.name}`, () => save(name, { id, hidden: true }));
    if (remove) {
      remove.className = "icon-btn dev-conf-remove";
      remove.title = r.added ? "Remove" : "Remove (Reset to detected brings it back)";
    }
    tr.append(
      cell(el("span", "dev-conf-name", r.name)),
      cell(field(r.command, `Command for ${r.name}`, "command", (c) => save(name, { id, command: c }))),
      cell(dev),
      cell(check(r.autostart, `Start ${r.name} with Anvil`, (on) => save(name, { id, autostart: on }))),
      cell(remove),
    );
    body.append(tr);
  }
  if (draft) {
    const d = draft;
    const tr = el("tr", "dev-conf-draft");
    const n = field(d.name, "Name of the new process", "name", null);
    const c = field(d.command, "Command of the new process", "command", null);
    n.oninput = () => (d.name = n.value);
    c.oninput = () => (d.command = c.value);
    const add = async () => {
      if (await save(name, { name: d.name.trim(), command: d.command.trim() }, "process_add")) draft = null;
      drawConf(name, true);
    };
    for (const i of [n, c])
      i.onkeydown = (e) => {
        if (e.key === "Enter") add();
        if (e.key === "Escape") {
          draft = null;
          confError = "";
          drawConf(name, true);
        }
      };
    const addBtn = procBtn("Add", "Add the process", add);
    const cancel = procBtn("×", "Cancel adding", () => {
      draft = null;
      confError = "";
      drawConf(name, true);
    });
    cancel.className = "icon-btn dev-conf-remove";
    tr.append(cell(n), cell(c), cell(), cell(), cell(addBtn, cancel));
    body.append(tr);
  }
  table.append(thead, body);
  const error = el("p", "error-text dev-conf-error", confError);
  error.hidden = !confError;
  const foot = el("div", "dev-conf-foot");
  const plus = el("button", null, "+ Add process");
  plus.disabled = !!draft;
  plus.onclick = () => {
    draft = { name: "", command: "" };
    drawConf(name, true);
    ($(".dev-conf-draft input", conf) as HTMLInputElement | null)?.focus();
  };
  const reset = el("button", null, "Reset to detected");
  reset.title = "What the app’s dev script (php artisan dev, package.json) starts";
  reset.onclick = () => confirmPop(reset, "Reset to the detected processes?", "Reset", () => save(name, {}, "processes_reset"));
  const done = el("button", null, "Done");
  done.onclick = () => {
    configuring = null;
    draft = null;
    confError = "";
    drawDev(name);
    devTick();
  };
  foot.append(plus, reset, el("span", "spacer"), done);
  conf.replaceChildren(table, error, foot);
}

/** Open the tab, on one entry if given. */
export function openDev(name: string, key?: string) {
  if (key) devSel.set(name, key);
  showTab($("#site"), "dev");
}

late.drawDev = drawDev;
late.devTab = () => {
  if (!selected) return;
  drawDev(selected);
  devTick();
  if (!main.hidden && box.isConnected) list.focus({ preventScroll: true });
  // WebKit forgets a hidden pane's scroll position: put it back once it is laid out.
  requestAnimationFrame(term.restore);
};
late.openDev = openDev;

// Only while the tab is shown: output twice a second, the processes' states every two.
let ticks = 0;
setInterval(() => {
  if (!shown() || mode !== "apps" || !selected || tab !== "dev") return;
  if (view?.site === selected) devTick();
  if (++ticks % 4 === 0 && !procLoading.has(selected)) loadProcs(selected);
}, 500);
