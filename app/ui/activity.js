/* The Processes window: everything Anvil runs, with usage, Restart/Stop and the
 * selected row's log. Keys come from `activity`; the log is asked for by key. */
const { invoke } = window.__TAURI__.core;

const $ = (sel, root = document) => root.querySelector(sel);
let data = null;
let drawn = ""; // what the table shows, to redraw only on a change
let selected = null; // row key
let logFrom = 0;
let logToken = 0;
let tailing = false;
const busy = new Set(); // keys with an action on its way

function showError(msg) {
  $("#error").textContent = msg ?? "";
  $("#error").hidden = !msg;
}

async function load() {
  try {
    data = await invoke("activity");
    showError(null);
  } catch (e) {
    return showError(String(e));
  }
  render();
}

window.refresh = load;

const mb = (b) => `${(b / 1048576).toFixed(1)} MB`;
const status = { running: "Running", starting: "Starting", crashed: "Crashed" };

function td(text, cls) {
  const c = document.createElement("td");
  if (cls) c.className = cls;
  c.textContent = text ?? "";
  return c;
}

function head(text, cls) {
  const tr = document.createElement("tr");
  tr.className = cls;
  const th = document.createElement("th");
  th.colSpan = 7;
  th.textContent = text;
  tr.append(th);
  return tr;
}

/** Which command a row's action goes to, by key form. */
function run(key, action) {
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

async function act(key, action) {
  busy.add(key);
  render();
  try {
    await run(key, action);
    showError(null);
  } catch (e) {
    showError(String(e));
  }
  busy.delete(key);
  await load();
}

function btn(label, r, action) {
  const b = document.createElement("button");
  b.textContent = label;
  b.setAttribute("aria-label", `${label} ${r.name}`);
  b.disabled = busy.has(r.key);
  b.onclick = (e) => {
    e.stopPropagation();
    act(r.key, action);
  };
  return b;
}

function row(r, inner) {
  const tr = document.createElement("tr");
  tr.className = `act-row ${r.state}${inner ? " inner" : ""}`;
  tr.setAttribute("aria-selected", r.key === selected);
  tr.dataset.key = r.key;
  tr.onclick = () => select(r.key);
  const name = td(r.name, "act-name");
  if (r.detail && r.state !== "crashed") {
    const d = document.createElement("span");
    d.className = "act-detail";
    d.textContent = r.detail;
    name.append(" ", d);
  }
  const crashed = r.state === "crashed";
  tr.append(
    name,
    td(r.pid),
    td(r.port),
    td(r.cpu == null ? "" : `${r.cpu.toFixed(1)}%`),
    td(r.memory == null ? "" : mb(r.memory)),
    td(crashed ? r.detail || status.crashed : status[r.state], "act-status"),
  );
  const actions = td(null, "act-actions");
  if (r.actions) {
    if (!r.key.includes("#")) actions.append(btn("Restart", r, "restart"));
    actions.append(btn("Stop", r, "stop"));
  }
  tr.append(actions);
  return tr;
}

function render() {
  if (!data) return;
  const keys = [...data.web, ...data.apps.flatMap((a) => a.rows), ...data.services].map((r) => r.key);
  if (selected && !keys.includes(selected)) select(null);
  // Redraw only on a change, so hover and focus survive the polling.
  const now = JSON.stringify([data, selected, [...busy]]);
  if (now === drawn) return;
  drawn = now;
  const body = $("#rows");
  const out = [];
  if (data.web.length) out.push(head("Web server", "act-group"), ...data.web.map((r) => row(r)));
  if (data.apps.length) {
    out.push(head("Applications", "act-group"));
    for (const a of data.apps) out.push(head(a.site, "act-app"), ...a.rows.map((r) => row(r, true)));
  }
  if (data.services.length) out.push(head("Services", "act-group"), ...data.services.map((r) => row(r)));
  body.replaceChildren(...out);
  $("#empty").hidden = keys.length > 0;
  $(".act-table table").hidden = !keys.length;
}

function select(key) {
  selected = key;
  logFrom = 0;
  logToken++;
  $("#log").textContent = "";
  $("#log").hidden = !key;
  render();
  if (key) tail();
}

const line = (e) => [e.time ? `${e.time} ${e.message}` : e.message, e.context, ...e.trace].filter(Boolean).join("\n");

async function tail() {
  const key = selected;
  const token = logToken;
  let page;
  tailing = true;
  try {
    page = await invoke("activity_log", { key, from: logFrom });
  } catch (e) {
    if (token === logToken) $("#log").textContent = String(e);
    return;
  } finally {
    tailing = false;
  }
  if (token !== logToken) return;
  const log = $("#log");
  const atEnd = log.scrollTop + log.clientHeight >= log.scrollHeight - 4;
  const text = page.entries.toReversed().map(line).join("\n");
  // Containers come back whole; a file that shrank starts over; otherwise only new lines.
  if (key.startsWith("container:") || page.end_offset < logFrom) {
    if (page.entries.length || page.end_offset < logFrom) log.textContent = text;
  } else if (text) {
    log.textContent += (log.textContent ? "\n" : "") + text;
  }
  logFrom = page.end_offset;
  if (atEnd) log.scrollTop = log.scrollHeight;
}

// Fresh while the window is shown and focused.
setInterval(() => {
  if (document.hidden || !document.hasFocus()) return;
  load();
  if (selected && !tailing) tail();
}, 2000);
window.addEventListener("focus", load);

load();
