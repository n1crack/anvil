import { $, M, PROCESS_KINDS, ago, bytes, el, invoke, seg, selected, showError, showTab, snap, tab, tilde } from "./app";
import { copy } from "./services";

/* Logs tab: an app's log files, newest first. app.js calls logsTab() from showTab. */

export const LEVELS: [string, string][] = [
  ["all", "All"],
  ["error", "Errors"],
  ["warning", "Warnings"],
];
// More sources than this become a menu instead of a row of segments.
export const SOURCES_INLINE = 5;
export const FOLLOW_ICON = `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 2.5v8M4.75 7.25 8 10.5l3.25-3.25M3.5 13.5h9" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
export const STEP = 500; // rows added to the page at a time
export const levelGroup = (l) =>
  ["emergency", "alert", "critical", "error", "fatal"].includes(l)
    ? "error"
    : l === "warning" || l === "warn"
      ? "warning"
      : l === "debug"
        ? "debug"
        : "info";

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
    <div class="logs-files"></div>
    <div class="logs-bar">
      <input class="logs-search" type="search" placeholder="Search this log" aria-label="Search this log" spellcheck="false">
      <button class="logs-follow icon-btn" aria-pressed="false" title="Follow: show new entries as they are written" aria-label="Follow">${FOLLOW_ICON}</button>
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

  $(".logs-search", node).after(
    seg("Level", LEVELS, v.level, (id) => {
      v.level = id;
      loadLogs(v);
    }),
  );

  let debounce;
  const search = $(".logs-search", node);
  search.oninput = () => {
    clearTimeout(debounce);
    debounce = setTimeout(() => {
      v.query = search.value.trim();
      loadLogs(v);
    }, 300);
  };

  const fol = $(".logs-follow", node);
  fol.setAttribute("aria-pressed", String(v.follow));
  fol.onclick = () => {
    v.follow = !v.follow;
    fol.setAttribute("aria-pressed", String(v.follow));
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

/** Which log this is, on one line: a segment per file, or a menu when there are many.
 * Size and last change are in the tooltip. */
export function drawFiles(v) {
  const strip = $(".logs-files", v.node);
  strip.hidden = !v.sources.length;
  const pick = (id) => {
    v.source = v.sources.find((x) => x.id === id);
    logSource.set(v.site, id);
    drawFiles(v);
    loadLogs(v);
  };
  const tip = (src) => `${tilde(src.path)}\n${bytes(src.bytes)}${src.modified ? `, changed ${ago(new Date(src.modified * 1000))}` : ""}`;
  if (v.sources.length > SOURCES_INLINE) {
    const menu = el("select", "logs-source");
    menu.setAttribute("aria-label", "Log file");
    for (const src of v.sources) menu.add(new Option(src.label, src.id));
    menu.value = v.source?.id;
    if (v.source) menu.title = tip(v.source);
    menu.onchange = () => pick(menu.value);
    return strip.replaceChildren(menu);
  }
  const box = seg(
    "Log file",
    v.sources.map((x) => [x.id, x.label]),
    v.source?.id,
    pick,
  );
  for (const b of box.children) {
    const src = v.sources.find((x) => x.id === b.dataset.id);
    b.title = tip(src);
    // A very large file is worth a glance before it's opened.
    if (src.bytes >= LARGE_LOG) b.classList.add("large");
  }
  strip.replaceChildren(box);
}

// Above this a log isn't opened until asked: it's usually a forgotten single-file log.
export const LARGE_LOG = 100 * 1024 * 1024;
export const largeOk = new Set<string>(); // "site/source" opened anyway this session

export function showLarge(v) {
  const box = el("li", "logs-large");
  box.append(
    el("strong", null, `${v.source.label} is ${bytes(v.source.bytes)}`),
    el(
      "p",
      "muted",
      v.site && v.source.label.endsWith(".log")
        ? "A log this big slows searches and fills the disk. Laravel keeps adding to a single file unless LOG_STACK=daily (or LOG_CHANNEL=daily) is set, which keeps one file per day."
        : "A log this big slows searches and fills the disk.",
    ),
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

/** Adjacent entries with the same level and message, as one row each: [[newest, …], …]. */
export const sameEntry = (a, b) => a.level === b.level && a.message === b.message;
export function groupEntries(entries) {
  const out: any[][] = [];
  for (const e of entries) {
    const last = out.at(-1);
    if (last && sameEntry(last[0], e)) last.push(e);
    else out.push([e]);
  }
  return out;
}

/** The rendered row at the newest or oldest end of the list. */
const edgeRow = (v, newest) => {
  const list = $(".logs-list", v.node);
  const r = newest === chrono(v) ? list.lastElementChild : list.firstElementChild;
  return r?.entries ? r : null;
};

/** Merge into an existing row: it is drawn again, still open if it was. */
function grow(v, li, entries, older) {
  const group = older ? [...li.entries, ...entries] : [...entries, ...li.entries];
  const row = entryRow(v, group);
  li.replaceWith(row);
  if ($(".log-head", li).getAttribute("aria-expanded") === "true") toggleEntry(v, row);
}

export function showMore(v) {
  const list = $(".logs-list", v.node);
  const next = v.entries.slice(v.shown, v.shown + STEP);
  const groups = groupEntries(next);
  // The page before ended in the middle of a run of repeats.
  const oldest = edgeRow(v, false);
  if (oldest && groups.length && sameEntry(oldest.entries[0], groups[0][0])) grow(v, oldest, groups.shift(), true);
  const frag = document.createDocumentFragment();
  if (chrono(v)) {
    // Older lines go on top without moving what is on screen.
    for (const g of groups.toReversed()) frag.append(entryRow(v, g));
    const below = list.scrollHeight - list.scrollTop;
    list.prepend(frag);
    list.scrollTop = list.scrollHeight - below;
  } else {
    for (const g of groups) frag.append(entryRow(v, g));
    list.append(frag);
  }
  v.shown += next.length;
  if (!v.entries.length) {
    showEmpty(
      v,
      filtered(v)
        ? `Nothing matches${v.page.truncated ? ` in the newest ${bytes(v.page.end_offset - v.page.start_offset)}` : ""}.`
        : "This log is empty.",
    );
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

/** One row: time of the newest, level as a dot and a word, the first line, ×N for repeats. */
export function entryRow(v, group) {
  const e = group[0];
  const li = el("li", `log lv-${levelGroup(e.level)}`);
  li.entries = group;
  const head = el("button", "log-head");
  head.setAttribute("aria-expanded", "false");
  const d = logTime(e.time);
  const time = el("span", "log-time", d ? ago(d) : (e.time ?? ""));
  if (d) time.title = d.toLocaleString();
  const level = el("span", "log-level");
  level.append(el("i", "log-dot"), e.level.toLowerCase());
  const msg = el("span", "log-msg", e.message.split("\n", 1)[0] || "(empty)");
  head.append(time, level, msg);
  if (group.length > 1) {
    const n = el("span", "log-count", `×${group.length}`);
    n.title = `${group.length} times in a row`;
    head.append(n);
  }
  li.append(head);
  return li;
}

// A frame's file and line: Laravel's "/x/Foo.php(42)", Node's "/x/foo.js:42:7".
export const FRAME = /(\/[^\s():'"]+\.[A-Za-z0-9]+)(?:\((\d+)\)|:(\d+))/;
export const VENDOR = /\/(vendor|node_modules)\//;

/** The frame's file relative to the app, or null when it isn't the app's. A path from a
 * container (/var/www/html/app/…) is matched by its first app folder.
 * ponytail: a custom container layout without those folders gets no editor link. */
export function appFile(root, file) {
  if (VENDOR.test(file)) return null;
  if (file.startsWith(`${root}/`)) return file.slice(root.length + 1);
  return file.match(/\/((?:app|src|routes|config|database|resources|tests|bootstrap|lib)\/.+)$/)?.[1] ?? null;
}

/** The stack trace: the app's own frames, the rest folded into "+N vendor frames". */
export function traceList(v, trace) {
  const ol = el("ol", "log-trace");
  let own = 0;
  for (const line of trace) {
    const m = line.match(FRAME);
    const rel = m ? appFile(v.path, m[1]) : null;
    const li = el("li", rel ? "own" : "vendor");
    if (!m) li.textContent = line;
    else {
      const lineNo = Number(m[2] ?? m[3]);
      const label = `${rel ?? m[1].replace(`${v.path}/`, "")}:${lineNo}`;
      let file;
      if (rel && snap.editor) {
        file = el("button", "log-file", label);
        file.title = `Open in ${snap.editor} at line ${lineNo}`;
        file.onclick = () => invoke("open_in_editor", { site: v.site, path: rel, line: lineNo }).catch((err) => showError(String(err)));
      } else file = el("span", "log-file", label);
      li.append(line.slice(0, m.index), file, line.slice(m.index! + m[0].length));
    }
    if (rel) own++;
    ol.append(li);
  }
  const vendor = trace.length - own;
  // Nothing of the app's own: the whole trace is what there is to read.
  if (!own || !vendor) return [ol];
  ol.classList.add("folded");
  const more = el("button", "log-vendor", `+${vendor} vendor ${vendor === 1 ? "frame" : "frames"}`);
  more.onclick = () => {
    ol.classList.remove("folded");
    more.remove();
  };
  return [ol, more];
}

/** Context JSON, indented, with quiet colors for keys, strings and literals. */
export function jsonView(text) {
  let src = text;
  try {
    const obj = JSON.parse(text);
    if (obj && typeof obj === "object" && !Object.keys(obj).length) return null;
    src = JSON.stringify(obj, null, 2);
  } catch {
    return el("pre", "log-text log-json", text);
  }
  const pre = el("pre", "log-text log-json");
  let at = 0;
  for (const m of src.matchAll(/("(?:\\.|[^"\\])*")(\s*:)?|\b(?:true|false|null)\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g)) {
    pre.append(src.slice(at, m.index));
    pre.append(el("span", m[1] ? (m[2] ? "j-key" : "j-str") : /\d/.test(m[0]) ? "j-num" : "j-lit", m[1] ?? m[0]));
    if (m[2]) pre.append(m[2]);
    at = m.index! + m[0].length;
  }
  pre.append(src.slice(at));
  return pre;
}

export const entryText = (e) => [`[${e.time ?? ""}] ${e.level}: ${e.message}`, e.context, ...e.trace].filter(Boolean).join("\n");

export function toggleEntry(v, li) {
  const head = $(".log-head", li);
  const open = head.getAttribute("aria-expanded") !== "true";
  head.setAttribute("aria-expanded", String(open));
  if (!open) return $(".log-detail", li)?.remove();
  const e = li.entries[0];
  const box = el("div", "log-detail");
  const top = el("div", "log-meta");
  // Today's times without the date: "4 times: 10:42:07 · 10:41:55 · …".
  const today = new Date().toDateString();
  const times = li.entries.map((x) => {
    const d = logTime(x.time);
    return !d ? (x.time ?? "") : d.toDateString() === today ? d.toLocaleTimeString() : d.toLocaleString();
  });
  const when = el("span", null, li.entries.length > 1 ? `${li.entries.length} times: ${times.join(" · ")}` : times[0]);
  if (e.env) when.append(el("span", "muted", ` · ${e.env}`));
  const cp = el("button", "log-copy", "Copy");
  cp.onclick = () => copy(entryText(e), cp);
  top.append(when, cp);
  box.append(top, el("pre", "log-text", e.message));
  const ctx = e.context && jsonView(e.context);
  if (ctx) box.append(el("h4", null, "Context"), ctx);
  if (e.trace.length)
    box.append(el("h4", null, li.entries.length > 1 ? "Stack trace of the latest" : "Stack trace"), ...traceList(v, e.trace));
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
  const fresh = page.entries.filter(
    (e) =>
      (v.level === "all" || levelGroup(e.level) === v.level) &&
      (!q || [e.message, e.context ?? "", ...e.trace].join("\n").toLowerCase().includes(q)),
  );
  if (!fresh.length) return;
  const list = $(".logs-list", v.node);
  if (!v.entries.length) list.replaceChildren();
  const groups = groupEntries(fresh);
  // The same entry again: the newest row counts up instead of a new row.
  const newest = edgeRow(v, true);
  if (newest && sameEntry(newest.entries[0], groups.at(-1)![0])) grow(v, newest, groups.pop(), false);
  if (chrono(v)) {
    // Stay at the bottom only if that is where the reader was.
    list.append(...groups.toReversed().map((g) => entryRow(v, g)));
    if (v.atEnd) list.scrollTop = list.scrollHeight;
  } else {
    list.prepend(...groups.map((g) => entryRow(v, g)));
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
