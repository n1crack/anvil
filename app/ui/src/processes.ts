import type { AgentAction, Container, Overview, ProcessRow } from "./types";
import { $, ago, cardHead, el, invoke, mode, popAt, renderSite, selected, showError, showTab, snap, tab } from "./app";
import { late } from "./late";
import { clock, shown } from "./live";
import { keys } from "./platform";
import { silkSync } from "./silk";

/* An app's processes as the Overview and the header see them: the backend's overview
 * (rows, one-off scripts, Docker Compose containers), the header's Dev start/stop, a Dev
 * card with a row per process whose header opens the Dev tab (dev.ts), and the same card
 * for the containers, which opens the Containers tab (containers.ts). */

export const procCache = new Map<string, Overview>(); // site name -> overview from the backend
export const procLoading = new Set<string>(); // site names with a request on its way
export const procBusy = new Map<string, string>(); // "site/service" -> "starting…" while Docker works

/** Called by renderSite: draw what is known, then ask again. */
window.renderProcesses = (s) => {
  drawProcs(s.name);
  loadProcs(s.name);
  loadAgents(s.name);
};

/** The Overview's Agents group: the app's newest 5 actions from agents and the terminal,
 * hidden when there are none. Asked on every render, which `changed()` triggers. */
async function loadAgents(name: string) {
  let list: AgentAction[];
  try {
    list = await invoke("agent_activity", { site: name });
    showError(null, "agent_activity");
  } catch (e) {
    return showError(String(e), "agent_activity");
  }
  const box = $("#site .agents");
  if (selected !== name || !box) return;
  box.hidden = !list.length;
  $(".group", box).replaceChildren(
    ...list.slice(0, 5).map((a) => {
      const row = el("div", "row");
      const dt = el("dt");
      dt.append(el("span", "agent-line", `${a.client} ${a.what} · ${clock(a.at)}`));
      if (!a.ok && a.detail) dt.append(el("small", "agent-detail error-text", a.detail));
      row.append(dt);
      return row;
    }),
  );
}

let procAsked = 0; // numbers every request for an overview
const procShown = new Map<string, number>(); // site name -> newest number in procCache
const actShown = new Map<string, number>(); // the same, for actions only
const procAsking = new Map<string, number>(); // site name -> loadProcs calls on their way

/** `cmd`'s overview of the site into procCache, unless it is old news: a poll is dropped
 * when anything asked after it answered first (an action it started before), an action
 * only when a later action did. */
export async function askProcs(name: string, cmd: string, args = {}) {
  const n = ++procAsked;
  const o = await invoke(cmd, { site: name, ...args });
  const action = cmd !== "processes";
  if (n < ((action ? actShown : procShown).get(name) ?? 0)) return;
  if (action) actShown.set(name, n);
  procShown.set(name, Math.max(n, procShown.get(name) ?? 0));
  procCache.set(name, o);
}

/** Ask the backend; false when it failed (the error is shown). The site stays in
 * procLoading, which holds the polls back, until every call for it has answered. */
export async function loadProcs(name, cmd = "processes", args = {}) {
  const before = JSON.stringify(procCache.get(name));
  let ok = true;
  procLoading.add(name);
  procAsking.set(name, (procAsking.get(name) ?? 0) + 1);
  try {
    await askProcs(name, cmd, args);
    showError(null, cmd);
  } catch (e) {
    showError(String(e), cmd);
    ok = false;
  }
  procAsking.set(name, procAsking.get(name)! - 1);
  if (!procAsking.get(name)) procLoading.delete(name);
  // Redraw only on a change, so focus and tooltips survive the polling.
  if (selected === name && JSON.stringify(procCache.get(name)) !== before) drawProcs(name);
  return ok;
}

/** Docker can take minutes: the rows say what they are doing until it is done.
 * Without a service, the whole project in compose's order. */
export async function containerRun(name, services, action, service?: string) {
  const keys = services.map((s) => `${name}/${s}`);
  const word = { start: "starting…", stop: "stopping…", restart: "restarting…" }[action];
  for (const k of keys) procBusy.set(k, word);
  if (selected === name) drawProcs(name);
  const cmd = service ? "container_run" : "containers_run";
  try {
    await askProcs(name, cmd, { service, action });
    showError(null, cmd);
  } catch (e) {
    showError(String(e), cmd);
  }
  for (const k of keys) procBusy.delete(k);
  if (selected === name) drawProcs(name);
}

/** A button: `label` is its text, an icon or both ([icon, text]). */
export function procBtn(label, aria, onclick) {
  const b = el("button");
  b.append(...[label].flat());
  b.setAttribute("aria-label", String(aria));
  b.onclick = (e) => {
    e.stopPropagation();
    onclick(b);
  };
  return b;
}

/** A small confirm bubble under `anchor`: Enter or Run confirms, Esc or a click elsewhere cancels.
 * `more` are other answers, each a labelled button between Cancel and the main one. A long
 * question wraps, with the buttons on their own line. */
export function confirmPop(anchor, text, label, onYes, more: [string, () => void][] = [], align: "left" | "right" = "right") {
  const long = text.length > 70;
  const pop = el("div", long ? "pop-confirm wide" : "pop-confirm");
  const yes = el("button", "primary", label);
  const no = el("button", null, "Cancel");
  const others = more.map(([t, fn]) => {
    const b = el("button", null, t);
    b.onclick = (e) => {
      e.stopPropagation();
      close();
      fn();
    };
    return b;
  });
  pop.append(el(long ? "p" : "span", null, text), no, ...others, yes);
  const close = popAt(anchor, pop, yes, align);
  yes.onclick = (e) => {
    e.stopPropagation();
    close();
    onYes();
  };
  no.onclick = (e) => {
    e.stopPropagation();
    close();
  };
}

late.confirmPop = confirmPop;

/** A container as its dot shows it: running (green); starting (amber) while its health
 * check hasn't passed or Docker restarts it; crashed (red) when it exited non-zero. */
export function containerState(c: Container): "running" | "starting" | "crashed" | "stopped" {
  if (c.state === "running") return c.health === "starting" || c.health === "unhealthy" ? "starting" : "running";
  if (c.state === "restarting") return "starting";
  return c.state === "exited" && c.exit_code ? "crashed" : "stopped";
}
export const DOT = { running: "on", starting: "amber", crashed: "bad", stopped: "" };

/** The app's own server when it is a process, then every other process: what Dev runs. */
export const devRows = (o?: Overview | null) => (o?.rows ?? []).filter((r) => r.kind !== "site");

/** Dev start/stop for the shown app, offered by the Dev toolbar: Start starts the app and
 * every process in Dev start (and its containers), Stop stops them all. */
export const devAll = {
  site: "",
  start: false,
  stop: false,
  busy: "" as "" | "start" | "stop",
  run: async (_action: "start" | "stop", _size: { columns?: number; lines?: number } = {}) => {},
};

/** The header: how many run (a click opens Dev), and what Dev start/stop would do.
 * A PHP site alone says whether it's served. */
function drawRunBox(name, rows, containers) {
  const box = $("#site .run-box");
  if (!box) return;
  const up = (c) => c.state === "running";
  const own = rows.filter((r) => r.kind !== "site");
  const total = own.length + containers.length;
  const running = own.filter((r) => r.running).length + containers.filter(up).length;
  const crashed = own.filter((r) => r.crashed && !r.running).length + containers.filter((c) => containerState(c) === "crashed").length;
  box.hidden = !rows.length && !containers.length;
  const text = $(".state-text", box);
  // A click on the state opens the Dev tab, or the Containers tab when there is no Dev.
  $(".state", box).onclick = () => {
    const t = ["dev", "containers"].find((t) => !$(`#site .tabs [data-tab="${t}"]`).hidden);
    if (t) showTab($("#site"), t);
  };
  if (devAll.site !== name) devAll.busy = "";
  devAll.site = name;
  if (!total) {
    const serving = rows.some((r) => r.running);
    $(".dot", box).className = `dot ${serving ? "on" : ""}`;
    text.textContent = serving ? "Serving" : "Not serving";
    text.classList.remove("error-text");
    devAll.start = devAll.stop = false;
    $(".state", box).onmouseenter = null;
    return;
  }
  $(".dot", box).className = `dot ${crashed ? "bad" : running ? "on" : ""}`;
  text.textContent = [running && `${running} running`, crashed && `${crashed} crashed`].filter(Boolean).join(", ") || "Stopped";
  text.classList.toggle("error-text", !!crashed);
  // Done when everything Dev start would start runs; processes left out don't count.
  devAll.start = !(own.every((r) => r.running || r.manual) && containers.every(up));
  devAll.stop = !!running;
  devAll.run = async (action, size = {}) => {
    devAll.busy = action;
    late.drawDev?.(name);
    const change = containers.filter((c) => up(c) !== (action === "start")).map((c) => c.service);
    await Promise.all([
      loadProcs(name, "process_run", { name: null, action, ...size }),
      change.length && containerRun(name, change, action),
    ]);
    if (devAll.site === name) devAll.busy = "";
    late.drawDev?.(name);
  };
  // Hover the state: which run (and which crashed), with their address or command.
  const shown = [
    ...own
      .filter((r) => r.running || r.crashed)
      .map((r) => [r.running ? "on" : "bad", r.name, r.address ?? (r.port ? `:${r.port}` : r.command)]),
    ...containers.filter(up).map((c) => ["on", c.service, "container"]),
  ];
  const state = $(".state", box);
  state.onmouseenter = () => {
    clearTimeout(box.hideTimer);
    if (!shown.length || $(".pop.run-pop")) return;
    const pop = el("div", "run-pop");
    for (const [cls, n, where] of shown) {
      const line = el("div", "run-pop-row");
      line.append(el("span", `dot ${cls}`), el("span", "run-pop-name", n), el("span", "run-pop-where", where ?? ""));
      pop.append(line);
    }
    pop.onmouseenter = () => clearTimeout(box.hideTimer);
    pop.onmouseleave = state.onmouseleave;
    box.closePop = popAt(state, pop);
  };
  state.onmouseleave = () => {
    clearTimeout(box.hideTimer);
    box.hideTimer = setTimeout(() => box.closePop?.(), 250);
  };
}

/** How long a process has run: "m:ss", "h:mm:ss" from an hour. */
export function uptime(sec: number) {
  const s = Math.max(0, Math.floor(sec));
  const mm = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return s < 3600 ? `${mm}:${ss}` : `${Math.floor(s / 3600)}:${String(mm).padStart(2, "0")}:${ss}`;
}

/** A card row: dot class, name, command, the right column, crashed. */
type Entry = [string, string, string, Node[], boolean];

/** A card: its header (title, counts or what went wrong) opens `tab`; one row per entry. */
function runCard(title: string, state: Node, rows: Entry[], tab: string, keys: string) {
  const card = el("div", "run-card");
  const head = cardHead(title, state, tab);
  const label = tab === "dev" ? "Dev" : "Containers";
  head.title = keys ? `Show the ${label} tab (${keys})` : `Show the ${label} tab`;
  card.append(head);
  for (const [cls, name, cmd, end, crashed] of rows) {
    const row = el("div", crashed ? "proc crashed" : "proc");
    const d = el("span", `dot ${cls}`);
    // The state in words too, not only the colour.
    d.setAttribute("role", "img");
    d.setAttribute("aria-label", `${name}: ${Object.keys(DOT).find((k) => DOT[k] === cls) ?? "stopped"}`);
    const n = el("span", "proc-name", name);
    const c = el("span", "cmd", cmd);
    n.title = name;
    c.title = cmd;
    const right = el("span", "proc-end");
    right.append(...end);
    row.append(d, n, c, right);
    card.append(row);
  }
  return card;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** A Dev row's right column: its uptime, how it ended with Restart, or its state. */
function procEnd(name: string, r: ProcessRow): Node[] {
  if (r.running && r.started) {
    const up = el("span", "up", uptime(Date.now() / 1000 - r.started));
    up.dataset.started = String(r.started);
    return [up];
  }
  // Running without a start time (no pid of ours, e.g. the site Caddy serves): no uptime.
  if (!r.crashed || r.running) return [el("span", "up", r.starting ? "starting" : r.running ? "running" : "stopped")];
  const how = r.crashed.code == null ? "ended by a signal" : `exited ${r.crashed.code}`;
  return [
    el("span", "up bad", `${how} · ${ago(new Date(r.crashed.at * 1000))}`),
    procBtn("Restart", `Restart ${r.name}`, () => late.restartProc?.(name, r.id)),
  ];
}

/** Something of the app is live: a process or container runs or has crashed (the site's own
 * row counts once Caddy serves it). The address alone doesn't: a folder stays cold. */
const isAlive = (o?: Overview | null) =>
  !!o?.rows.some((r) => r.running || r.crashed) || !!o?.containers?.some((c) => c.state === "running" || containerState(c) === "crashed");

export function drawProcs(name) {
  const box = $("#site .procs");
  if (!box) return;
  const o = procCache.get(name);
  $("#site").classList.toggle("alive", isAlive(o));
  silkSync();
  const rows = o?.rows ?? [];
  const dev = devRows(o);
  const containers = o?.containers ?? [];
  const compose = !!(o?.containers || o?.docker_error);
  drawRunBox(name, rows, containers);
  late.drawDev?.(name);
  late.drawContainers?.(name);
  box.hidden = !(dev.length || compose);
  box.replaceChildren();
  if (box.hidden) return syncUptime();
  const devTab = !$('#site .tabs [data-tab="dev"]')?.hidden;
  const counts = (running: number, crashed: number) =>
    el(
      "span",
      "run-card-count",
      [running && `${running} running`, crashed && `${crashed} crashed`].filter(Boolean).join(" · ") || "none running",
    );
  if (dev.length) {
    const running = dev.filter((r) => r.running).length;
    const crashed = dev.filter((r) => r.crashed && !r.running).length;
    const rows = dev.map((r): Entry => {
      const cls = r.running ? (r.starting ? "amber" : "on") : r.crashed ? "bad" : "";
      return [cls, r.name, r.command, procEnd(name, r), cls === "bad"];
    });
    box.append(runCard(`Dev · ${plural(dev.length, "process", "processes")}`, counts(running, crashed), rows, "dev", keys("⌘3")));
  }
  const error = o?.docker_error;
  if (error && !containers.length) box.append(composeError(error, o!.compose_file));
  else if (compose) {
    const states = containers.map(containerState);
    const state = error
      ? el("span", "run-card-count error-text", composeBrief(error, o!.compose_file))
      : counts(containers.filter((c) => c.state === "running").length, states.filter((s) => s === "crashed").length);
    const rows = containers.map((c, i): Entry => {
      const s = states[i];
      const word = s === "crashed" ? `exited ${c.exit_code}` : s;
      return [
        DOT[s],
        c.service,
        c.ports.map((p) => `:${p}`).join(" "),
        [el("span", s === "crashed" ? "up bad" : "up", word)],
        s === "crashed",
      ];
    });
    const card = runCard(`Containers · ${containers.length}`, state, rows, "containers", devTab ? "" : keys("⌘3"));
    if (error) card.firstElementChild!.setAttribute("title", error);
    box.append(card);
  }
  syncUptime();
}

/** The Dev card's uptimes tick once a second, in place: one timer, alive only while the
 * Overview shows a card with a running process. Called on every draw and tab change. */
let uptimeTimer = 0;
export function syncUptime() {
  const want = mode === "apps" && tab === "overview" && !!$("#site .run-card [data-started]");
  if (want && !uptimeTimer) uptimeTimer = window.setInterval(tickUptime, 1000);
  else if (!want && uptimeTimer) {
    window.clearInterval(uptimeTimer);
    uptimeTimer = 0;
  }
}

function tickUptime() {
  syncUptime();
  const now = Date.now() / 1000;
  for (const u of document.querySelectorAll<HTMLElement>("#site .run-card [data-started]")) {
    u.textContent = uptime(now - Number(u.dataset.started));
  }
}

late.syncUptime = syncUptime;

/** What a compose error says in one line: a short one as it is ("Docker isn't running"),
 * else that the file can't be read; the whole message is its tooltip. */
const composeBrief = (error: string, file?: string | null) =>
  error.includes("\n") || error.length > 40 ? `can’t read ${file ?? "the compose file"}` : error;

/** The compose file can't be read and no container runs: one muted line (no tab to
 * open); a click shows the message and its hint below it. */
function composeError(error: string, file?: string | null) {
  const wrap = el("div", "dev-summary-wrap");
  const b = el("button", "dev-summary is-muted");
  b.append(el("span", "dev-summary-title", "Containers"), el("span", "dev-summary-state", composeBrief(error, file)));
  b.title = error;
  b.setAttribute("aria-expanded", "false");
  const detail = el("p", "dev-summary-detail", error);
  detail.hidden = true;
  b.onclick = () => {
    detail.hidden = !detail.hidden;
    b.setAttribute("aria-expanded", String(!detail.hidden));
  };
  wrap.append(b, detail);
  return wrap;
}

// While the Overview is shown, keep the Dev and Containers lines fresh.
setInterval(() => {
  if (!shown() || mode !== "apps" || !selected || tab !== "overview") return;
  const o = procCache.get(selected);
  if (procLoading.has(selected) || !(devRows(o).length || o?.containers || o?.docker_error)) return;
  loadProcs(selected);
}, 2000);

// The first state may arrive before this file loads.
if (snap && mode === "apps") renderSite();
