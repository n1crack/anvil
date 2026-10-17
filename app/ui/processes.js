/* Processes: an app's queue worker, log tailer, Vite… in the Overview, read from
 * .anvil.toml or the project's own dev script. Shares $, el, invoke, selected,
 * tab, mode and showError with app.js. */

const procCache = new Map(); // site name -> rows from the backend

/** Called by renderSite: draw what is known, then ask again. */
window.renderProcesses = (s) => {
  drawProcs(s.name);
  loadProcs(s.name);
};

async function loadProcs(name, cmd = "processes", args = {}) {
  const before = JSON.stringify(procCache.get(name));
  try {
    procCache.set(name, await invoke(cmd, { site: name, ...args }));
  } catch (e) {
    showError(String(e));
  }
  // Redraw only on a change, so focus and tooltips survive the polling.
  if (selected === name && JSON.stringify(procCache.get(name)) !== before) drawProcs(name);
}

function drawProcs(name) {
  const box = $("#site .procs");
  if (!box) return;
  const rows = procCache.get(name) ?? [];
  box.hidden = !rows.length;
  box.replaceChildren();
  if (!rows.length) return;

  const head = el("div", "procs-head");
  head.append(el("h3", "res-sub", "Processes"));
  const any = rows.some((r) => r.running);
  const all = el("button", null, any ? "Stop All" : "Start All");
  all.setAttribute("aria-label", `${any ? "Stop" : "Start"} all processes`);
  all.onclick = () => loadProcs(name, "process_run", { name: null, action: any ? "stop" : "start" });
  head.append(all);

  const list = el("div", "group procs-list");
  for (const r of rows) {
    const row = el("div", r.running ? "proc running" : "proc");
    const dot = el("span", `dot ${r.running ? "on" : ""}`);
    dot.title = r.running ? "Running" : "Stopped";
    const label = el("button", "proc-name", r.name);
    label.title = `${r.command}\nFrom ${r.source}. Click to show its log.`;
    label.onclick = () => window.showLog?.(name, `proc:${r.name}`);
    const last = el("span", r.error ? "proc-last error-text" : "proc-last", r.error ?? (r.last || r.command));
    last.title = last.textContent;
    const btn = el("button", null, r.running ? "Stop" : "Start");
    btn.setAttribute("aria-label", `${r.running ? "Stop" : "Start"} ${r.name}`);
    btn.onclick = () => loadProcs(name, "process_run", { name: r.id, action: r.running ? "stop" : "start" });
    row.append(dot, label, last, btn);
    list.append(row);
  }
  box.append(head, list);
}

// While the Overview shows processes, keep their state and last line fresh.
setInterval(() => {
  if (document.hidden || mode !== "apps" || !selected || tab !== "overview") return;
  if (procCache.get(selected)?.length) loadProcs(selected);
}, 2000);

// The first state may arrive before this file loads.
if (snap && mode === "apps") renderSite();
