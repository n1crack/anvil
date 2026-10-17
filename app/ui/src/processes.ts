import type { Overview } from "./types";
import { $, el, invoke, mode, popAt, renderSite, selected, showError, showTab, snap, tab } from "./app";
import { late } from "./late";

/* An app's processes as the Overview and the header see them: the backend's overview
 * (rows, one-off scripts, Docker Compose containers), the header's Dev start/stop, one
 * "Dev · 3 of 4 running" line that opens the Dev tab (dev.ts), and the containers. */

export const procCache = new Map<string, Overview>(); // site name -> overview from the backend
export const procLoading = new Set<string>(); // site names with a request on its way
export const procBusy = new Map<string, string>(); // "site/service" -> "starting…" while Docker works
export const procLogs = new Map<string, string>(); // "site/service" -> container log text, while shown

/** Called by renderSite: draw what is known, then ask again. */
window.renderProcesses = (s) => {
  drawProcs(s.name);
  loadProcs(s.name);
};

/** Ask the backend; false when it failed (the error is shown). */
export async function loadProcs(name, cmd = "processes", args = {}) {
  const before = JSON.stringify(procCache.get(name));
  let ok = true;
  procLoading.add(name);
  try {
    procCache.set(name, await invoke(cmd, { site: name, ...args }));
  } catch (e) {
    showError(String(e));
    ok = false;
  }
  procLoading.delete(name);
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
  try {
    procCache.set(name, await invoke(service ? "container_run" : "containers_run", { site: name, service, action }));
  } catch (e) {
    showError(String(e));
  }
  for (const k of keys) procBusy.delete(k);
  if (selected === name) drawProcs(name);
}

export async function toggleContainerLog(name, service) {
  const key = `${name}/${service}`;
  if (procLogs.delete(key)) return drawProcs(name);
  procLogs.set(key, "Loading…");
  drawProcs(name);
  try {
    procLogs.set(key, (await invoke("container_log", { site: name, service })) || "No output yet");
  } catch (e) {
    procLogs.set(key, String(e));
  }
  if (selected === name && procLogs.has(key)) drawProcs(name);
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

/** A container: dot, name, detail, then Restart and ■ (or ▶). A click shows its log. */
function containerRow({ name, state, detail, busy, open, run }: Record<string, any>) {
  const row = el("div", `proc ${state}`);
  const dot = el("span", `dot ${state === "running" ? "on" : ""}`);
  dot.title = state === "running" ? "Running" : "Stopped";
  const label = el("button", "proc-name", name);
  label.title = "Click to show its log";
  row.onclick = open;
  const last = el("span", "proc-last", busy ?? detail);
  last.title = last.textContent;
  row.append(dot, label, last);
  const actions = el("span", "proc-actions");
  if (!busy) {
    if (state !== "stopped") actions.append(procBtn("Restart", `Restart ${name}`, () => run("restart")));
    if (state !== "stopped") actions.append(procBtn("■", `Stop ${name}`, () => run("stop")));
    if (state === "stopped") actions.append(procBtn("▶", `Start ${name}`, () => run("start")));
  }
  row.append(actions);
  return row;
}

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
  // A click on the state opens the Dev tab, when there is one.
  $(".state", box).onclick = () => {
    if (!$('#site .tabs [data-tab="dev"]').hidden) showTab($("#site"), "dev");
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

/** "Dev · 3 of 4 running" with a dot per process; a click opens the Dev tab. */
function devLine(rows) {
  const b = el("button", "dev-summary");
  const running = rows.filter((r) => r.running).length;
  const dots = el("span", "dev-summary-dots");
  for (const r of rows) {
    const d = el("span", `dot ${r.running ? "on" : r.crashed ? "bad" : ""}`);
    d.title = r.name;
    dots.append(d);
  }
  b.append(el("span", "dev-summary-title", "Dev"), el("span", "dev-summary-state", `${running} of ${rows.length} running`), dots);
  b.title = "Show the Dev tab (⌘3)";
  b.onclick = () => showTab($("#site"), "dev");
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
  box.hidden = !(dev.length || compose);
  box.replaceChildren();
  if (box.hidden) return;
  if (dev.length) box.append(devLine(dev));
  if (!compose) return;
  box.append(el("h3", "res-sub", "Containers"));
  if (o!.docker_error) {
    const note = el("div", "procs-containers");
    note.append(el("p", "procs-note", o!.docker_error));
    return box.append(note);
  }
  const list = el("div", "group procs-list procs-containers");
  const up = (c) => c.state === "running";
  for (const c of containers) {
    const key = `${name}/${c.service}`;
    list.append(
      containerRow({
        name: c.service,
        state: up(c) ? "running" : "stopped",
        detail: [...c.ports.map((p) => `:${p}`), c.health ?? c.state].join(" · "),
        busy: procBusy.get(key),
        open: () => toggleContainerLog(name, c.service),
        run: (action) => containerRun(name, [c.service], action, c.service),
      }),
    );
    if (procLogs.has(key)) list.append(el("pre", "proc-log", procLogs.get(key)));
  }
  box.append(list);
}

// While the Overview is shown, keep the Dev line and the containers fresh.
setInterval(() => {
  if (document.hidden || mode !== "apps" || !selected || tab !== "overview") return;
  const o = procCache.get(selected);
  if (procLoading.has(selected) || !(devRows(o).length || o?.containers || o?.docker_error)) return;
  loadProcs(selected);
}, 2000);

// The first state may arrive before this file loads.
if (snap && mode === "apps") renderSite();
