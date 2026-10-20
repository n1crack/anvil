import "./platform";
import type { Activity, AgentAction } from "./types";
import { follower } from "./console";
import { splitter } from "./split";
import { clock, ownError, shown } from "./live";
import { type Sgr, clean, colorize, sgr } from "./term";

/* The Processes window: everything Anvil runs, with usage, Restart/Stop (right-click)
 * and the selected row's log. Keys come from `activity`; the log is asked for by key. */
export const { invoke } = window.__TAURI__.core;

export const $ = (sel: string, root: ParentNode = document): any => root.querySelector(sel);
export let data: Activity | null = null;
export let drawn = ""; // what the table shows, to redraw only on a change
export let selected: string | null = null; // row key
let logFrom = 0;
let logToken = 0;
export let tailing: number | null = null; // token of the log request on its way
export let replace = true; // the next page replaces the log (new row, error shown, file rotated)
export const busy = new Set<string>(); // keys with an action on its way
// Folded application groups, kept across windows like the main list's folds.
export const folded = new Set(JSON.parse(localStorage.getItem("act-folded") ?? "[]"));
// Which tab is shown: apps, services, anvil (Anvil's own: Caddy, PHP-FPM, DNS…) or agents
// (what agents and the terminal did); the last one comes back. "web" was anvil's name before.
const TABS = ["apps", "services", "anvil", "agents"];
let agents: AgentAction[] = []; // newest first, from `agent_activity`
export const storedTab = (v: string | null) => (v === "web" ? "anvil" : TABS.includes(v!) ? v! : "apps");
export let tab = storedTab(localStorage.getItem("act-tab"));
let logStyle: Sgr | undefined; // colors carried from one page of the log to the next
const logView = follower($("#log")); // follows the end while the reader is there

const errors = new Map<string, string>(); // owner -> its error, see ownError
export function showError(msg, owner = "") {
  const text = ownError(errors, msg, owner);
  $("#error").textContent = text;
  $("#error").hidden = !text;
}

export let loading: Promise<void> | null = null; // the `activity` request on its way: a slow Docker must not stack them

export function load() {
  loading ??= (async () => {
    try {
      [data, agents] = await Promise.all([invoke("activity"), invoke("agent_activity", { site: null })]);
      showError(null, "activity");
      showError(data?.docker_error, "docker");
    } catch (e) {
      return showError(String(e), "activity");
    } finally {
      loading = null;
    }
    render();
  })();
  return loading;
}

window.refresh = load;

/** 9.4 MB, 136 MB, 1.2 GB: as many digits as help to compare. */
const mb = (b) => {
  const m = b / 1048576;
  return m >= 1024 ? `${(m / 1024).toFixed(1)} GB` : `${m < 10 ? m.toFixed(1) : Math.round(m)} MB`;
};
export const status = { running: "Running", starting: "Starting", crashed: "Crashed" };

export function td(text, cls?: string) {
  const c = document.createElement("td");
  if (cls) c.className = cls;
  c.textContent = text ?? "";
  return c;
}

export function head(text, cls) {
  const tr = document.createElement("tr");
  tr.className = cls;
  const th = document.createElement("th");
  th.colSpan = 6;
  th.textContent = text;
  tr.append(th);
  return tr;
}

/** An application's header: click anywhere on it to fold its rows away. Folded, it
 * says what it hides on the same line: "2 running · dev, docs:dev". */
function appHead(a) {
  const tr = head("", "act-app");
  const open = !folded.has(a.site);
  tr.setAttribute("aria-expanded", String(open));
  (tr.firstChild as HTMLElement).innerHTML =
    `<svg class="chevron" viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 2.5 8 6l-3.5 3.5"/></svg><span class="act-site"></span><span class="act-sum"></span>`;
  $(".act-site", tr).textContent = a.site;
  if (!open) {
    const sum = $(".act-sum", tr);
    const counts = ["running", "starting", "crashed"].map((st) => [st, a.rows.filter((r) => r.state === st).length]).filter(([, n]) => n);
    counts.forEach(([st, n], i) => {
      const c = document.createElement("span");
      c.className = `act-sum-${st}`;
      c.textContent = `${n} ${st}`;
      sum.append(i ? ", " : "", c);
    });
    sum.append(`${counts.length ? " · " : ""}${a.rows.map((r) => r.name).join(", ")}`);
  }
  tr.onclick = () => {
    folded[open ? "add" : "delete"](a.site);
    localStorage.setItem("act-folded", JSON.stringify([...folded]));
    drawn = "";
    render();
  };
  return tr;
}

/** Which command a row's action goes to, by key form. */
export function run(key, action) {
  if (key.startsWith("service:")) return invoke("service_run", { id: key.slice(8), action });
  if (key.startsWith("container:")) {
    const [site, service] = key.slice(10).split("/");
    return invoke("container_run", { site, service, action });
  }
  if (key.includes("#")) {
    const [site, id] = key.split("#");
    return invoke("once_stop", { site, id });
  }
  if (key.includes("@")) {
    const [site, name] = key.split("@");
    return invoke("process_run", { site, name, action });
  }
  return invoke("app_run", { site: key, action });
}

export async function act(key, action?) {
  busy.add(key);
  render();
  try {
    await run(key, action);
    showError(null, "act");
  } catch (e) {
    showError(String(e), "act");
  }
  busy.delete(key);
  // A restart begins a new log (see `Apps::spawn`): read it from the start.
  if (key === selected && action === "restart") select(key);
  // A poll that began before the action ended would show the old state.
  await loading;
  await load();
}

export const M = window.__TAURI__.menu;

/** Right-click: Restart, Stop (Dismiss once it crashed) and Open in Browser, by row state. */
async function rowMenu(r) {
  const items: any[] = [];
  if (r.actions && !busy.has(r.key)) {
    if (!r.key.includes("#")) items.push(M.MenuItem.new({ text: "Restart", action: () => act(r.key, "restart") }));
    items.push(M.MenuItem.new({ text: r.state === "crashed" ? "Dismiss" : "Stop", action: () => act(r.key, "stop") }));
  }
  if (r.web && r.state === "running") {
    if (items.length) items.push(M.PredefinedMenuItem.new({ item: "Separator" }));
    items.push(
      M.MenuItem.new({ text: "Open in Browser", action: () => invoke("open_url", { url: r.web }).catch((e) => showError(String(e))) }),
    );
  }
  if (items.length) (await M.Menu.new({ items: await Promise.all(items) })).popup();
}

const cpuText = (r) => (r.cpu == null ? "" : `${r.cpu.toFixed(1)}%`);
const memText = (r) => (r.memory == null ? "" : mb(r.memory));

export function row(r, inner?: boolean) {
  const tr = document.createElement("tr");
  tr.className = `act-row ${r.state}${inner ? " inner" : ""}`;
  tr.setAttribute("aria-selected", String(r.key === selected));
  tr.dataset.key = r.key;
  // Control-click is a right-click on macOS: the menu only, the selection stays.
  tr.onclick = (e) => !e.ctrlKey && select(r.key);
  tr.oncontextmenu = (e) => {
    e.preventDefault();
    rowMenu(r).catch((err) => showError(String(err)));
  };
  const name = td(r.name, "act-name");
  if (r.detail && r.state !== "crashed") {
    const d = document.createElement("span");
    d.className = "act-detail";
    d.textContent = r.detail;
    name.append(" ", d);
  }
  // A dot and a short word; a crash keeps its exit detail, muted and as the dot's tooltip.
  const st = td(null, "act-status");
  const dot = document.createElement("span");
  dot.className = `dot ${{ running: "on", starting: "amber", crashed: "bad" }[r.state] ?? ""}`;
  st.append(dot, status[r.state] ?? r.state);
  if (r.state === "crashed" && r.detail) {
    dot.title = r.detail;
    const d = document.createElement("span");
    d.className = "act-detail";
    d.textContent = r.detail;
    st.append(" ", d);
  }
  tr.append(name, td(r.port, "num"), td(r.pid, "num"), td(cpuText(r), "num"), td(memText(r), "num"), st);
  return tr;
}

/** The rows of each tab, for the counts and the table. */
const tabRows = (d: Activity) => ({ apps: d.apps.flatMap((a) => a.rows), services: d.services, anvil: d.web });

export function render() {
  if (!data) return;
  const rows = tabRows(data);
  const keys = Object.values(rows).flatMap((rs) => rs.map((r) => r.key));
  if (selected && !keys.includes(selected)) select(null);
  // Redraw only on a change, so hover and focus survive the polling; usage changes all
  // the time, so it is left out and written into its cells.
  const now = JSON.stringify([data, agents, selected, [...busy], tab], (k, v) => (k === "cpu" || k === "memory" ? undefined : v));
  if (now === drawn) return tab === "agents" || usage(rows[tab]);
  drawn = now;
  for (const b of document.querySelectorAll("#tabs [role=tab]") as NodeListOf<HTMLElement>) {
    b.setAttribute("aria-selected", String(b.dataset.tab === tab));
    const n = rows[b.dataset.tab!]?.length;
    const c = $(".act-count", b);
    if (c) c.textContent = n ? String(n) : "";
  }
  $("#empty").textContent = tab === "agents" ? "No actions from agents or the terminal yet" : "Nothing running";
  $("#agents").hidden = tab !== "agents" || !agents.length;
  if (tab === "agents") {
    $("#agent-rows").replaceChildren(...agents.map(agentRow));
    $("#procs").hidden = true;
    $("#empty").hidden = agents.length > 0;
    return;
  }
  const out: any[] = [];
  if (tab === "anvil") out.push(...data.web.map((r) => row(r)));
  if (tab === "services") out.push(...data.services.map((r) => row(r)));
  if (tab === "apps") {
    for (const a of data.apps) {
      out.push(appHead(a));
      if (!folded.has(a.site)) out.push(...a.rows.map((r) => row(r, true)));
    }
  }
  $("#rows").replaceChildren(...out);
  $("#empty").hidden = rows[tab].length > 0;
  $("#procs").hidden = !rows[tab].length;
}

/** "Claude Code restarted api", its app and time; a failure says why, inline. Not a link. */
function agentRow(a: AgentAction) {
  const tr = document.createElement("tr");
  const what = td(`${a.client} ${a.what}`, "act-name");
  if (!a.ok && a.detail) {
    const d = document.createElement("span");
    d.className = "act-detail agent-detail error-text";
    d.textContent = a.detail;
    what.append(" ", d);
  }
  tr.append(what, td(a.site ?? ""), td(clock(a.at), "num"));
  return tr;
}

/** CPU and memory into the drawn rows, only the cells whose text changed. */
function usage(rows) {
  const trs = new Map([...document.querySelectorAll<HTMLTableRowElement>("#rows tr.act-row")].map((tr) => [tr.dataset.key, tr]));
  for (const r of rows) {
    const tr = trs.get(r.key);
    if (!tr) continue;
    const [cpu, mem] = [tr.cells[3], tr.cells[4]];
    if (cpu.textContent !== cpuText(r)) cpu.textContent = cpuText(r);
    if (mem.textContent !== memText(r)) mem.textContent = memText(r);
  }
}

/** Show a tab; the log of the selected row stays open whichever tab is shown. */
export function showTab(name) {
  tab = name;
  localStorage.setItem("act-tab", name);
  render();
}

for (const b of document.querySelectorAll("#tabs [role=tab]") as NodeListOf<HTMLElement>) b.onclick = () => showTab(b.dataset.tab);

export function select(key) {
  selected = key;
  logFrom = 0;
  replace = true;
  logStyle = undefined;
  logToken++;
  $("#log").textContent = "";
  logView.end();
  $("#pane").hidden = $("#split").hidden = !key;
  $("#log-title").textContent = key ? `${nameOf(key)} · log` : "";
  // A container's log is Docker's: only Anvil's own log files can be emptied.
  $("#log-clear").hidden = !key || key.startsWith("container:");
  render();
  if (key) tail();
}

const nameOf = (key) =>
  (data ? [...data.web, ...data.apps.flatMap((a) => a.rows), ...data.services] : []).find((r) => r.key === key)?.name ?? key;

export async function tail() {
  const key = selected;
  const token = logToken;
  const log = $("#log");
  let page;
  tailing = token;
  try {
    page = await invoke("activity_log", { key, from: logFrom });
  } catch (e) {
    if (token === logToken) {
      log.textContent = String(e);
      logFrom = 0; // recovery reads the last page again, not only what came after
      replace = true;
    }
    return;
  } finally {
    if (tailing === token) tailing = null;
  }
  if (token !== logToken) return;
  // A file that shrank (cleared or rotated): read it again from the start.
  if (page.end_offset < logFrom) {
    logFrom = 0;
    replace = true;
    return tail();
  }
  // Containers come back whole (empty when unchanged); files only send what is new.
  if (replace || (key?.startsWith("container:") && page.text)) {
    log.replaceChildren();
    logStyle = undefined;
    replace = false;
  }
  if (page.text) {
    // A page at a time, so the oldest can go a page at a time.
    const p = document.createElement("span");
    p.append(colorize(clean(page.text, true), (logStyle ??= sgr())));
    (p as any).lines = page.text.split("\n").length - 1;
    log.append(p);
    logView.keep(() => trimLog(log));
  }
  logFrom = page.end_offset;
  logView.follow();
}

export const LOG_LINES = 3000; // about this many newest lines stay in the log

/** Whole pages out, the oldest first, while the rest still has LOG_LINES: in one removal. */
export function trimLog(log: HTMLElement) {
  const pages = [...log.children] as any[];
  let left = pages.reduce((n, p) => n + (p.lines ?? 0), 0);
  let k = 0;
  while (k < pages.length - 1 && left - pages[k].lines >= LOG_LINES) left -= pages[k++].lines;
  if (!k) return;
  const r = document.createRange();
  r.setStartBefore(pages[0]);
  r.setEndBefore(pages[k]);
  r.deleteContents();
}

/** Show one row with its log, on its tab: a service page's log button asks for it, as
 * `window.activitySelect` when it opens this window, then by the event. */
export async function reveal(key: string) {
  showTab(key.startsWith("service:") ? "services" : key.startsWith("web:") ? "anvil" : "apps");
  // A service started a moment ago may not be in the last poll yet.
  await load();
  select(key);
}
window.__TAURI__.event?.listen("activity-select", (e) => reveal(e.payload));
if (window.activitySelect) reveal(window.activitySelect);

// Fresh while the window is shown, focused or not.
setInterval(() => {
  if (!shown()) return;
  load();
  if (selected && tailing !== logToken) tail();
}, 2000);
window.addEventListener("focus", load);
$("#log-clear").onclick = async () => {
  const key = selected;
  if (!key) return;
  try {
    await invoke("activity_log_clear", { key });
  } catch (e) {
    $("#log").textContent = String(e);
    return;
  }
  if (selected === key) select(key); // read again from the now empty start
};
$("#log-close").onclick = () => select(null);

// The splitter: drag to give the log more or less room; the width is remembered.
const pane = $("#pane");
const split = $("#split");
splitter(split, {
  key: "act-log-width",
  min: 260,
  max: () => $(".act-main").clientWidth - 360 - split.offsetWidth,
  at: (m) => $(".act-main").getBoundingClientRect().right - m.clientX - split.offsetWidth / 2,
  apply: (w) => (pane.style.width = w ? `${w}px` : ""),
});

// No browser context menu (Reload, Inspect…), as in the other windows.
document.addEventListener("contextmenu", (e) => e.preventDefault());

load();
