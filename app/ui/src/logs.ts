import { $, M, PROCESS_KINDS, ago, bytes, el, invoke, open, select, selected, showError, showTab, snap, tab, tilde, toast } from "./app";

/* Logs tab: an app's log files, newest first. app.js calls logsTab() from showTab. */

export const LEVELS = [
  ["all", "All"],
  ["error", "Errors"],
  ["warning", "Warnings"],
  ["info", "Info"],
  ["debug", "Debug"],
];
export const STEP = 500; // rows added to the page at a time
export const levelGroup = (l) =>
  ["emergency", "alert", "critical", "error", "fatal"].includes(l) ? "error"
    : l === "warning" || l === "warn" ? "warning"
      : l === "debug" ? "debug" : "info";

export const logSource = new Map<string, string>(); // site name -> chosen source id
// An app's own output reads top to bottom like a terminal; log files show the newest first.
export const chrono = (v) => v.source?.id === "app";
export let lv: any = null; // the view: { site, node, ... }
export let followTimer: ReturnType<typeof setInterval> | null = null;

export function logTime(t) {
  if (!t) return null;
  // PHP-FPM writes 17-Oct-2026 09:05:35; WebKit only parses ISO-like dates.
  const MONTHS = "JanFebMarAprMayJunJulAugSepOctNovDec";
  const fpm = t.match(/^(\d{2})-([A-Z][a-z]{2})-(\d{4}) (.*)$/);
  if (fpm) t = `${fpm[3]}-${String(MONTHS.indexOf(fpm[2]) / 3 + 1).padStart(2, "0")}-${fpm[1]} ${fpm[4]}`;
  const d = new Date(t.replace(" ", "T").replace(/(\.\d{3})\d+/, "$1"));
  return Number.isNaN(d.getTime()) ? null : d;
}

window.logsTab = (root, visible) => {
  const panel = $('[data-panel="logs"]', root);
  if (!visible || !panel) return follow(false);
  const s = snap.sites.find((x) => x.name === selected);
  if (!s) return;
  if (lv?.site !== s.name) lv = buildLogs(s);
  panel.replaceChildren(lv.node);
  // Re-attaching resets the scroll box; put it back where the reader left it.
  const list = $(".logs-list", lv.node);
  list.scrollTop = lv.atEnd ? list.scrollHeight : lv.scrollTop;
  if (lv.follow) follow(true);
};

export function buildLogs(s) {
  const node = el("div", "logs");
  node.innerHTML = `
    <div class="logs-files" role="tablist" aria-label="Log files"></div>
    <div class="logs-bar">
      <input class="logs-search" type="search" placeholder="Search this log" aria-label="Search this log" spellcheck="false">
      <select class="logs-level" aria-label="Level"></select>
      <label class="logs-follow" title="Show new entries as they are written"><input type="checkbox" role="switch">Follow</label>
      <button class="logs-menu icon-btn" title="More" aria-label="More">⋯</button>
    </div>
    <p class="logs-status muted"></p>
    <ol class="logs-list"></ol>
    <button class="logs-more" hidden>Load older</button>`;
  const v: any = {
    site: s.name,
    path: s.path,
    node,
    sources: [],
    source: null,
    level: "all",
    query: "",
    page: null, // last page from the backend (offsets)
    scrollTop: 0,
    atEnd: true, // app output: keep showing the newest line
    entries: [],
    shown: 0,
    follow: localStorage.getItem("logsFollow") === "1",
    token: 0,
  };

  const levels = $(".logs-level", node);
  for (const [id, label] of LEVELS) levels.add(new Option(id === "all" ? "All levels" : label, id));
  levels.onchange = () => {
    v.level = levels.value;
    loadLogs(v);
  };

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

  // Less frequent actions wait behind ⋯.
  $(".logs-menu", node).onclick = async () => {
    const menu = await M.Menu.new({
      items: await Promise.all([
        M.MenuItem.new({
          text: "Open in Console",
          action: () => invoke("log_open", { site: v.site, source: v.source!.id }).catch((e) => showError(String(e))),
        }),
        M.PredefinedMenuItem.new({ item: "Separator" }),
        M.MenuItem.new({ text: `Clear ${v.source.label}…`, action: () => clearLog(v) }),
      ]),
    });
    menu.popup();
  };
  $(".logs-more", node).onclick = () => more(v);
  const list = $(".logs-list", node);
  list.onscroll = () => {
    v.scrollTop = list.scrollTop;
    v.atEnd = list.scrollHeight - list.scrollTop - list.clientHeight < 40;
  };
  $(".logs-list", node).onclick = (e) => {
    const head = (e.target as HTMLElement).closest(".log-head");
    if (head) toggleEntry(v, head.parentElement);
  };

  loadSources(v, s);
  return v;
}

export async function loadSources(v, s) {
  try {
    v.sources = await invoke("log_sources", { site: v.site });
  } catch (e) {
    v.sources = [];
    showError(String(e));
  }
  v.source = v.sources.find((x) => x.id === logSource.get(v.site)) ?? v.sources[0] ?? null;
  for (const b of v.node.querySelectorAll(".logs-bar")) b.hidden = !v.source;
  drawFiles(v);
  if (!v.source) return showEmpty(v, noSourcesText(s));
  loadLogs(v);
}

/** Every log file of the app side by side: name, size and when it last changed. */
export function drawFiles(v) {
  const strip = $(".logs-files", v.node);
  strip.replaceChildren();
  // Shown even for one file: it says which log this is.
  strip.hidden = !v.sources.length;
  for (const src of v.sources) {
    const b = el("button", src.bytes >= LARGE_LOG ? "logs-file large" : "logs-file");
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", String(src === v.source));
    b.title = src.path;
    const when = src.modified ? `, ${ago(new Date(src.modified * 1000))}` : "";
    b.append(el("strong", null, src.label), el("small", null, `${bytes(src.bytes)}${when}`));
    b.onclick = () => {
      v.source = src;
      logSource.set(v.site, src.id);
      drawFiles(v);
      loadLogs(v);
    };
    strip.append(b);
  }
}

// Above this a log isn't opened until asked: it's usually a forgotten single-file log.
export const LARGE_LOG = 100 * 1024 * 1024;
export const largeOk = new Set<string>(); // "site/source" opened anyway this session

export function showLarge(v) {
  const box = el("li", "logs-large");
  box.append(
    el("strong", null, `${v.source.label} is ${bytes(v.source.bytes)}`),
    el("p", "muted", v.site && v.source.label.endsWith(".log")
      ? "A log this big slows searches and fills the disk. Laravel keeps adding to a single file unless LOG_STACK=daily (or LOG_CHANNEL=daily) is set, which keeps one file per day."
      : "A log this big slows searches and fills the disk."),
  );
  const actions = el("div", "logs-large-actions");
  const show = el("button", null, "Show Latest Entries");
  show.onclick = () => {
    largeOk.add(`${v.site}/${v.source.id}`);
    loadLogs(v);
  };
  const clear = el("button", null, "Clear…");
  clear.onclick = () => clearLog(v);
  actions.append(show, clear);
  box.append(actions);
  $(".logs-list", v.node).replaceChildren(box);
  $(".logs-status", v.node).textContent = "";
  $(".logs-more", v.node).hidden = true;
  // The card has its own actions; the toolbar would only repeat them.
  $(".logs-bar", v.node).hidden = true;
}

export function noSourcesText(s) {
  if (s.kind === "laravel" || s.kind === "statamic")
    return "No log files in storage/logs yet. They appear here once the app logs something, for example an exception or Log::info().";
  if (s.kind === "symfony") return "No log files in var/log yet. They appear here once the app logs something.";
  if (PROCESS_KINDS.has(s.kind)) return "No output yet. Start the app and what it prints shows up here.";
  if (s.kind === "static") return "Static sites don’t write logs.";
  return "No logs yet. PHP errors show up here once a PHP version serves this app.";
}

export function showEmpty(v, text) {
  $(".logs-list", v.node).replaceChildren(el("li", "empty", text));
  $(".logs-status", v.node).textContent = "";
  $(".logs-more", v.node).hidden = true;
}

export const filtered = (v) => v.level !== "all" || v.query;

/** First page for the current source and filters. */
export async function loadLogs(v, keepScroll = false) {
  const token = ++v.token;
  $(".logs-bar", v.node).hidden = !v.source;
  if (v.source.bytes >= LARGE_LOG && !largeOk.has(`${v.site}/${v.source.id}`)) return showLarge(v);
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
  const list = $(".logs-list", v.node);
  list.replaceChildren();
  // A terminal's older lines are above it, so is the button that loads them.
  list.classList.toggle("chrono", chrono(v));
  list.classList.toggle("untimed", !v.entries.some((e) => e.time));
  if (chrono(v)) list.before($(".logs-more", v.node));
  else list.after($(".logs-more", v.node));
  showMore(v);
  v.atEnd = true;
  if (chrono(v)) list.scrollTop = list.scrollHeight;
}

export function fetchPage(v, before) {
  const base = { site: v.site, source: v.source.id, before };
  return filtered(v)
    ? invoke("log_search", { ...base, query: v.query, level: v.level === "all" ? null : v.level })
    : invoke("log_read", base);
}

/** Load older: first rows already fetched, then the next page. */
export async function more(v) {
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

export function showMore(v) {
  const list = $(".logs-list", v.node);
  const frag = document.createDocumentFragment();
  const next = v.entries.slice(v.shown, v.shown + STEP);
  if (chrono(v)) {
    // Older lines go on top without moving what is on screen.
    for (const e of next.toReversed()) frag.append(entryRow(v, e));
    const below = list.scrollHeight - list.scrollTop;
    list.prepend(frag);
    list.scrollTop = list.scrollHeight - below;
  } else {
    for (const e of next) frag.append(entryRow(v, e));
    list.append(frag);
  }
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

export function status(v) {
  const src = v.source;
  const n = v.shown;
  const text = filtered(v)
    ? `${n} ${n === 1 ? "match" : "matches"} in ${src.label}${v.page.truncated ? `, searched the newest ${bytes(v.page.end_offset - v.page.start_offset)}` : ""}`
    : "";
  $(".logs-status", v.node).textContent = text;
  $(".logs-status", v.node).title = tilde(src.path);
}

export function entryRow(v, e) {
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

export function toggleEntry(v, li) {
  const head = $(".log-head", li);
  const open = head.getAttribute("aria-expanded") !== "true";
  head.setAttribute("aria-expanded", String(open));
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
export function follow(on) {
  clearInterval(followTimer ?? undefined);
  followTimer = on ? setInterval(followTick, 2000) : null;
}

export async function followTick() {
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
  if (chrono(v)) {
    // Stay at the bottom only if that is where the reader was.
    list.append(...fresh.toReversed().map((e) => entryRow(v, e)));
    if (v.atEnd) list.scrollTop = list.scrollHeight;
  } else {
    list.prepend(...fresh.map((e) => entryRow(v, e)));
  }
  v.entries.unshift(...fresh);
  v.shown += fresh.length;
  status(v);
}

export async function clearLog(v) {
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
    drawFiles(v);
    // Show the file as it is now: empty, not the entries read before.
    await loadLogs(v);
    toast(`Cleared ${src.label}`);
  } catch (e) {
    showError(String(e));
  }
  loadLogs(v);
}

/** Open the Logs tab on one of the app's sources (e.g. "proc:queue"). */
window.showLog = (site, source) => {
  logSource.set(site, source);
  if (lv?.site === site) lv = null;
  showTab($("#site"), "logs");
};

// app.js may have drawn the tab before this file loaded.
if (tab === "logs") window.logsTab($("#site"), true);
