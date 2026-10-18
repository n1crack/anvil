import type { Container, Overview } from "./types";
import { $, el, invoke, mode, popAt, renderSite, selected, showError, showTab, snap, tab } from "./app";
import { late } from "./late";
import { shown } from "./live";

/* An app's processes as the Overview and the header see them: the backend's overview
 * (rows, one-off scripts, Docker Compose containers), the header's Dev start/stop, one
 * "Dev · 3 of 4 running" line that opens the Dev tab (dev.ts), and the same line for the
 * containers, which opens the Containers tab (containers.ts). */

export const procCache = new Map<string, Overview>(); // site name -> overview from the backend
export const procLoading = new Set<string>(); // site names with a request on its way
export const procBusy = new Map<string, string>(); // "site/service" -> "starting…" while Docker works

/** Called by renderSite: draw what is known, then ask again. */
window.renderProcesses = (s) => {
  drawProcs(s.name);
  loadProcs(s.name);
};

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

export function procBtn(label, aria, onclick) {
  const b = el("button", null, label);
  b.setAttribute("aria-label", String(aria));
  b.onclick = (e) => {
    e.stopPropagation();
    onclick(b);
  };
  return b;
}

/** A small confirm bubble under `anchor`: Enter or Run confirms, Esc or a click elsewhere cancels. */
export function confirmPop(anchor, text, label, onYes) {
  const pop = el("div", "pop-confirm");
  const yes = el("button", "primary", label);
  const no = el("button", null, "Cancel");
  pop.append(el("span", null, text), no, yes);
  const close = popAt(anchor, pop, yes);
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
  run: async (_action: "start" | "stop") => {},
};

/** The header: how many run (a click opens Dev), and what Dev start/stop would do.
 * A PHP site alone says whether it's served. */
export function drawRunBox(name, rows, containers) {
  const box = $("#site .run-box");
  if (!box) return;
  const up = (c) => c.state === "running";
  const own = rows.filter((r) => r.kind !== "site");
  const total = own.length + containers.length;
  const running = own.filter((r) => r.running).length + containers.filter(up).length;
  const crashed = own.filter((r) => r.crashed && !r.running).length;
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
  devAll.run = async (action) => {
    devAll.busy = action;
    late.drawDev?.(name);
    const change = containers.filter((c) => up(c) !== (action === "start")).map((c) => c.service);
    await Promise.all([loadProcs(name, "process_run", { name: null, action }), change.length && containerRun(name, change, action)]);
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

/** "Dev · 3 of 4 running" with a dot per entry (or what went wrong); a click opens `tab`. */
export function summaryLine(title: string, dots: [string, string][], running: number, tab: string, keys: string, error?: string | null) {
  const b = el("button", "dev-summary");
  const box = el("span", "dev-summary-dots");
  for (const [name, cls] of dots) {
    const d = el("span", `dot ${cls}`);
    d.title = name;
    box.append(d);
  }
  const state = el("span", `dev-summary-state${error ? " error-text" : ""}`, error ?? `${running} of ${dots.length} running`);
  b.append(el("span", "dev-summary-title", title), state, box);
  b.title = keys ? `Show the ${title} tab (${keys})` : `Show the ${title} tab`;
  b.onclick = () => showTab($("#site"), tab);
  return b;
}

export function drawProcs(name) {
  const box = $("#site .procs");
  if (!box) return;
  const o = procCache.get(name);
  const rows = o?.rows ?? [];
  const dev = devRows(o);
  const containers = o?.containers ?? [];
  const compose = !!(o?.containers || o?.docker_error);
  drawRunBox(name, rows, containers);
  late.drawDev?.(name);
  late.drawContainers?.(name);
  box.hidden = !(dev.length || compose);
  box.replaceChildren();
  if (box.hidden) return;
  const devTab = !$('#site .tabs [data-tab="dev"]')?.hidden;
  if (dev.length) {
    const dots: [string, string][] = dev.map((r) => [r.name, r.running ? "on" : r.crashed ? "bad" : ""]);
    box.append(summaryLine("Dev", dots, dev.filter((r) => r.running).length, "dev", "⌘3"));
  }
  if (compose) {
    const dots: [string, string][] = containers.map((c) => [c.service, DOT[containerState(c)]]);
    const up = containers.filter((c) => c.state === "running").length;
    box.append(summaryLine("Containers", dots, up, "containers", devTab ? "" : "⌘3", o!.docker_error));
  }
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
