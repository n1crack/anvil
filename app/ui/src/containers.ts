import type { Container } from "./types";
import { $, M, el, invoke, mode, selected, showError, showTab, snap, tab } from "./app";
import { termOut } from "./console";
import { COLORS, barBtn, icon, line, trimLines } from "./dev";
import { shown } from "./live";
import { listSplitter } from "./split";
import { late } from "./late";
import { DOT, containerRun, containerState, loadProcs, procBtn, procBusy, procCache, procLoading } from "./processes";
import { type Sgr, sgr } from "./term";

/* Containers tab: a Docker Compose app's services on the left, the chosen one's log live
 * on the right (one `docker compose logs -f` per service, kept by Anvil while the tab
 * shows the app and read by line number), built like the
 * Dev tab and drawn with its pieces. "All" merges every container's log by time, each line
 * tagged with its service in its color. Stop is `docker compose stop`: Anvil never runs
 * down or rm, so containers and their volumes stay. */

export const containerSel = new Map<string, string>(); // site name -> "all" or a service
const MAX_LINES = 5000;

type Item = { key: string; name: string; color: string; c: Container; state: ReturnType<typeof containerState>; busy?: string };

const items = (name: string): Item[] =>
  (procCache.get(name)?.containers ?? []).map((c, i) => ({
    key: c.service,
    name: c.service,
    color: `pc${i % COLORS}`,
    c,
    state: containerState(c),
    busy: procBusy.get(`${name}/${c.service}`),
  }));
const up = (x: Item) => x.c.state === "running";
/** ":5432 · healthy", or how it ended: "exited (1)". */
const detail = (x: Item) =>
  up(x) || x.c.state === "restarting"
    ? [...x.c.ports.map((p) => `:${p}`), x.c.health ?? (up(x) ? "" : x.c.state)].filter(Boolean).join(" · ")
    : x.c.exit_code
      ? `exited (${x.c.exit_code})`
      : x.c.state;

/** docker's "2026-10-17T10:00:01.112345678Z text" lines after the time `after`, each
 * with a time that sorts as text; a line without one keeps the one before.
 * ponytail: lines written in the same nanosecond as `after` are taken as seen. */
export function stamped(raw: string, after: string | null) {
  const key = (t: string | null) => {
    const m = t?.match(/^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?Z$/);
    return m ? `${m[1]}.${(m[2] ?? "").padEnd(9, "0")}` : null;
  };
  const from = key(after) ?? "";
  const out: { at: string; ts: string; text: string }[] = [];
  let at = from;
  let ts = after ?? "";
  for (const l of raw.split("\n")) {
    const sp = l.indexOf(" ");
    const k = key(sp > 0 ? l.slice(0, sp) : l);
    if (k) [at, ts] = [k, l.slice(0, sp > 0 ? sp : l.length)];
    else if (!l) continue;
    if (from && at <= from) continue;
    out.push({ at, ts, text: k ? (sp > 0 ? l.slice(sp + 1) : "") : l });
  }
  return out;
}

const box = el("div", "dev containers");
const bar = el("div", "dev-bar");
const main = el("div", "dev-main");
const list = el("div", "group procs-list dev-list");
list.tabIndex = 0;
list.setAttribute("role", "listbox");
list.setAttribute("aria-label", "Containers");
const pane = el("section", "console dev-pane");
const head = el("header", "console-head dev-head");
const term = termOut("console-out dev-out");
const empty = el("div", "dev-empty");
term.out.tabIndex = 0;
const keysHint = el("p", "dev-keys", "↑↓ select · s start · x stop · r restart · q stop all · c clear");
pane.append(head, term.out, empty, keysHint);
main.append(list, listSplitter(main, "containers-list-width", 160), pane);
box.append(bar, main);

// What is shown: which app and entry, the time and number of the last line read per
// service, requests on their way, and each service's colors carried from line to line.
type View = {
  site: string;
  key: string;
  since: Map<string, string | null>;
  seq: Map<string, number>;
  asking: Set<string>;
  styles: Map<string, Sgr>;
};
let view: View | null = null;
let following = ""; // the app whose log streams Anvil keeps
let allBusy: "" | "start" | "stop" = "";
let allSite = "";

function syncEmpty() {
  const none = !term.out.querySelector(".dev-line");
  empty.hidden = !none;
  pane.classList.toggle("is-empty", none);
}

function pick(name: string, key: string) {
  containerSel.set(name, key);
  drawContainers(name);
  containersTick();
}

/** Start, stop or restart one container; Docker can take a while, so its row says so. */
const runOne = (name: string, x: Item, action: string) => containerRun(name, [x.name], action, x.name);

/** Start all (`up -d`) or stop all (`stop`, never down): one compose command for the project. */
async function runAll(name: string, action: "start" | "stop") {
  if (allBusy) return;
  const change = items(name).filter((x) => up(x) !== (action === "start"));
  if (!change.length) return;
  [allBusy, allSite] = [action, name];
  drawContainers(name);
  await containerRun(
    name,
    change.map((x) => x.name),
    action,
  );
  allBusy = "";
  if (selected === name) drawContainers(name);
}

main.onkeydown = (e) => {
  const v = view;
  if (!v || e.metaKey || e.ctrlKey || e.altKey || (e.target as HTMLElement).closest("input, textarea, select")) return;
  const name = v.site;
  const all = items(name);
  const one = all.find((x) => x.key === v.key);
  const k = e.key;
  if (k === "ArrowDown" || k === "ArrowUp" || k === "j" || k === "k") {
    const keys = ["all", ...all.map((x) => x.key)];
    const i = keys.indexOf(v.key) + (k === "ArrowDown" || k === "j" ? 1 : -1);
    if (i >= 0 && i < keys.length) pick(name, keys[i]);
  } else if (k === "s") {
    if (!one) runAll(name, "start");
    else if (!up(one) && !one.busy) runOne(name, one, "start");
  } else if (k === "x") {
    if (!one) runAll(name, "stop");
    else if (up(one) && !one.busy) runOne(name, one, "stop");
  } else if (k === "r") {
    for (const x of one ? [one] : all.filter(up)) if (up(x) && !x.busy) runOne(name, x, "restart");
  } else if (k === "q") runAll(name, "stop");
  else if (k === "c") {
    // The screen only: Docker keeps the log.
    for (const l of term.out.querySelectorAll(one ? `[data-key="${CSS.escape(one.key)}"]` : ".dev-line")) l.remove();
    syncEmpty();
  } else return;
  e.preventDefault();
};

const webOf = (x: Item) => (x.c.ports[0] ? `http://localhost:${x.c.ports[0]}` : null);

/** Right-click on a container: Start, or Stop and Restart; its page when it publishes a port. */
async function itemMenu(name: string, x: Item) {
  if (x.busy) return;
  const item = (text, action) => M.MenuItem.new({ text, action });
  const items: Promise<any>[] = up(x)
    ? [item("Stop", () => runOne(name, x, "stop")), item("Restart", () => runOne(name, x, "restart"))]
    : [item("Start", () => runOne(name, x, "start"))];
  const web = webOf(x);
  if (web)
    items.push(
      M.PredefinedMenuItem.new({ item: "Separator" }),
      item("Open in Browser", () => invoke("open_url", { url: web }).catch((e) => showError(String(e)))),
    );
  (await M.Menu.new({ items: await Promise.all(items) })).popup();
}

function entry(name: string, x: Item | null, running = 0, bad = false) {
  const key = x?.key ?? "all";
  const it = el("div", "dev-item");
  it.setAttribute("role", "option");
  it.setAttribute("aria-selected", String(key === view?.key));
  it.dataset.key = key;
  let status: HTMLElement;
  if (x?.busy) {
    status = el("span", "dev-status");
    status.append(el("span", "spin-dot"));
  } else status = el("span", `dot ${x ? DOT[x.state] : running ? "on" : bad ? "bad" : ""}`);
  status.title =
    x?.busy ?? (x ? { running: "Running", starting: "Starting", crashed: "Exited with an error", stopped: "Stopped" }[x.state] : "");
  it.append(status, el("span", "dev-name", x?.name ?? "All"));
  if (x) {
    const d = el("span", `dev-exit${x.state === "crashed" && !x.busy ? " bad" : ""}`, x.busy ?? detail(x));
    d.title = d.textContent;
    it.append(d);
  }
  it.onclick = () => {
    pick(name, key);
    list.focus();
  };
  if (x)
    it.oncontextmenu = (e) => {
      e.preventDefault();
      itemMenu(name, x);
    };
  return it;
}

/** Start (green) while a container is down, else Stop (red) while any runs; a spinner while Docker works. */
function drawBar(name: string, all: Item[]) {
  bar.replaceChildren();
  const busy = allSite === name ? allBusy : "";
  const action = busy || (all.some((x) => !up(x)) ? "start" : all.some(up) ? "stop" : "");
  if (!action) return;
  const label = busy ? (busy === "start" ? "Starting…" : "Stopping…") : action === "start" ? "Start" : "Stop";
  const b = barBtn(`dev-all dev-${action}`, busy ? el("span", "spin-dot") : icon(action), label);
  b.title = action === "start" ? "Start every container (docker compose up -d)" : "Stop every container (docker compose stop)";
  b.disabled = !!busy;
  b.onclick = () => runAll(name, action);
  bar.append(b);
}

/** Called whenever the app's processes are drawn, and when the tab is shown. */
export function drawContainers(name: string) {
  const root = $("#site");
  const panel = $('[data-panel="containers"]', root);
  if (!panel) return;
  const o = procCache.get(name);
  // A compose file that can't be read, with no container up, isn't the app's (see the Overview).
  const show = !!o?.containers;
  $('.tabs [data-tab="containers"]', root).hidden = !show;
  // A compose project's logs are its containers'; a plain folder writes none.
  const kind = snap?.sites.find((s) => s.name === name)?.kind ?? "";
  const noLogs = kind === "compose" || kind === "folder";
  $('.tabs [data-tab="logs"]', root).hidden = noLogs;
  if (noLogs && tab === "logs") showTab(root, show ? "containers" : "overview");
  if (o && !show && tab === "containers") showTab(root, "overview");
  if (!show) return panel.replaceChildren();

  const all = items(name);
  let key = containerSel.get(name) ?? "all";
  if (key !== "all" && !all.some((x) => x.key === key)) key = "all";
  if (view?.site !== name || view.key !== key) {
    view = { site: name, key, since: new Map(), seq: new Map(), asking: new Set(), styles: new Map() };
    term.out.replaceChildren();
    term.atEnd = true;
  }
  if (box.parentNode !== panel) {
    panel.replaceChildren(box);
    term.restore();
  }
  drawBar(name, all);

  const running = all.filter(up).length;
  list.replaceChildren(
    entry(
      name,
      null,
      running,
      all.some((x) => x.state === "crashed"),
    ),
    ...all.map((x) => entry(name, x)),
  );
  if (o?.docker_error) list.append(el("p", "dev-none", `Can’t read ${o.compose_file ?? "the compose file"}`));
  else if (!all.length) list.append(el("p", "dev-none", "The compose file has no services."));

  head.replaceChildren();
  const actions = el("span", "proc-actions");
  const one = all.find((x) => x.key === key);
  if (!one) head.append(el("span", "console-title", "All"), el("span", "dev-cmd", `${running} of ${all.length} running`));
  else {
    head.append(el("span", "console-title", one.name), el("span", "dev-cmd", one.busy ?? detail(one)));
    if (!one.busy && up(one)) {
      actions.append(procBtn("Restart", `Restart ${one.name}`, () => runOne(name, one, "restart")));
      actions.append(procBtn("■", `Stop ${one.name}`, () => runOne(name, one, "stop")));
    } else if (!one.busy) {
      const start = procBtn("▶ Start", `Start ${one.name}`, () => runOne(name, one, "start"));
      start.classList.add("dev-head-start");
      actions.append(start);
    }
  }
  head.append(actions);

  empty.replaceChildren();
  empty.classList.toggle("is-error", !!o?.docker_error && !one);
  if (o?.docker_error && !one) empty.append(el("span", null, o.docker_error));
  else if (one ? up(one) : running) empty.append(el("span", null, "No output yet"));
  else {
    empty.append(el("span", null, one?.busy ?? "Not running"));
    if (one && !one.busy) {
      const go = procBtn("▶", `Start ${one.name}`, () => runOne(name, one, "start"));
      go.title = `Start ${one.name}`;
      empty.append(go);
    }
  }
  syncEmpty();
}

/** New log lines for what is shown; All merges them by time. */
export async function containersTick() {
  const v = view;
  if (!v) return;
  const all = items(v.site);
  const shown = v.key === "all" ? all : all.filter((x) => x.key === v.key);
  const width = v.key === "all" ? Math.max(0, ...shown.map((x) => x.name.length)) : -1;
  following = v.site;
  const got = await Promise.all(
    shown.map(async (x) => {
      if (v.asking.has(x.key)) return [];
      v.asking.add(x.key);
      let raw = "";
      try {
        const r = await invoke("container_log_read", { site: v.site, service: x.name, after: v.seq.get(x.key) ?? 0 });
        raw = r.text;
        v.seq.set(x.key, r.next);
      } catch {
        return []; // Docker isn't running, or the container never ran: the list says so
      } finally {
        v.asking.delete(x.key);
      }
      const lines = stamped(raw, v.since.get(x.key) ?? null);
      if (lines.length) v.since.set(x.key, lines.at(-1)!.ts);
      return lines.map((l) => ({ ...l, x }));
    }),
  );
  if (v !== view) return;
  const lines = got.flat().sort((a, b) => (a.at < b.at ? -1 : a.at > b.at ? 1 : 0));
  if (!lines.length) return;
  const out = document.createDocumentFragment();
  for (const l of lines) {
    if (!v.styles.has(l.x.key)) v.styles.set(l.x.key, sgr());
    out.append(line(l.x, l.text, v.styles.get(l.x.key)!, width));
  }
  term.out.append(out);
  term.keep(() => trimLines(term.out, MAX_LINES));
  term.follow();
  syncEmpty();
}

late.drawContainers = drawContainers;
late.containersTab = () => {
  if (!selected) return;
  drawContainers(selected);
  containersTick();
  if (box.isConnected) list.focus({ preventScroll: true });
  // WebKit forgets a hidden pane's scroll position: put it back once it is laid out.
  requestAnimationFrame(term.restore);
};

// Only while the tab is shown: the logs every second, the containers' states every two.
// Once it is hidden or shows another app, Anvil's log streams for it end; shown again,
// they start over and the lines already on screen (by time) are skipped.
let ticks = 0;
setInterval(() => {
  const visible = shown() && mode === "apps" && !!selected && tab === "containers";
  if (following && (!visible || following !== selected)) {
    following = "";
    view?.seq.clear();
    invoke("container_logs_stop").catch(() => {});
  }
  if (!visible || !selected) return;
  if (view?.site === selected) containersTick();
  if (++ticks % 2 === 0 && !procLoading.has(selected)) loadProcs(selected);
}, 1000);
