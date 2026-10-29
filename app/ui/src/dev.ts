import type { CommandChip, ProcessRow } from "./types";
import { $, M, PHP_KINDS, PROCESS_KINDS, ask, call, el, invoke, popAt, seg, selected, showError, showTab, snap, tab } from "./app";
import { type Item as Proc, apply, items, labelSpan, plan } from "./confplan";
import { type Tail, readTail, tail } from "./console";
import { late } from "./late";
import { icon } from "./icons";
import { askProcs, confirmPop, devAll, devRows, drawProcs, loadProcs, procBtn, procCache } from "./processes";
import { type Sgr, sgr } from "./term";
import { line, streamPane } from "./stream";

/* Dev tab: the app's dev processes on the left, the chosen one's output live on the right,
 * as `php artisan dev` shows them in a terminal. "All" (the default) merges every
 * process's output, each line tagged with its name in its color. Run ▾ runs a one-off
 * script, listed below the processes until dismissed; Configure edits the processes. */

const devSel = new Map<string, string>(); // site name -> "all", "app", "proc:<id>" or "once:<id>"
export const COLORS = 6; // name colors pc0…pc5 (blue, violet, magenta, cyan, indigo, slate), none a state's

type Item = {
  key: string;
  name: string;
  color: string;
  running: boolean;
  starting?: boolean;
  crashed: boolean;
  row?: ProcessRow;
  run?: CommandChip;
};

/** The app's own server when it is a process, then every other process, each in its color. */
const procItems = (name: string): Item[] =>
  devRows(procCache.get(name)).map((r, i) => ({
    key: r.main ? "app" : `proc:${r.id}`,
    name: r.name,
    color: `pc${i % COLORS}`,
    running: r.running,
    starting: r.starting,
    crashed: !!r.crashed && !r.running,
    row: r,
  }));
/** Scripts run from Run ▾ that are running or ended and not yet dismissed. */
const runItems = (name: string): Item[] =>
  (procCache.get(name)?.commands ?? [])
    .filter((c) => c.running || c.exit)
    .map((c) => ({ key: `once:${c.id}`, name: c.name, color: "", running: c.running, crashed: !c.running && c.exit?.code !== 0, run: c }));
const scriptLabel = (c: CommandChip) =>
  c.source === "Packages" ? c.name : c.source === "composer.json" ? `composer ${c.name}` : `npm run ${c.name}`;
/** The scripts Run ▾ offers: not the Packages tab's Install and Update. */
const scripts = (name: string) => (procCache.get(name)?.commands ?? []).filter((c) => c.source !== "Packages");

/** Dev with a run's output, `once:<id>` (the Packages tab's Install and Update). */
export function showRun(name: string, id: string) {
  devSel.set(name, `once:${id}`);
  showTab($("#site"), "dev");
  drawDev(name);
}
const exitText = (exit: { code: number | null } | null) => (exit?.code == null ? "Ended by a signal" : `Exited with code ${exit.code}`);

const pane = streamPane({
  listCls: "dev-list",
  label: "Processes",
  widthKey: "dev-list-width",
  // "All processes" with its Start, and Commands with Run, still fit.
  minWidth: 168,
  sel: devSel,
  draw: (name) => drawDev(name),
  tick: () => devTick(),
  keys,
  current: () => view,
  entries: (name) => [...procItems(name), ...runItems(name)].map((x) => x.key),
});
const { box, bar, main, list, term, syncEmpty, pick } = pane;
const conf = el("div", "dev-conf");
box.append(conf);

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
/** Configure's changes until Done saves them and Cancel drops them: the processes as
 * items, and `base`, them as saved (JSON). Follows the app's processes while unchanged. */
let edits: { site: string; base: string; items: Proc[] } | null = null;
let saving = false;

/** An entry's output began again (a restart, Clear): its lines go, and so does its reader. */
function forget(v: View, key: string) {
  v.tails.delete(key);
  v.pending.delete(key);
  v.styles.delete(key);
  if (v.live?.dataset.key === key) v.live = null;
  for (const l of term.out.querySelectorAll(`[data-key="${CSS.escape(key)}"]`)) l.remove();
  syncEmpty();
}

/** The output pane in characters, for COLUMNS and LINES of what starts from here (in
 * All, less the name tag); nothing when it isn't laid out, so Anvil's default holds. */
function termSize(): { columns?: number; lines?: number } {
  const out = term.out as HTMLElement;
  if (!out.clientWidth) return {};
  const probe = el("span", null, "0".repeat(20));
  probe.style.cssText = "position:absolute;visibility:hidden;white-space:pre";
  out.append(probe);
  const { width, height } = probe.getBoundingClientRect();
  probe.remove();
  if (!width) return {};
  const css = getComputedStyle(out);
  const px = (k: string) => parseFloat(css.getPropertyValue(k)) || 0;
  const tag = view?.key === "all" ? Math.max(0, ...procItems(view.site).map((x) => x.name.length)) + 3 : 0;
  return {
    columns: Math.max(20, Math.floor((out.clientWidth - px("padding-left") - px("padding-right")) / (width / 20)) - tag),
    lines: Math.max(5, Math.floor((out.clientHeight - px("padding-top") - px("padding-bottom")) / height)),
  };
}

/** Dev start or stop (the toolbar's Start/Stop), when it is offered. */
function headerAll(action: "start" | "stop") {
  if (devAll.site === view?.site && devAll[action] && !devAll.busy) devAll.run(action, termSize());
}

/** The keys after ↑↓ (see `streamPane`). */
function keys(k: string, name: string) {
  const v = view!;
  const procs = procItems(name);
  const one = [...procs, ...runItems(name)].find((x) => x.key === v.key);
  if (k === "s") {
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
  } else return false;
  return true;
}

/** Start, stop or restart a process; a restart starts its output over. */
async function runProc(name: string, x: Item, action: string) {
  const r = x.row!;
  const size = action === "stop" ? {} : termSize();
  if (r.main) {
    await call("app_run", { site: name, action, ...size });
    await loadProcs(name);
  } else await loadProcs(name, "process_run", { name: r.id, action, ...size });
  if (action === "restart" && view?.site === name) forget(view, x.key);
}

async function runScript(name: string, c: CommandChip) {
  if (await loadProcs(name, "command_run", { id: c.id, ...termSize() })) pick(name, `once:${c.id}`);
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
  // <app>--<label>.<tld>: only the label is editable.
  const [from, to] = labelSpan(name, host);
  const done = await ask({
    title: `Address of ${r.name}`,
    field: "Label",
    placeholder: host.slice(from, to),
    optional: true,
    preview: (v) => (v ? `Opens ${host.slice(0, from)}${v}${host.slice(to)}` : "Empty: back to the automatic label"),
    ok: "Save",
    submit: (v) => askProcs(name, "set_process", { id: r.id, ...(v ? { host: v } : { resetHost: true }) }),
  });
  if (done) await loadProcs(name);
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
    // An extra dev server's own address, <app>--<label>.<tld>: the label is editable.
    if (r.address && !r.main) items.push(item("Edit Address…", () => editAddress(name, r)));
    items.push(
      M.PredefinedMenuItem.new({ item: "Separator" }),
      item("Clear", () => clearOutput(name, x)),
    );
  }
  (await M.Menu.new({ items: await Promise.all(items) })).popup();
}

function entry(name: string, x: Item) {
  // The status column: a process's dot; a run's spinner, then a check or a cross.
  let status: HTMLElement;
  // Red crashed, amber starting (its port doesn't answer yet), green running, grey stopped;
  // in that order for All, as in the sidebar.
  if (!x.run) status = el("span", `dot ${x.crashed ? "bad" : x.running ? (x.starting ? "amber" : "on") : ""}`);
  else if (x.running) {
    status = el("span", "dev-status");
    status.append(el("span", "spin-dot"));
    status.title = "Running";
  } else {
    const ok = x.run.exit?.code === 0;
    status = el("span", `dev-status ${ok ? "ok" : "bad"}`);
    status.append(icon(ok ? "check" : "x"));
    status.title = ok ? "Succeeded" : exitText(x.run.exit);
  }
  const label =
    x.run?.source === "composer.json" && runItems(name).some((y) => y.key !== x.key && y.name === x.name) ? `composer ${x.name}` : x.name;
  const parts = [status, el("span", `dev-name ${x.color}`.trim(), label)];
  // All starts or stops everything; each process has its own ▶ or ■. A click on either
  // acts without picking the row.
  const act = x.key === "all" ? devButton(name) : x.row ? procAct(name, x) : null;
  if (act) {
    const go = act.onclick;
    act.classList.add("dev-act");
    act.onclick = (e) => {
      e.stopPropagation();
      go?.call(act, e);
    };
    parts.push(act);
  }
  if (x.run) {
    const c = x.run;
    const x2 = procBtn(icon("x"), c.running ? `Stop and dismiss ${c.name}` : `Dismiss ${c.name}`, () => dismiss(name, c));
    x2.className = "dev-dismiss";
    x2.title = c.running ? "Stop and dismiss" : "Dismiss";
    parts.push(x2);
  }
  return pane.entry(name, x.key, parts, x.key === "all" ? undefined : (it) => itemMenu(name, x, it));
}

/** A process's own button in its row: ▶ starts it, ■ stops it. */
function procAct(name: string, x: Item) {
  const running = !!x.row?.running;
  const b = el("button", `dev-proc ${running ? "dev-stop" : "dev-start"}`);
  b.append(icon(running ? "stop" : "start"));
  b.title = running ? `Stop ${x.name}` : `Start ${x.name}`;
  b.setAttribute("aria-label", b.title);
  b.onclick = () => runProc(name, x, running ? "stop" : "start");
  return b;
}

/** Run ▾: the app's one-off scripts by name, under where they come from (the script
 * itself is the tooltip); choosing one asks first. */
function runMenu(name: string, btn: HTMLElement) {
  const cmds = scripts(name);
  const pop = el("div", "dev-run-menu");
  pop.setAttribute("role", "menu");
  const sources = [...new Set(cmds.map((c) => c.source))];
  // Every source titled, in its own color: Node's green, Composer's orange.
  const tone = { "package.json": "pc4", "composer.json": "pc3" };
  for (const src of sources) {
    pop.append(el("div", `dev-run-group ${tone[src] ?? "pc1"}`, src));
    for (const c of cmds.filter((c) => c.source === src)) {
      const b = el("button", "dev-run-item");
      b.setAttribute("role", "menuitem");
      b.append(el("span", "dev-run-name", c.name));
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
  // Run sits at the left of the window: the menu opens rightwards from it, kept inside.
  const r = btn.getBoundingClientRect();
  const left = Math.max(8, Math.min(r.left, document.documentElement.clientWidth - pop.offsetWidth - 8));
  Object.assign(pop.style, { right: "", left: `${left + window.scrollX}px` });
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
  b.onclick = () => devAll.run(action, termSize());
  return b;
}

function drawBar(name: string) {
  bar.replaceChildren();
  const cfg = barBtn("dev-configure", icon("configure"), "Configure");
  cfg.setAttribute("aria-pressed", String(configuring === name));
  cfg.title = "Choose the processes Dev runs and their commands";
  cfg.onclick = () => {
    if (configuring !== name) {
      configuring = name;
      drawDev(name);
      return;
    }
    // Closing from the toolbar is Cancel; changes not saved are asked about first.
    if (changed(name)) confirmPop(cfg, "Discard the changes?", "Discard", () => closeConf(name));
    else closeConf(name);
  };
  // Configure on the left; Table | JSON on the right while configuring. Start and Stop
  // sit in All's row.
  bar.append(cfg, el("span", "spacer"));
  if (configuring === name) bar.append(modeSeg);
}

/** Below the processes, a small box of their own for one-off scripts: Run ▾ and each
 * run with how it went. */
function commandsBox(name: string, runs: Item[]) {
  const cmds = scripts(name);
  if (!cmds.length && !runs.length) return null;
  const card = el("div", "group procs-list dev-card dev-cmds");
  const head = el("div", "dev-cmds-head");
  head.append(el("span", null, "Commands"));
  if (cmds.length) {
    const run = barBtn("dev-run", icon("run"), "Run");
    const chevron = el("span", "dev-chevron");
    chevron.append(icon("chevron"));
    run.append(chevron);
    run.title = "Run one of the app’s scripts";
    run.setAttribute("aria-haspopup", "menu");
    run.onclick = () => runMenu(name, run);
    head.append(run);
  }
  card.append(head, ...runs.map((x) => entry(name, x)));
  return card;
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
  if (o && !show && tab === "dev") showTab(root, "overview", true);
  if (!show) return panel.replaceChildren();

  if (configuring && configuring !== name) configuring = null;
  const items = [...procs, ...runs];
  let key = devSel.get(name) ?? "all";
  if (key !== "all" && !items.some((x) => x.key === key)) key = "all";
  const fresh = view?.site !== name || view.key !== key;
  if (fresh) view = { site: name, key, tails: new Map(), pending: new Map(), styles: new Map(), ran: new Map(), live: null };
  pane.attach(panel, fresh);
  const v = view!;
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
  const procCard = el("div", "group procs-list dev-card dev-procs");
  procCard.append(
    entry(name, {
      key: "all",
      name: "All processes",
      color: "",
      running: !!running,
      starting: procs.some((x) => x.starting),
      crashed: anyBad,
    }),
    ...procs.map((x) => entry(name, x)),
  );
  if (!procs.length) procCard.append(el("p", "dev-none", "No processes yet. Configure adds one."));
  const cmdCard = commandsBox(name, runs);
  list.replaceChildren(procCard, ...(cmdCard ? [cmdCard] : []));

  const one = items.find((x) => x.key === key);
  if (!one) pane.heading("All processes", `${running} of ${procs.length} running`);
  else if (one.run) {
    const c = one.run;
    const bad = c.exit && !c.running && c.exit.code !== 0 ? [el("span", "dev-exit bad", exitText(c.exit))] : [];
    const actions = pane.heading(c.name, scriptLabel(c), ...bad);
    if (c.running) actions.append(procBtn([icon("stop"), "Stop"], `Stop ${scriptLabel(c)}`, () => dismiss(name, c)));
    else {
      const again = procBtn([icon("again"), "Run again"], `Run ${scriptLabel(c)} again`, () =>
        confirmPop(again, `Run ${scriptLabel(c)}?`, "Run", () => runScript(name, c)),
      );
      actions.append(again);
    }
  } else {
    const r = one.row!;
    // A crash says how it ended, as a one-off run's does.
    const bad = r.crashed && !r.running ? [el("span", "dev-exit bad", exitText(r.crashed))] : [];
    const actions = pane.heading(r.name, r.command, ...bad);
    if (r.running || r.crashed) {
      actions.append(procBtn("Restart", `Restart ${r.name}`, () => runProc(name, one, "restart")));
      // ■ stops; a crashed one has nothing to stop, so its ✕ only dismisses the crash.
      actions.append(
        procBtn(icon(r.running ? "stop" : "x"), r.running ? `Stop ${r.name}` : `Dismiss ${r.name}`, () => runProc(name, one, "stop")),
      );
    } else {
      const start = procBtn([icon("start"), "Start"], `Start ${r.name}`, () => runProc(name, one, "start"));
      start.classList.add("dev-head-start");
      actions.append(start);
    }
  }

  // Nothing on screen yet: whether it runs, and ▶ for a stopped process.
  const stopped = one ? !one.running : !running;
  if (!stopped) pane.notice("No output yet");
  else if (one?.run) pane.notice("No output");
  // All has the toolbar's Start; one process gets its own ▶.
  else pane.notice("Not running", one && { name: one.name, run: () => runProc(name, one, "start") });
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
  pane.push(out);
}

/** New output for what is shown. */
async function devTick() {
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

/* ---------- Configure ---------- */

const rowsOf = (name: string) => devRows(procCache.get(name));
const mainOf = (name: string) => rowsOf(name).find((r) => r.main)?.name;
const asJson = (xs: Proc[]) => JSON.stringify(xs, null, 2);

/** Configure's changes for `name`, made afresh from its processes while none are pending. */
function editsFor(name: string) {
  const base = asJson(items(rowsOf(name)));
  if (!edits || edits.site !== name || (!saving && !changed(name) && edits.base !== base))
    edits = { site: name, base, items: JSON.parse(base) };
  return edits;
}

/** Changes not saved yet: the table's, or the JSON being typed. */
function changed(name: string) {
  if (!edits || edits.site !== name) return false;
  const now = confMode === "json" && js.ed && js.site === name ? js.ed.text : asJson(edits.items);
  return now !== edits.base;
}

/** Close Configure; whatever wasn't saved is dropped. */
function closeConf(name: string) {
  configuring = null;
  draft = null;
  confError = "";
  edits = null;
  js.ed?.view.destroy();
  js.ed = null;
  js.site = "";
  drawDev(name);
  devTick();
}

/** Done: save the changes (the JSON's when editing JSON), then close. A failure stays
 * open with the changes and says why. */
async function finish(name: string) {
  const e = editsFor(name);
  let next = e.items;
  if (confMode === "json" && js.ed) {
    const m = await (mods ??= Promise.all([import("./jsoneditor"), import("./confjson")]));
    const got = m[1].check(js.ed.text, mainOf(name)).items;
    if (!got) return;
    next = got;
  }
  const empty = next.find((x) => !x.command.trim());
  if (empty) {
    confError = `${empty.name}: a command is needed`;
    return drawConf(name, true);
  }
  if (asJson(next) !== e.base) {
    saving = true;
    drawConf(name, true);
    try {
      await apply(
        plan(rowsOf(name), next),
        ([cmd, args]) => askProcs(name, cmd, args),
        () => rowsOf(name),
      );
    } catch (err) {
      confError = String(err);
      e.items = next;
      await loadProcs(name);
      return drawConf(name, true);
    } finally {
      saving = false;
      if (selected === name) drawProcs(name);
    }
  }
  closeConf(name);
}

/** Configure's foot, the same in both modes: what the mode has on the left, then Cancel
 * and Done. */
function confFoot(name: string, left: HTMLElement[]) {
  const foot = el("div", "dev-conf-foot");
  const cancel = el("button", null, "Cancel");
  cancel.title = "Close without saving the changes";
  cancel.onclick = () => closeConf(name);
  const done = el("button", "primary", "Done");
  done.onclick = () => finish(name);
  cancel.disabled = done.disabled = saving;
  foot.append(...left, el("span", "spacer"), cancel, done);
  return { foot, done };
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

/** A one-line field that changes `set` as it's typed; Esc puts back what it was. */
function field(value: string, aria: string, placeholder: string, set: (v: string) => void) {
  const input = el("input", "dev-conf-input");
  input.value = input.defaultValue = value;
  input.spellcheck = false;
  input.placeholder = placeholder;
  input.setAttribute("aria-label", aria);
  input.oninput = () => set(input.value);
  input.onkeydown = (e) => {
    if (e.key !== "Escape") return;
    input.value = input.defaultValue;
    set(input.value);
  };
  return input;
}

/* ---------- Configure: the processes as JSON ---------- */

let confMode: "table" | "json" = "table";
const modeSeg = seg(
  "Edit as",
  [
    ["table", "Table"],
    ["json", "JSON"],
  ],
  "table",
  async (m) => {
    const name = configuring;
    if (!name || m === confMode) return;
    confError = "";
    if (m === "table" && js.ed) {
      // The JSON's changes carry over to the table; JSON with mistakes stays JSON.
      const [, cj] = await mods!;
      const got = cj.check(js.ed.text, mainOf(name)).items;
      if (!got) {
        confError = "Fix the JSON first, or Cancel";
        return drawConf(name, true);
      }
      editsFor(name).items = got;
    }
    confMode = m as "table" | "json";
    if (m === "json") {
      js.ed?.view.destroy();
      js.ed = null;
      js.site = "";
    }
    drawConf(name, true);
  },
);
modeSeg.classList.add("dev-conf-mode");
type Mods = [typeof import("./jsoneditor"), typeof import("./confjson")];
let mods: Promise<Mods> | null = null; // CodeMirror, loaded on the first JSON
const js = {
  site: "",
  ed: null as null | ReturnType<Mods[0]["jsonEditor"]>,
  wrap: el("div", "dev-json"),
  host: el("div", "dev-json-editor"),
  msg: el("p", "dev-json-msg"),
  format: el("button", null, "Format"),
  done: null as HTMLButtonElement | null,
};
js.wrap.append(js.host, js.msg);

/** The line a position is on, 1-based. */
const lineOf = (text: string, at: number) => text.slice(0, at).split("\n").length;

/** Done's and Format's state and the line under the editor, after every edit. */
function syncJson(m: Mods[1], name: string) {
  if (!js.ed) return;
  const text = js.ed.text;
  const { problems } = m.check(text, mainOf(name));
  const p = problems[0];
  const more = problems.length > 1 ? ` (${problems.length - 1} more)` : "";
  js.msg.className = `dev-json-msg${p || confError ? " error-text" : ""}`;
  js.msg.textContent =
    confError || (p ? `Line ${lineOf(text, p.from)}: ${p.message}${more}` : changed(name) ? "Done saves the changes" : "");
  if (js.done) js.done.disabled = saving || !!problems.length;
  let parses = true;
  try {
    JSON.parse(text);
  } catch {
    parses = false;
  }
  js.format.disabled = saving || !parses;
}

async function drawJson(name: string) {
  const e = editsFor(name);
  const { foot, done } = confFoot(name, [js.format]);
  js.done = done;
  js.wrap.querySelector(".dev-conf-foot")?.remove();
  js.wrap.append(foot);
  if (!conf.contains(js.wrap)) conf.replaceChildren(js.wrap);
  if (js.site !== name) {
    js.ed?.view.destroy();
    js.ed = null;
    js.site = name;
    js.host.replaceChildren(el("span", "dev-json-loading", "Loading the editor…"));
  }
  const [ed, m] = await (mods ??= Promise.all([import("./jsoneditor"), import("./confjson")]));
  if (js.site !== name || configuring !== name) return;
  if (!js.ed) {
    js.host.replaceChildren();
    js.ed = ed.jsonEditor(
      js.host,
      asJson(e.items),
      (t) => m.check(t, mainOf(name)).problems,
      () => {
        confError = "";
        syncJson(m, name);
      },
    );
  } else if (!saving && !changed(name) && js.ed.text !== e.base) js.ed.set(e.base); // follows changes made elsewhere
  syncJson(m, name);
  js.format.onclick = () => {
    try {
      js.ed!.set(JSON.stringify(JSON.parse(js.ed!.text), null, 2));
    } catch {}
  };
}

/* ---------- Configure: the processes as a table ---------- */

function drawConf(name: string, force = false) {
  for (const b of modeSeg.querySelectorAll("button")) b.setAttribute("aria-pressed", String((b as HTMLElement).dataset.id === confMode));
  if (confMode === "json") return void drawJson(name);
  // Someone is typing: a poll doesn't wipe it.
  const active = document.activeElement as HTMLInputElement | null;
  if (!force && active && conf.contains(active) && active.tagName === "INPUT" && active.type !== "checkbox") return;
  const e = editsFor(name);
  const rows = rowsOf(name);
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
  for (const x of e.items) {
    const r = rows.find((r) => r.name === x.name);
    const main = !!r?.main;
    const tr = el("tr");
    const dev = check(main || x.devStart, `${x.name} in Dev start`, (on) => (x.devStart = on));
    if (main) {
      dev.disabled = true;
      dev.title = "The app itself always starts with Dev";
    }
    const remove = main
      ? ""
      : procBtn(icon("x"), `Remove ${x.name}`, () => {
          e.items = e.items.filter((y) => y !== x);
          drawConf(name, true);
        });
    if (remove) {
      remove.className = "icon-btn dev-conf-remove";
      remove.title = !r || r.added ? "Remove" : "Remove (Reset to detected brings it back)";
    }
    tr.append(
      cell(el("span", "dev-conf-name", x.name)),
      cell(field(x.command, `Command for ${x.name}`, "command", (c) => (x.command = c))),
      cell(dev),
      cell(check(x.withAnvil, `Start ${x.name} with Anvil`, (on) => (x.withAnvil = on))),
      cell(remove),
    );
    body.append(tr);
  }
  if (draft) {
    const d = draft;
    const tr = el("tr", "dev-conf-draft");
    const n = field(d.name, "Name of the new process", "name", (v) => (d.name = v));
    const c = field(d.command, "Command of the new process", "command", (v) => (d.command = v));
    const add = () => {
      const nm = d.name.trim();
      const cm = d.command.trim();
      confError =
        !nm || !cm
          ? "A new process needs a name and a command"
          : e.items.some((y) => y.name === nm)
            ? `There is a process named ${nm} already`
            : "";
      if (!confError) {
        e.items.push({ name: nm, command: cm, devStart: true, withAnvil: false });
        draft = null;
      }
      drawConf(name, true);
    };
    const drop = () => {
      draft = null;
      confError = "";
      drawConf(name, true);
    };
    for (const i of [n, c])
      i.onkeydown = (ev) => {
        if (ev.key === "Enter") add();
        if (ev.key === "Escape") drop();
      };
    const addBtn = procBtn("Add", "Add the process", add);
    const cancel = procBtn(icon("x"), "Cancel adding", drop);
    cancel.className = "icon-btn dev-conf-remove";
    tr.append(cell(n), cell(c), cell(), cell(), cell(addBtn, cancel));
    body.append(tr);
  }
  table.append(thead, body);
  const error = el("p", "error-text dev-conf-error", confError);
  error.hidden = !confError;
  const plus = el("button", null, "+ Add process");
  plus.disabled = !!draft || saving;
  plus.onclick = () => {
    draft = { name: "", command: "" };
    drawConf(name, true);
    ($(".dev-conf-draft input", conf) as HTMLInputElement | null)?.focus();
  };
  const reset = el("button", null, "Reset to detected");
  reset.title = "What the app’s dev script (php artisan dev, package.json) starts";
  reset.disabled = saving;
  reset.onclick = () =>
    confirmPop(reset, "Reset to the detected processes?", "Reset", async () => {
      edits = null;
      draft = null;
      try {
        await askProcs(name, "processes_reset", {});
        confError = "";
      } catch (err) {
        confError = String(err);
      }
      if (selected === name) drawProcs(name);
      drawConf(name, true);
    });
  conf.replaceChildren(table, error, confFoot(name, [plus, reset]).foot);
}

late.drawDev = drawDev;
late.devTab = pane.shownTab;

// Only while the tab is shown: output twice a second, the processes' states every two.
pane.poll("dev", 500, 4);
