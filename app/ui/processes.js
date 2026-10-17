/* Processes: an app's own server (or the site Anvil serves), queue worker, log tailer,
 * extra dev servers, one-off commands and Docker Compose containers, first in the
 * Overview. Each row starts and stops on its own; the header's Start/Stop run them all.
 * A row or command opens its output in a console below. Shares $, el, invoke, call,
 * snap, selected, tab, mode, M, render, toast and showError with app.js; clean and
 * ansi with term.js. */

const procCache = new Map(); // site name -> overview from the backend
const procLoading = new Set(); // site names with a request on its way
const procBusy = new Map(); // "site/service" -> "starting…" while Docker works
const procLogs = new Map(); // "site/service" -> container log text, while shown
let procEdit = null; // { site, id, value, error } while a row's command is edited

/** Called by renderSite: draw what is known, then ask again. */
window.renderProcesses = (s) => {
  drawProcs(s.name);
  loadProcs(s.name);
};

/** Ask the backend; false when it failed (the error is shown). */
async function loadProcs(name, cmd = "processes", args = {}) {
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
async function containerRun(name, services, action, service) {
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
    onclick(b);
  };
  return b;
}

/** Below a button, as the other menus. */
function popup(menu, btn) {
  const r = btn.getBoundingClientRect();
  const dpi = window.__TAURI__.dpi;
  menu.popup(dpi ? new dpi.LogicalPosition(r.left, r.bottom + 4) : undefined);
}

const setProc = (name, id, change) => loadProcs(name, "set_process", { id, ...change });

/** ⋯ on a row: edit its command, start it with Anvil, reset or remove it. */
async function procMenu(name, r, btn) {
  const items = [
    M.MenuItem.new({
      text: "Edit command…",
      action: () => {
        procEdit = { site: name, id: r.id, value: r.command, error: null };
        drawProcs(name);
      },
    }),
    M.CheckMenuItem.new({ text: "Start with Anvil", checked: r.autostart, action: () => setProc(name, r.id, { autostart: !r.autostart }) }),
  ];
  if (r.edited) items.push(M.MenuItem.new({ text: "Reset command", action: () => setProc(name, r.id, { reset: true }) }));
  if (!r.main) {
    items.push(
      M.PredefinedMenuItem.new({ item: "Separator" }),
      r.pinned
        ? M.MenuItem.new({ text: "Unpin", action: () => setProc(name, r.id, { pinned: false }) })
        : M.MenuItem.new({ text: "Remove from processes", action: () => setProc(name, r.id, { hidden: true }) }),
    );
  }
  popup(await M.Menu.new({ items: await Promise.all(items) }), btn);
}

/** Hidden rows, to bring back one at a time. */
async function hiddenMenu(name, hidden, btn) {
  const items = hidden.map((h) => M.MenuItem.new({ text: h.name, action: () => setProc(name, h.id, { hidden: false }) }));
  popup(await M.Menu.new({ items: await Promise.all(items) }), btn);
}

/** The row's command as a one-line field: Return saves, Esc cancels. */
function editField(name, r) {
  const form = el("form", "proc-edit");
  form.onclick = (e) => e.stopPropagation();
  const input = el("input");
  input.value = procEdit.value;
  input.spellcheck = false;
  input.setAttribute("aria-label", `Command for ${r.name}`);
  input.oninput = () => (procEdit.value = input.value);
  const cancel = () => {
    procEdit = null;
    drawProcs(name);
  };
  input.onkeydown = (e) => {
    if (e.key === "Escape") cancel(); // a handler returning false would swallow every other key
  };
  form.onsubmit = async (e) => {
    e.preventDefault();
    try {
      procCache.set(name, await invoke("set_process", { site: name, id: r.id, command: procEdit.value.trim() }));
      procEdit = null;
    } catch (err) {
      procEdit.error = String(err);
    }
    if (selected === name) drawProcs(name);
  };
  const save = el("button", null, "Save");
  save.type = "submit";
  const no = el("button", null, "Cancel");
  no.type = "button";
  no.onclick = cancel;
  form.append(input, save, no);
  if (procEdit.error) form.append(el("span", "proc-last error-text", procEdit.error));
  return form;
}

/** One line: dot, name, detail, then the auto tag, 🌐, Restart and ■ (or ▶) and ⋯. */
function procRow({ name, state, detail, error, title, busy, open, run, auto, web, menu, edit }) {
  const row = el("div", `proc ${state}`);
  const dot = el("span", `dot ${state === "running" ? "on" : state === "crashed" ? "bad" : ""}`);
  dot.title = { running: "Running", crashed: "Crashed", stopped: "Stopped" }[state];
  const label = el(open ? "button" : "span", "proc-name", name);
  label.title = title;
  if (open) row.onclick = open;
  row.append(dot, label);
  if (edit) row.append(edit);
  else {
    const last = el("span", error ? "proc-last error-text" : "proc-last", busy ?? error ?? detail);
    last.title = last.textContent;
    row.append(last);
  }
  const actions = el("span", "proc-actions");
  if (auto) actions.append(Object.assign(el("span", "proc-tag", "auto"), { title: "Starts with Anvil" }));
  if (web) {
    const b = procBtn("🌐", `Open ${name} in the browser`, () => invoke("open_url", { url: web }).catch((e) => showError(String(e))));
    b.title = state === "running" ? web : "Not running";
    b.disabled = state !== "running";
    actions.append(b);
  }
  if (run && !busy && !edit) {
    if (state !== "stopped") actions.append(procBtn("Restart", `Restart ${name}`, () => run("restart")));
    // On a crashed row ■ dismisses the crash.
    if (state !== "stopped") actions.append(procBtn("■", state === "crashed" ? `Dismiss ${name}` : `Stop ${name}`, () => run("stop")));
    if (state === "stopped") actions.append(procBtn("▶", `Start ${name}`, () => run("start")));
  }
  if (menu && !edit) actions.append(procBtn("⋯", `More for ${name}`, menu));
  row.append(actions);
  return row;
}

/** The header: how many run, and Start/Stop for all; a PHP site alone says whether it's served. */
function drawRunBox(name, rows, containers) {
  const box = $("#site .run-box");
  if (!box) return;
  const up = (c) => c.state === "running";
  const own = rows.filter((r) => r.kind !== "site");
  const total = own.length + containers.length;
  const running = own.filter((r) => r.running).length + containers.filter(up).length;
  const crashed = own.filter((r) => r.crashed && !r.running).length;
  box.hidden = !rows.length && !containers.length;
  const text = $(".state-text", box);
  const [start, stop] = [$('[data-all="start"]', box), $('[data-all="stop"]', box)];
  if (!total) {
    const serving = rows.some((r) => r.running);
    $(".dot", box).className = `dot ${serving ? "on" : ""}`;
    text.textContent = serving ? "Serving" : "Not serving";
    text.classList.remove("error-text");
    start.hidden = stop.hidden = true;
    return;
  }
  $(".dot", box).className = `dot ${crashed ? "bad" : running ? "on" : ""}`;
  text.textContent = [running && `${running} running`, crashed && `${crashed} crashed`].filter(Boolean).join(", ") || "Stopped";
  text.classList.toggle("error-text", !!crashed);
  start.hidden = running === total;
  stop.hidden = !running;
  const all = (action) => {
    loadProcs(name, "process_run", { name: null, action });
    const change = containers.filter((c) => up(c) !== (action === "start")).map((c) => c.service);
    if (change.length) containerRun(name, change, action);
  };
  start.onclick = () => all("start");
  stop.onclick = () => all("stop");
}

function drawProcs(name) {
  const box = $("#site .procs");
  if (!box) return;
  const o = procCache.get(name);
  const rows = o?.rows ?? [];
  const commands = o?.commands ?? [];
  const containers = o?.containers ?? [];
  const hidden = o?.hidden ?? [];
  const compose = !!(o?.containers || o?.docker_error);
  drawRunBox(name, rows, containers);
  box.hidden = !(rows.length || commands.length || compose || hidden.length);
  box.replaceChildren();
  drawConsole(name);
  if (box.hidden) return;
  let focus = null;

  if (rows.length || hidden.length) {
    const head = el("div", "procs-head");
    head.append(el("h3", "res-sub", "Processes"));
    if (hidden.length) {
      const more = el("button", "proc-link", `Show hidden (${hidden.length})`);
      more.onclick = () => hiddenMenu(name, hidden, more);
      head.append(more);
    }
    box.append(head);
  }

  if (rows.length) {
    const list = el("div", "group procs-list");
    for (const r of rows) {
      const site = r.kind === "site";
      const where = r.address ?? (r.port ? `:${r.port}` : null);
      const exit = r.crashed && !r.running ? `Exited${r.crashed.code == null ? "" : ` with code ${r.crashed.code}`}` : null;
      const editing = procEdit?.site === name && procEdit.id === r.id;
      const edit = editing ? editField(name, r) : null;
      if (edit) focus = $("input", edit);
      list.append(procRow({
        name: r.name,
        state: r.running ? "running" : r.crashed ? "crashed" : "stopped",
        detail: exit ?? where ?? r.command,
        error: r.error,
        title: site ? (r.running ? "Served by Anvil" : "Start Anvil to serve it") : `${r.command}\nFrom ${r.source}. Click to show its output.`,
        open: site ? null : () => openConsole(name, r.main ? "app" : `proc:${r.id}`, r.command || r.name),
        run: site ? null : r.main ? (action) => call("app_run", { site: name, action }) : (action) => loadProcs(name, "process_run", { name: r.id, action }),
        auto: r.autostart,
        web: r.web,
        menu: site ? null : (btn) => procMenu(name, r, btn),
        edit,
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
      const up = (c) => c.state === "running";
      for (const c of containers) {
        const key = `${name}/${c.service}`;
        list.append(procRow({
          name: c.service,
          state: up(c) ? "running" : "stopped",
          detail: [...c.ports.map((p) => `:${p}`), c.health ?? c.state].join(" · "),
          title: "Click to show its log",
          busy: procBusy.get(key),
          open: () => toggleContainerLog(name, c.service),
          run: (action) => containerRun(name, [c.service], action, c.service),
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
      // Running: a filling ring; then green or red for how the last run ended.
      const mark = c.running ? "warm" : c.exit ? (c.exit.code === 0 ? "on" : "bad") : null;
      const wrap = el("span", "chip-wrap");
      const chip = el("button", "chip");
      if (mark) chip.append(el("span", `dot ${mark}`));
      chip.append(c.name);
      chip.title = c.running
        ? `npm run ${c.name} is running. Click to show its output.`
        : `npm run ${c.name}${c.exit ? `, last run ${c.exit.code === 0 ? "succeeded" : "failed"}` : ""}. Click to run it.`;
      chip.onclick = async () => {
        if (!c.running && !(await loadProcs(name, "command_run", { script: c.name }))) return;
        openConsole(name, `once:${c.id}`, `npm run ${c.name}`);
      };
      const pin = procBtn("📌", `Add ${c.name} to processes`, () => setProc(name, c.id, { pinned: true }));
      pin.className = "chip-pin";
      pin.title = "Add to processes";
      wrap.append(chip, pin);
      chips.append(wrap);
    }
    box.append(chips);
  }
  focus?.focus();
}

/* ---------- Console: one row's or command's output, as a terminal shows it ---------- */

const conBox = el("section", "console");
conBox.setAttribute("aria-label", "Output");
conBox.innerHTML = `<header class="console-head"><span class="console-title"></span><span class="proc-actions"></span><button class="icon-btn console-close" aria-label="Close output">×</button></header><pre class="console-out"></pre>`;
const conOut = $(".console-out", conBox);
let con = null; // { site, source, label, raw, from, busy }
let conAtEnd = true; // follow new output only while the reader is at the bottom
let conTop = 0;
conOut.onscroll = () => {
  if (!conOut.clientHeight) return; // detached by a redraw, not scrolled
  conTop = conOut.scrollTop;
  conAtEnd = conTop + conOut.clientHeight >= conOut.scrollHeight - 4;
};
$(".console-close", conBox).onclick = () => {
  con = null;
  conBox.hidden = true;
};

function openConsole(site, source, label) {
  if (con?.site !== site || con.source !== source) {
    con = { site, source, label, raw: "", from: 0, busy: false };
    conOut.replaceChildren();
    conAtEnd = true;
  }
  drawConsole(site);
  conTail();
}

/** Under the Commands; what it shows, ■ while running, ↻ for a command that ended. */
function drawConsole(name) {
  const box = $("#site .procs");
  if (!box) return;
  if (box.nextElementSibling !== conBox) {
    box.after(conBox);
    conOut.scrollTop = conAtEnd ? conOut.scrollHeight : conTop;
  }
  conBox.hidden = con?.site !== name;
  if (conBox.hidden) return;
  const o = procCache.get(name);
  const [kind, id] = con.source === "app" ? ["app", ""] : con.source.split(":");
  const chip = kind === "once" ? o?.commands.find((c) => c.id === id) : null;
  const row = kind === "once" ? null : o?.rows.find((r) => (kind === "app" ? r.main : !r.main && r.id === id));
  $(".console-title", conBox).textContent = con.label;
  const actions = $(".proc-actions", conBox);
  actions.replaceChildren();
  if ((chip ?? row)?.running) {
    const stop = {
      app: () => call("app_run", { site: name, action: "stop" }),
      proc: () => loadProcs(name, "process_run", { name: id, action: "stop" }),
      once: () => invoke("once_stop", { site: name, id }).then(() => loadProcs(name), (e) => showError(String(e))),
    }[kind];
    actions.append(procBtn("■ Stop", `Stop ${con.label}`, stop));
  } else if (chip) {
    actions.append(procBtn("↻ Run again", `Run ${con.label} again`, () => loadProcs(name, "command_run", { script: chip.name })));
  }
}

/** New output since the last look; all of it is drawn again so a line redrawn
 * with \r, or a color begun earlier, comes out right. */
async function conTail() {
  const c = con;
  if (!c || c.busy) return;
  c.busy = true;
  let page;
  try {
    page = await invoke("log_text", { site: c.site, source: c.source, from: c.from });
  } catch {
    return; // no output yet
  } finally {
    c.busy = false;
  }
  if (c !== con) return;
  if (page.end_offset < c.from) c.raw = ""; // cleared: start over
  c.from = page.end_offset;
  if (!page.text) return;
  c.raw += page.text;
  // ponytail: keeps the newest ~512 KB and redraws all of it; a ring of lines if that gets slow.
  if (c.raw.length > 524288) c.raw = c.raw.slice(c.raw.indexOf("\n", c.raw.length - 262144) + 1);
  conOut.replaceChildren(ansi(clean(c.raw, true)));
  if (conAtEnd) conOut.scrollTop = conOut.scrollHeight;
}

// While the Overview is shown, keep states and the console fresh.
setInterval(() => {
  if (document.hidden || mode !== "apps" || !selected || tab !== "overview") return;
  if (con?.site === selected) conTail();
  const o = procCache.get(selected);
  if (procLoading.has(selected) || !(o?.rows.length || o?.commands.length || o?.containers || o?.docker_error || o?.hidden.length)) return;
  loadProcs(selected);
}, 2000);

// The first state may arrive before this file loads.
if (snap && mode === "apps") renderSite();
