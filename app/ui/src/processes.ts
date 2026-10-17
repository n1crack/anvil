import type { Overview } from "./types";
import { ansi, clean } from "./term";
import { $, M, call, el, invoke, mode, popAt, renderSite, selected, showError, snap, tab } from "./app";

/* Processes: an app's own server (or the site Anvil serves), queue worker, log tailer,
 * extra dev servers, one-off commands and Docker Compose containers, first in the
 * Overview. Each row starts and stops on its own; the header's Start/Stop run them all.
 * A row or command opens its output in a console below. Shares $, el, invoke, call,
 * snap, selected, tab, mode, M, render, toast and showError with app.js; clean and
 * ansi with term.js. */

export const procCache = new Map<string, Overview>(); // site name -> overview from the backend
export const procLoading = new Set<string>(); // site names with a request on its way
export const procBusy = new Map<string, string>(); // "site/service" -> "starting…" while Docker works
export const procLogs = new Map<string, string>(); // "site/service" -> container log text, while shown
export let procEdit: any = null; // { site, id, field, value, error } while a row's command or address is edited

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

/** Below a button, as the other menus. */
export function popup(menu, btn) {
  const r = btn.getBoundingClientRect();
  const dpi = (window.__TAURI__ as any).dpi;
  menu.popup(dpi ? new dpi.LogicalPosition(r.left, r.bottom + 4) : undefined);
}

/** A pin outline in currentColor, for "Add to processes" and rows kept on top. */
export const PIN = `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 2.5h4M6.75 2.5v4L4.5 9.25h7L9.25 6.5v-4M8 9.25v4.25" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

export const setProc = (name, id, change) => loadProcs(name, "set_process", { id, ...change });

/** An address's subdomain label and the fixed rest: ["docs", ".vuefinder.test"]. */
export function hostParts(address) {
  const host = new URL(address).hostname;
  const dot = host.indexOf(".");
  return [host.slice(0, dot), host.slice(dot)];
}

/** ⋯ on a row: edit its command or address, start it with Anvil, reset or remove it. */
export async function procMenu(name, r, btn) {
  const edit = (field, value) => () => {
    procEdit = { site: name, id: r.id, field, value, error: null };
    drawProcs(name);
  };
  const items = [
    M.MenuItem.new({ text: "Edit command…", action: edit("command", r.command) }),
    M.CheckMenuItem.new({ text: "Start with Anvil", checked: r.autostart, action: () => setProc(name, r.id, { autostart: !r.autostart }) }),
  ];
  if (r.edited) items.push(M.MenuItem.new({ text: "Reset command", action: () => setProc(name, r.id, { reset: true }) }));
  if (r.address) items.push(M.MenuItem.new({ text: "Edit address…", action: edit("host", hostParts(r.address)[0]) }));
  if (r.host_edited) items.push(M.MenuItem.new({ text: "Reset address", action: () => setProc(name, r.id, { resetHost: true }) }));
  if (!r.main) {
    items.push(
      // The app's own row is always first already.
      M.CheckMenuItem.new({ text: "Keep on Top", checked: !!r.top, action: () => setProc(name, r.id, { top: !r.top }) }),
      M.PredefinedMenuItem.new({ item: "Separator" }),
      r.pinned
        ? M.MenuItem.new({ text: "Unpin", action: () => setProc(name, r.id, { pinned: false }) })
        : M.MenuItem.new({ text: "Remove from processes", action: () => setProc(name, r.id, { hidden: true }) }),
    );
  }
  popup(await M.Menu.new({ items: await Promise.all(items) }), btn);
}

/** Hidden rows, to bring back one at a time. */
export async function hiddenMenu(name, hidden, btn) {
  const items = hidden.map((h) => M.MenuItem.new({ text: h.name, action: () => setProc(name, h.id, { hidden: false }) }));
  popup(await M.Menu.new({ items: await Promise.all(items) }), btn);
}

/** The row's command, or its address's label before the fixed rest, as a one-line
 * field: Return saves, Esc cancels. */
export function editField(name, r) {
  const host = procEdit.field === "host";
  const form = el("form", "proc-edit");
  form.onclick = (e) => e.stopPropagation();
  const input = el("input");
  input.value = procEdit.value;
  input.spellcheck = false;
  input.setAttribute("aria-label", String(host ? `Address of ${r.name}` : `Command for ${r.name}`));
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
      procCache.set(name, await invoke("set_process", { site: name, id: r.id, [procEdit.field]: procEdit.value.trim() }));
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
  form.append(input);
  if (host) form.append(el("span", "proc-suffix", hostParts(r.address)[1]));
  form.append(save, no);
  if (procEdit.error) form.append(el("span", "proc-last error-text", procEdit.error));
  return form;
}

/** One line: dot, name, detail, then the on-top pin, the auto tag, 🌐, Restart and ■ (or ▶) and ⋯. */
export function procRow({ name, state, detail, error, title, busy, open, run, top, auto, web, menu, edit }: Record<string, any>) {
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
  if (top) actions.append(Object.assign(el("span", "proc-top"), { title: "Kept on top", innerHTML: PIN }));
  if (auto) actions.append(Object.assign(el("span", "proc-tag", "auto"), { title: "Starts with Anvil" }));
  // Only something running has a page to open.
  if (web && state === "running") {
    const b = procBtn("🌐", `Open ${name} in the browser`, () => invoke("open_url", { url: web }).catch((e) => showError(String(e))));
    b.title = web;
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

export const CMD_MAX = 6; // commands listed before "Show all"

/** "now", "5m", "2h", "3d" since unix seconds `at`. */
export function agoShort(at) {
  const s = Date.now() / 1000 - at;
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
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

/** One package.json command: name, script, then the pin (on hover) and ▶, then how the
 * last run went, if it ran. A click shows its output once there is some. */
export function cmdRow(name, c) {
  const row = el("div", "proc cmd");
  const label = `npm run ${c.name}`;
  const show = () => openConsole(name, `once:${c.id}`, label);
  const run = async () => {
    if (await loadProcs(name, "command_run", { script: c.name })) show();
  };
  // A click only shows output that exists; running is ▶'s job, after a confirm.
  if (c.running || c.exit) row.onclick = show;
  row.title = c.running ? `${label} is running. Click to show its output.` : c.exit ? `Click to show the last output of ${label}.` : label;
  const script = el("span", "proc-last", c.command);
  row.append(el("span", "proc-name", c.name), script);
  const actions = el("span", "proc-actions");
  const pin = procBtn("", `Add ${c.name} to processes`, () => setProc(name, c.id, { pinned: true }));
  pin.innerHTML = PIN;
  pin.title = "Add to processes";
  pin.className = "cmd-hover cmd-pin";
  actions.append(pin);
  if (!c.running) {
    const play = procBtn("▶", `Run ${c.name}`, () => confirmPop(play, `Run ${label}?`, "Run", run));
    play.title = "Run";
    actions.append(play);
  }
  row.append(actions);
  // Only a command that ran gets a result column; otherwise ▶ sits flush right.
  if (!c.running && !c.exit) return row;
  const result = el("span", "cmd-result");
  if (c.running) {
    result.append(el("span", "dot warm"));
    result.title = "Running";
  } else if (c.exit) {
    const ok = c.exit.code === 0;
    result.append(el("span", ok ? "cmd-ok" : "cmd-bad", ok ? "✓" : "✗"), ` ${agoShort(c.exit.at)}`);
    result.title = `${ok ? "Succeeded" : "Failed"} ${new Date(c.exit.at * 1000).toLocaleString()}`;
  }
  row.append(result);
  return row;
}

/** The header: how many run, and Start/Stop for all; a PHP site alone says whether it's served. */
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
  const [start, stop] = [$('[data-all="start"]', box), $('[data-all="stop"]', box)];
  if (!total) {
    const serving = rows.some((r) => r.running);
    $(".dot", box).className = `dot ${serving ? "on" : ""}`;
    text.textContent = serving ? "Serving" : "Not serving";
    text.classList.remove("error-text");
    start.hidden = stop.hidden = true;
    $(".state", box).onmouseenter = null;
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

export function drawProcs(name) {
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
  let focus: HTMLElement | null = null;

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
      list.append(
        procRow({
          name: r.name,
          state: r.running ? "running" : r.crashed ? "crashed" : "stopped",
          detail: exit ?? where ?? r.command,
          error: r.error,
          title: site
            ? r.running
              ? "Served by Anvil"
              : "Start Anvil to serve it"
            : `${r.command}\nFrom ${r.source}. Click to show its output.`,
          open: site ? null : () => openConsole(name, r.main ? "app" : `proc:${r.id}`, r.command || r.name),
          run: site
            ? null
            : r.main
              ? (action) => call("app_run", { site: name, action })
              : (action) => loadProcs(name, "process_run", { name: r.id, action }),
          top: r.top && !r.main,
          auto: r.autostart,
          web: r.web,
          menu: site ? null : (btn) => procMenu(name, r, btn),
          edit,
        }),
      );
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
        list.append(
          procRow({
            name: c.service,
            state: up(c) ? "running" : "stopped",
            detail: [...c.ports.map((p) => `:${p}`), c.health ?? c.state].join(" · "),
            title: "Click to show its log",
            busy: procBusy.get(key),
            open: () => toggleContainerLog(name, c.service),
            run: (action) => containerRun(name, [c.service], action, c.service),
          }),
        );
        if (procLogs.has(key)) list.append(el("pre", "proc-log", procLogs.get(key)));
      }
      box.append(list);
    }
  }

  if (commands.length) {
    const head = el("div", "procs-head");
    head.append(el("h3", "res-sub", "Commands"));
    const key = `anvil.commands.all.${name}`;
    const all = localStorage.getItem(key) === "1";
    if (commands.length > CMD_MAX) {
      const more = el("button", "proc-link", all ? "Show fewer" : `Show all (${commands.length})`);
      more.onclick = () => {
        localStorage.setItem(key, all ? "0" : "1");
        drawProcs(name);
      };
      head.append(more);
    }
    box.append(head);
    // package.json's order, so rows don't move when one runs.
    const list = el("div", "group procs-list");
    for (const c of all ? commands : commands.slice(0, CMD_MAX)) list.append(cmdRow(name, c));
    box.append(list);
  }
  focus?.focus();
}

/* ---------- Console: one row's or command's output, as a terminal shows it ---------- */

export const conBox = el("section", "console");
conBox.setAttribute("aria-label", "Output");
conBox.innerHTML = `<header class="console-head"><span class="console-title"></span><span class="proc-actions"></span><button class="icon-btn console-close" aria-label="Close output">×</button></header><pre class="console-out"></pre>`;
export const conOut = $(".console-out", conBox);
export let con: any = null; // { site, source, label, raw, from, busy }
export let conAtEnd = true; // follow new output only while the reader is at the bottom
export let conTop = 0;
conOut.onscroll = () => {
  if (!conOut.clientHeight) return; // detached by a redraw, not scrolled
  conTop = conOut.scrollTop;
  conAtEnd = conTop + conOut.clientHeight >= conOut.scrollHeight - 4;
};
$(".console-close", conBox).onclick = () => {
  con = null;
  conBox.hidden = true;
};

export function openConsole(site, source, label) {
  if (con?.site !== site || con.source !== source) {
    con = { site, source, label, raw: "", from: 0, busy: false };
    conOut.replaceChildren();
    conAtEnd = true;
  }
  drawConsole(site);
  conTail();
}

/** Under the Commands; what it shows, ■ while running, ↻ for a command that ended. */
export function drawConsole(name) {
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
      once: () =>
        invoke("once_stop", { site: name, id }).then(
          () => loadProcs(name),
          (e) => showError(String(e)),
        ),
    }[kind];
    actions.append(procBtn("■ Stop", `Stop ${con.label}`, stop));
  } else if (chip) {
    actions.append(procBtn("↻ Run again", `Run ${con.label} again`, () => loadProcs(name, "command_run", { script: chip.name })));
  }
}

/** New output since the last look; all of it is drawn again so a line redrawn
 * with \r, or a color begun earlier, comes out right. */
export async function conTail() {
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
