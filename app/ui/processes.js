/* Processes: an app's queue worker, log tailer, extra dev servers, one-off commands
 * and Docker Compose containers, in the Overview. Each row starts and stops on its
 * own. Shares $, el, invoke, snap, selected, tab, mode, toast and showError with app.js. */

const procCache = new Map(); // site name -> overview from the backend
const procLoading = new Set(); // site names with a request on its way
const procBusy = new Map(); // "site/service" -> "starting…" while Docker works
const procLogs = new Map(); // "site/service" -> container log text, while shown

/** Called by renderSite: draw what is known, then ask again. */
window.renderProcesses = (s) => {
  drawProcs(s.name);
  loadProcs(s.name);
};

async function loadProcs(name, cmd = "processes", args = {}) {
  const before = JSON.stringify(procCache.get(name));
  procLoading.add(name);
  try {
    procCache.set(name, await invoke(cmd, { site: name, ...args }));
  } catch (e) {
    showError(String(e));
  }
  procLoading.delete(name);
  // Redraw only on a change, so focus and tooltips survive the polling.
  if (selected === name && JSON.stringify(procCache.get(name)) !== before) drawProcs(name);
}

/** Docker can take minutes: the row says what it is doing until it is done. */
async function containerRun(name, service, action) {
  const key = `${name}/${service}`;
  procBusy.set(key, { start: "starting…", stop: "stopping…", restart: "restarting…" }[action]);
  if (selected === name) drawProcs(name);
  try {
    procCache.set(name, await invoke("container_run", { site: name, service, action }));
  } catch (e) {
    showError(String(e));
  }
  procBusy.delete(key);
  if (selected === name) drawProcs(name);
}

async function toggleContainerLog(name, service) {
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

function procBtn(label, aria, onclick) {
  const b = el("button", null, label);
  b.setAttribute("aria-label", aria);
  b.onclick = (e) => {
    e.stopPropagation();
    onclick();
  };
  return b;
}

/** One row: dot, name, detail; Restart and ■ when running, ▶ when stopped, Restart when crashed. */
function procRow({ name, state, detail, error, title, busy, open, run }) {
  const row = el("div", `proc ${state}`);
  const dot = el("span", `dot ${state === "running" ? "on" : state === "crashed" ? "bad" : ""}`);
  dot.title = { running: "Running", crashed: "Crashed", stopped: "Stopped" }[state];
  const label = el("button", "proc-name", name);
  label.title = title;
  row.onclick = open;
  const last = el("span", error ? "proc-last error-text" : "proc-last", busy ?? error ?? detail);
  last.title = last.textContent;
  row.append(dot, label, last);
  const actions = el("span", "proc-actions");
  if (!busy) {
    if (state !== "stopped") actions.append(procBtn("Restart", `Restart ${name}`, () => run("restart")));
    if (state === "running") actions.append(procBtn("■", `Stop ${name}`, () => run("stop")));
    if (state === "stopped") actions.append(procBtn("▶", `Start ${name}`, () => run("start")));
  }
  row.append(actions);
  return row;
}

function drawProcs(name) {
  const box = $("#site .procs");
  if (!box) return;
  const o = procCache.get(name);
  const rows = o?.rows ?? [];
  const commands = o?.commands ?? [];
  const containers = o?.containers ?? [];
  const compose = !!(o?.containers || o?.docker_error);
  box.hidden = !(rows.length || commands.length || compose);
  box.replaceChildren();
  if (box.hidden) return;

  const s = snap?.sites.find((x) => x.name === name);
  const own = s && PROCESS_KINDS.has(s.kind);
  const up = (c) => c.state === "running";
  const anyUp = (own && !!snap.app_states[name]) || rows.some((r) => r.running) || containers.some(up);
  const anyDown = (own && !snap.app_states[name]) || rows.some((r) => !r.running) || containers.some((c) => !up(c));
  if (rows.length || containers.length) {
    const head = el("div", "procs-head");
    head.append(el("h3", "res-sub", "Processes"));
    const all = (action) => {
      loadProcs(name, "process_run", { name: null, action });
      for (const c of containers) if (up(c) !== (action === "start")) containerRun(name, c.service, action);
    };
    const buttons = el("span", "proc-actions");
    if (anyDown) buttons.append(procBtn("Start all", "Start the app and all its processes", () => all("start")));
    if (anyUp) buttons.append(procBtn("Stop all", "Stop the app and all its processes", () => all("stop")));
    head.append(buttons);
    box.append(head);
  }

  if (rows.length) {
    const list = el("div", "group procs-list");
    for (const r of rows) {
      const where = r.address ?? (r.port ? `:${r.port}` : null);
      const exit = r.crashed && !r.running ? `Exited${r.crashed.code == null ? "" : ` with code ${r.crashed.code}`}` : null;
      list.append(procRow({
        name: r.name,
        state: r.running ? "running" : r.crashed ? "crashed" : "stopped",
        detail: [where, exit, r.last || r.command].filter(Boolean).join(" · "),
        error: r.error,
        title: `${r.command}\nFrom ${r.source}. Click to show its log.`,
        open: () => window.showLog?.(name, `proc:${r.id}`),
        run: (action) => loadProcs(name, "process_run", { name: r.id, action }),
      }));
    }
    box.append(list);
  }

  if (compose) {
    box.append(el("h3", "res-sub", "Containers"));
    if (o.docker_error) {
      const note = el("div", "procs-containers");
      note.append(el("p", "procs-note", o.docker_error));
      box.append(note);
    } else {
      const list = el("div", "group procs-list procs-containers");
      for (const c of containers) {
        const key = `${name}/${c.service}`;
        list.append(procRow({
          name: c.service,
          state: up(c) ? "running" : "stopped",
          detail: [...c.ports.map((p) => `:${p}`), c.health ?? c.state].join(" · "),
          title: "Click to show its log",
          busy: procBusy.get(key),
          open: () => toggleContainerLog(name, c.service),
          run: (action) => containerRun(name, c.service, action),
        }));
        if (procLogs.has(key)) list.append(el("pre", "proc-log", procLogs.get(key)));
      }
      box.append(list);
    }
  }

  if (commands.length) {
    box.append(el("h3", "res-sub", "Commands"));
    const chips = el("div", "chips");
    for (const c of commands) {
      const chip = el("button", "chip", c);
      chip.title = `npm run ${c}`;
      chip.onclick = () => {
        loadProcs(name, "command_run", { script: c });
        toast(`Running ${c}`);
      };
      chips.append(chip);
    }
    box.append(chips);
  }
}

// While the Overview is shown, keep states and last lines fresh.
setInterval(() => {
  if (document.hidden || mode !== "apps" || !selected || tab !== "overview") return;
  const o = procCache.get(selected);
  if (procLoading.has(selected) || !(o?.rows.length || o?.containers || o?.docker_error)) return;
  loadProcs(selected);
}, 2000);

// The first state may arrive before this file loads.
if (snap && mode === "apps") renderSite();
