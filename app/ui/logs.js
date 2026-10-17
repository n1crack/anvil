/* Logs tab: an app's log files, newest first. app.js calls logsTab() from showTab. */

const LEVELS = [
  ["all", "All"],
  ["error", "Errors"],
  ["warning", "Warnings"],
  ["info", "Info"],
  ["debug", "Debug"],
];
const STEP = 500; // rows added to the page at a time
const levelGroup = (l) =>
  ["emergency", "alert", "critical", "error", "fatal"].includes(l) ? "error"
    : l === "warning" || l === "warn" ? "warning"
      : l === "debug" ? "debug" : "info";

const logSource = new Map(); // site name -> chosen source id
let lv = null; // the view: { site, node, ... }
let followTimer = null;

function logTime(t) {
  if (!t) return null;
  // PHP-FPM writes 17-Oct-2026 09:05:35; WebKit only parses ISO-like dates.
  const MONTHS = "JanFebMarAprMayJunJulAugSepOctNovDec";
  const fpm = t.match(/^(\d{2})-([A-Z][a-z]{2})-(\d{4}) (.*)$/);
  if (fpm) t = `${fpm[3]}-${String(MONTHS.indexOf(fpm[2]) / 3 + 1).padStart(2, "0")}-${fpm[1]} ${fpm[4]}`;
  const d = new Date(t.replace(" ", "T").replace(/(\.\d{3})\d+/, "$1"));
  return Number.isNaN(d.getTime()) ? null : d;
}

function ago(d) {
  const s = Math.round((Date.now() - d) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 7 * 86400) return `${Math.round(s / 86400)} d ago`;
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: d.getFullYear() === new Date().getFullYear() ? undefined : "numeric" });
}

window.logsTab = (root, visible) => {
  const panel = $('[data-panel="logs"]', root);
  if (!visible || !panel) return follow(false);
  const s = snap.sites.find((x) => x.name === selected);
  if (!s) return;
  if (lv?.site !== s.name) lv = buildLogs(s);
  panel.replaceChildren(lv.node);
  if (lv.follow) follow(true);
};

function buildLogs(s) {
  const node = el("div", "logs");
  node.innerHTML = `
    <div class="logs-bar">
      <select class="logs-source" aria-label="Log file"></select>
      <input class="logs-search" type="search" placeholder="Search this log" aria-label="Search this log" spellcheck="false">
      <label class="logs-follow" title="Show new entries as they are written"><input type="checkbox" role="switch">Follow</label>
    </div>
    <div class="logs-bar">
      <div class="logs-levels" role="radiogroup" aria-label="Level"></div>
      <span class="spacer"></span>
      <button class="logs-console">Open in Console</button>
      <button class="logs-clear">Clear…</button>
    </div>
    <p class="logs-status muted"></p>
    <ol class="logs-list"></ol>
    <button class="logs-more" hidden>Load older</button>`;
  const v = {
    site: s.name,
    path: s.path,
    node,
    sources: [],
    source: null,
    level: "all",
    query: "",
    page: null, // last page from the backend (offsets)
    entries: [],
    shown: 0,
    follow: localStorage.getItem("logsFollow") === "1",
    token: 0,
  };

  const levels = $(".logs-levels", node);
  for (const [id, label] of LEVELS) {
    const b = el("button", "chip-btn", label);
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", id === "all");
    b.onclick = () => {
      v.level = id;
      for (const x of levels.children) x.setAttribute("aria-checked", x === b);
      loadLogs(v);
    };
    levels.append(b);
  }

  let debounce;
  const search = $(".logs-search", node);
  search.oninput = () => {
    clearTimeout(debounce);
    debounce = setTimeout(() => {
      v.query = search.value.trim();
      loadLogs(v);
    }, 300);
  };

  const fol = $(".logs-follow input", node);
  fol.checked = v.follow;
  fol.onchange = () => {
    v.follow = fol.checked;
    localStorage.setItem("logsFollow", v.follow ? "1" : "0");
    follow(v.follow);
  };

  $(".logs-source", node).onchange = (e) => {
    v.source = v.sources.find((x) => x.id === e.target.value);
    logSource.set(v.site, v.source.id);
    loadLogs(v);
  };
  $(".logs-console", node).onclick = () =>
    invoke("log_open", { site: v.site, source: v.source.id }).catch((e) => showError(String(e)));
  $(".logs-clear", node).onclick = () => clearLog(v);
  $(".logs-more", node).onclick = () => more(v);
  $(".logs-list", node).onclick = (e) => {
    const head = e.target.closest(".log-head");
    if (head) toggleEntry(v, head.parentElement);
  };

  loadSources(v, s);
  return v;
}

async function loadSources(v, s) {
  try {
    v.sources = await invoke("log_sources", { site: v.site });
  } catch (e) {
    v.sources = [];
    showError(String(e));
  }
  const sel = $(".logs-source", v.node);
  sel.replaceChildren(...v.sources.map((x) => new Option(`${x.label} (${bytes(x.bytes)})`, x.id)));
  sel.hidden = v.sources.length < 2;
  v.source = v.sources.find((x) => x.id === logSource.get(v.site)) ?? v.sources[0] ?? null;
  for (const b of v.node.querySelectorAll(".logs-bar")) b.hidden = !v.source;
  if (!v.source) return showEmpty(v, noSourcesText(s));
  sel.value = v.source.id;
  loadLogs(v);
}

function noSourcesText(s) {
  if (s.kind === "laravel" || s.kind === "statamic")
    return "No log files in storage/logs yet. They appear here once the app logs something, for example an exception or Log::info().";
  if (s.kind === "symfony") return "No log files in var/log yet. They appear here once the app logs something.";
  if (PROCESS_KINDS.has(s.kind)) return "No output yet. Start the app and what it prints shows up here.";
  if (s.kind === "static") return "Static sites don’t write logs.";
  return "No logs yet. PHP errors show up here once a PHP version serves this app.";
}

function showEmpty(v, text) {
  $(".logs-list", v.node).replaceChildren(el("li", "empty", text));
  $(".logs-status", v.node).textContent = "";
  $(".logs-more", v.node).hidden = true;
}

const filtered = (v) => v.level !== "all" || v.query;

/** First page for the current source and filters. */
async function loadLogs(v, keepScroll = false) {
  const token = ++v.token;
  if (!keepScroll) $(".logs-status", v.node).textContent = filtered(v) ? "Searching…" : "Reading…";
  let page;
  try {
    page = await fetchPage(v, null);
  } catch (e) {
    if (token === v.token) showEmpty(v, String(e));
    return;
  }
  if (token !== v.token) return;
  v.page = page;
  v.tailFrom = page.end_offset;
  v.entries = page.entries;
  v.shown = 0;
  $(".logs-list", v.node).replaceChildren();
  showMore(v);
}

function fetchPage(v, before) {
  const base = { site: v.site, source: v.source.id, before };
  return filtered(v)
    ? invoke("log_search", { ...base, query: v.query, level: v.level === "all" ? null : v.level })
    : invoke("log_read", base);
}

/** Load older: first rows already fetched, then the next page. */
async function more(v) {
  if (v.shown >= v.entries.length && v.page.truncated) {
    const btn = $(".logs-more", v.node);
    btn.disabled = true;
    btn.textContent = "Loading…";
    const token = v.token;
    try {
      const page = await fetchPage(v, v.page.start_offset);
      if (token !== v.token) return;
      v.page = { ...page, end_offset: v.page.end_offset };
      v.entries.push(...page.entries);
    } catch (e) {
      showError(String(e));
    }
    btn.disabled = false;
    btn.textContent = "Load older";
  }
  showMore(v);
}

function showMore(v) {
  const list = $(".logs-list", v.node);
  const frag = document.createDocumentFragment();
  const next = v.entries.slice(v.shown, v.shown + STEP);
  for (const e of next) frag.append(entryRow(v, e));
  list.append(frag);
  v.shown += next.length;
  if (!v.entries.length) {
    showEmpty(v, filtered(v)
      ? `Nothing matches${v.page.truncated ? ` in the newest ${bytes(v.page.end_offset - v.page.start_offset)}` : ""}.`
      : "This log is empty.");
  } else {
    status(v);
  }
  $(".logs-more", v.node).hidden = v.shown >= v.entries.length && !v.page?.truncated;
}

function status(v) {
  const src = v.source;
  const n = v.shown;
  const text = filtered(v)
    ? `${n} ${n === 1 ? "match" : "matches"} in ${src.label}${v.page.truncated ? `, searched the newest ${bytes(v.page.end_offset - v.page.start_offset)}` : ""}`
    : `Newest ${n} ${n === 1 ? "entry" : "entries"} of ${src.label} (${bytes(src.bytes)})`;
  $(".logs-status", v.node).textContent = text;
  $(".logs-status", v.node).title = tilde(src.path);
}

function entryRow(v, e) {
  const li = el("li", `log lv-${levelGroup(e.level)}`);
  li.entry = e;
  const head = el("button", "log-head");
  head.setAttribute("aria-expanded", "false");
  const d = logTime(e.time);
  const time = el("span", "log-time", d ? ago(d) : e.time ?? "");
  if (d) time.title = d.toLocaleString();
  const badge = el("span", "log-level", e.level);
  const msg = el("span", "log-msg", e.message.split("\n", 1)[0] || "(empty)");
  head.append(time, badge, msg);
  if (e.trace.length) head.append(el("span", "log-frames", `${e.trace.length} frames`));
  li.append(head);
  return li;
}

function toggleEntry(v, li) {
  const head = $(".log-head", li);
  const open = head.getAttribute("aria-expanded") !== "true";
  head.setAttribute("aria-expanded", open);
  if (!open) return $(".log-detail", li)?.remove();
  const e = li.entry;
  const box = el("div", "log-detail");
  const meta = [e.time, e.env].filter(Boolean).join(", ");
  if (meta) box.append(el("div", "log-meta", meta));
  box.append(el("pre", "log-text", e.message));
  if (e.context) {
    // One line per key; strings as written, without JSON escaping.
    let text = e.context;
    try {
      text = Object.entries(JSON.parse(text))
        .map(([k, val]) => `${k}: ${typeof val === "string" ? val : JSON.stringify(val, null, 2)}`)
        .join("\n");
    } catch {}
    if (text) {
      box.append(el("h4", null, "Context"), el("pre", "log-text log-json", text));
    }
  }
  if (e.trace.length) {
    box.append(el("h4", null, "Stack trace"));
    const ol = el("ol", "log-trace");
    const project = `${v.path}/`;
    for (const line of e.trace) {
      const own = line.includes(project) && !line.includes("/vendor/");
      ol.append(el("li", own ? "own" : "vendor", line.replaceAll(project, "")));
    }
    box.append(ol);
  }
  li.append(box);
}

/** Follow: check for new entries every 2 s while the tab is shown and the window focused. */
function follow(on) {
  clearInterval(followTimer);
  followTimer = on ? setInterval(followTick, 2000) : null;
}

async function followTick() {
  const v = lv;
  if (!v?.source || !v.page || !v.node.isConnected || v.node.closest("[hidden]")) return follow(false);
  if (document.hidden || !document.hasFocus()) return;
  const token = v.token;
  let page;
  try {
    page = await invoke("log_tail", { site: v.site, source: v.source.id, from: v.tailFrom });
  } catch {
    return;
  }
  if (token !== v.token) return;
  // The file shrank (cleared or rotated): start over.
  if (page.end_offset < v.tailFrom) return loadLogs(v, true);
  v.tailFrom = page.end_offset;
  v.source.bytes = page.end_offset;
  const q = v.query.toLowerCase();
  const fresh = page.entries.filter((e) =>
    (v.level === "all" || levelGroup(e.level) === v.level) &&
    (!q || [e.message, e.context ?? "", ...e.trace].join("\n").toLowerCase().includes(q)));
  if (!fresh.length) return;
  const list = $(".logs-list", v.node);
  if (!v.entries.length) list.replaceChildren();
  list.prepend(...fresh.map((e) => entryRow(v, e)));
  v.entries.unshift(...fresh);
  v.shown += fresh.length;
  status(v);
}

async function clearLog(v) {
  const src = v.source;
  const ok = await window.__TAURI__.dialog.ask(`Everything in ${tilde(src.path)} (${bytes(src.bytes)}) is deleted. This can’t be undone.`, {
    title: `Clear ${src.label}?`,
    kind: "warning",
    okLabel: "Clear",
    cancelLabel: "Cancel",
  });
  if (!ok) return;
  try {
    await invoke("log_clear", { site: v.site, source: src.id });
    src.bytes = 0;
    toast(`Cleared ${src.label}`);
  } catch (e) {
    showError(String(e));
  }
  loadLogs(v);
}

// app.js may have drawn the tab before this file loaded.
if (tab === "logs") window.logsTab($("#site"), true);
