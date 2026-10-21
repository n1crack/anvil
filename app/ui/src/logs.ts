import { $, M, PROCESS_KINDS, ago, bytes, el, inlineError, invoke, seg, selected, showError, showTab, snap, tab, tilde } from "./app";
import { follower, readTail, tail, termOut } from "./console";
import { copy } from "./services";
import { icon } from "./icons";
import { confirmPop } from "./processes";
import { shown } from "./live";
import { ansi, clean, linkify, sgr } from "./term";

/* Logs tab: an app's log files, newest first. app.js calls logsTab() from showTab. */

export const LEVELS: [string, string][] = [
  ["all", "All"],
  ["error", "Errors"],
  ["warning", "Warnings"],
];
// More sources than this become a menu instead of a row of segments.
const SOURCES_INLINE = 5;
const FOLLOW_ICON = `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 2.5v8M4.75 7.25 8 10.5l3.25-3.25M3.5 13.5h9" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
const STEP = 500; // rows added to the page at a time
const levelGroup = (l) =>
  ["emergency", "alert", "critical", "error", "fatal"].includes(l)
    ? "error"
    : l === "warning" || l === "warn"
      ? "warning"
      : l === "debug"
        ? "debug"
        : "info";

const logSource = new Map<string, string>(); // site name -> chosen source id
// A process's or a once-run command's output is a terminal's text, not log entries.
export const plain = (src) => src?.id === "app" || /^(proc|once):/.test(src?.id ?? "");
export let lv: any = null; // the view: { site, node, ... }
let followTimer: ReturnType<typeof setInterval> | null = null;

function logTime(t) {
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
  lv.list.restore();
  lv.term.restore();
  if (lv.follow) follow(true);
};

function buildLogs(s) {
  const node = el("div", "logs");
  node.innerHTML = `
    <div class="logs-files"></div>
    <div class="logs-bar">
      <input class="logs-search" type="search" placeholder="Search this log" aria-label="Search this log" spellcheck="false">
      <button class="logs-follow icon-btn" aria-pressed="false" title="Follow: show new entries as they are written" aria-label="Follow">${FOLLOW_ICON}</button>
      <button class="logs-menu icon-btn" title="More" aria-label="More">${icon("more").outerHTML}</button>
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
    term: termOut("logs-term"), // plain sources
    tail: null,
    entries: [],
    shown: 0,
    follow: localStorage.getItem("logsFollow") !== "0", // live unless turned off
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

  // Less frequent actions wait behind More.
  $(".logs-menu", node).onclick = async () => {
    const menu = await M.Menu.new({
      items: await Promise.all([
        M.MenuItem.new({
          text: "Open in Console",
          action: () => invoke("log_open", { site: v.site, source: v.source!.id }).catch((e) => showError(String(e))),
        }),
        M.PredefinedMenuItem.new({ item: "Separator" }),
        M.MenuItem.new({ text: `Clear ${v.source.label}…`, action: () => clearLog(v, $(".logs-menu", v.node)) }),
      ]),
    });
    menu.popup();
  };
  $(".logs-more", node).onclick = () => more(v);
  const list = $(".logs-list", node);
  v.list = follower(list, true); // newest first: it follows the top
  v.term.out.hidden = true;
  list.after(v.term.out);
  list.onclick = (e) => {
    const head = (e.target as HTMLElement).closest("button.log-head");
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
  v.source = v.sources.find((x) => x.id === logSource.get(v.site)) ?? v.sources[0] ?? null;
  for (const b of v.node.querySelectorAll(".logs-bar")) b.hidden = !v.source;
  drawFiles(v);
  if (!v.source) return showEmpty(v, noSourcesText(s));
  loadLogs(v);
}

/** Which log this is, on one line: a segment per file, or a menu when there are many.
 * Size and last change are in the tooltip. */
function drawFiles(v) {
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
const LARGE_LOG = 100 * 1024 * 1024;
const largeOk = new Set<string>(); // "site/source" opened anyway this session

function showLarge(v) {
  textMode(v, false);
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
  clear.onclick = () => clearLog(v, clear);
  actions.append(show, clear);
  box.append(actions);
  $(".logs-list", v.node).replaceChildren(box);
  $(".logs-status", v.node).textContent = "";
  $(".logs-more", v.node).hidden = true;
  // The card has its own actions; the toolbar would only repeat them.
  $(".logs-bar", v.node).hidden = true;
}

function noSourcesText(s) {
  if (s.kind === "laravel" || s.kind === "statamic")
    return "No log files in storage/logs yet. They appear here once the app logs something, for example an exception or Log::info().";
  if (s.kind === "symfony") return "No log files in var/log yet. They appear here once the app logs something.";
  if (PROCESS_KINDS.has(s.kind)) return "No output yet. Start the app and what it prints shows up here.";
  if (s.kind === "static") return "Static sites don’t write logs.";
  if (s.kind === "compose") return "Container logs are in the Containers tab.";
  return "No logs yet. PHP errors show up here once a PHP version serves this app.";
}

/** The list's one line: why it is empty, or (`error`) why it couldn't be read. */
function showEmpty(v, text, error = false) {
  textMode(v, false);
  $(".logs-list", v.node).replaceChildren(el("li", error ? "empty error-text" : "empty", text));
  $(".logs-status", v.node).textContent = "";
  $(".logs-more", v.node).hidden = true;
}

export const filtered = (v) => v.level !== "all" || v.query;

/** First page for the current source and filters. */
async function loadLogs(v, keepScroll = false) {
  const token = ++v.token;
  $(".logs-bar", v.node).hidden = !v.source;
  if (v.source.bytes >= LARGE_LOG && !largeOk.has(`${v.site}/${v.source.id}`)) return showLarge(v);
  if (plain(v.source)) return loadText(v, token);
  textMode(v, false);
  if (!keepScroll) $(".logs-status", v.node).textContent = filtered(v) ? "Searching…" : "Reading…";
  let page;
  try {
    page = await fetchPage(v, null);
  } catch (e) {
    if (token === v.token) showEmpty(v, String(e), true);
    return;
  }
  if (token !== v.token) return;
  v.page = page;
  v.trimmed = false;
  v.tailFrom = page.end_offset;
  v.entries = page.entries;
  v.shown = 0;
  const list = $(".logs-list", v.node);
  list.replaceChildren();
  if (!keepScroll) v.list.end(); // another file or filter: from the newest
  list.classList.toggle("untimed", !v.entries.some((e) => e.time));
  showMore(v);
}

/** Show the terminal view (plain sources) or the entry list. */
function textMode(v, on) {
  $(".logs-list", v.node).hidden = on;
  v.term.out.hidden = !on;
  if (on) $(".logs-more", v.node).hidden = true;
}

/** A plain source: its newest output, read as the console reads it (`log_text`). */
async function loadText(v, token) {
  const t = tail(v.site, v.source.id);
  await readTail(t);
  if (token !== v.token) return;
  // Another source starts at its end; the same one again (a filter) stays where it was.
  if (v.tail?.source !== t.source) v.term.end();
  v.tail = t;
  textMode(v, true);
  drawText(v);
}

// Errors and Warnings on plain text: lines with the word, as a terminal user would grep.
const KEYWORDS = {
  error: /\b(error|fatal|failed|exception)\b|\berr!|✗|✘/i,
  warning: /\bwarn(ing)?\b|⚠/i,
};

// The text drawn is kept to about what `readTail` keeps of the output.
const TEXT_MAX = 524288;

/** Lines as one block (display: block, so adding one doesn't lay out the others again):
 * colors carried in `v.style`, links found line by line. */
function textBlock(v, lines: string[]) {
  const b = el("span", "term-block");
  lines.forEach((l, i) => {
    if (i) b.append("\n");
    b.append(linkify(ansi(l, v.style)));
  });
  b.size = lines.reduce((n, l) => n + l.length + 1, 0);
  return b;
}

/** The output with ANSI colors, only the lines that match the filters, matches marked.
 * With `fresh` (what `readTail` just read) and no filter, only that is added. */
function drawText(v, fresh?: string) {
  const out = v.term.out;
  if (fresh != null && !filtered(v) && !v.tail.cleared && v.style) return appendText(v, fresh);
  v.tail.cleared = false;
  v.style = sgr();
  v.live = null;
  const q = v.query.toLowerCase();
  // Unfiltered, an unfinished last line is drawn apart, to be drawn again once it ends.
  const end = filtered(v) ? v.tail.raw.length : v.tail.raw.lastIndexOf("\n") + 1;
  v.pending = v.tail.raw.slice(end);
  let lines = clean(v.tail.raw.slice(0, end), true).replace(/\n$/, "").split("\n");
  if (!end) lines = [];
  if (filtered(v))
    lines = lines.filter((l) => {
      const t = clean(l);
      return (v.level === "all" || KEYWORDS[v.level].test(t)) && (!q || t.toLowerCase().includes(q));
    });
  out.dataset.empty = filtered(v) ? "Nothing matches." : "No output yet.";
  out.replaceChildren(); // empty, it says why (data-empty)
  if (lines.length) out.append(textBlock(v, lines));
  liveLine(v);
  if (q) mark(out, q);
  const n = lines.length;
  $(".logs-status", v.node).textContent = filtered(v) ? `${n} matching ${n === 1 ? "line" : "lines"} in ${v.source.label}` : "";
  $(".logs-status", v.node).title = tilde(v.source.path);
  v.term.follow();
}

/** The unfinished last line (a progress bar redrawn with \r), drawn after the rest. */
function liveLine(v) {
  v.live?.remove();
  v.live = null;
  if (!v.pending) return;
  v.live = el("span", "term-block");
  v.live.append(linkify(ansi(clean(v.pending, true), { ...v.style })));
  v.term.out.append(v.live);
}

/** Following without a filter: the new lines go after the others, the oldest blocks go
 * once there is more than TEXT_MAX. Nothing drawn before is drawn again. */
function appendText(v, fresh: string) {
  const out = v.term.out;
  const text = v.pending + fresh;
  const end = text.lastIndexOf("\n") + 1;
  v.pending = text.slice(end);
  v.live?.remove();
  if (end) out.append(textBlock(v, clean(text.slice(0, end - 1), true).split("\n")));
  liveLine(v);
  const blocks = [...out.children].filter((b: any) => b.size != null) as any[];
  let size = blocks.reduce((n, b) => n + b.size, 0);
  let k = 0;
  while (k < blocks.length - 1 && size > TEXT_MAX) size -= blocks[k++].size;
  if (k)
    v.term.keep(() => {
      const r = document.createRange();
      r.setStartBefore(blocks[0]);
      r.setEndBefore(blocks[k]);
      r.deleteContents();
    });
  v.term.follow();
}

/** Wrap each case-insensitive match of `q` in the text under `root` in <mark>. */
export function mark(root, q) {
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Node[] = [];
  while (walk.nextNode()) nodes.push(walk.currentNode);
  for (const n of nodes) {
    const t = n.textContent ?? "";
    const low = t.toLowerCase();
    let i = low.indexOf(q);
    if (i < 0) continue;
    const frag = document.createDocumentFragment();
    let at = 0;
    for (; i >= 0; i = low.indexOf(q, at)) {
      frag.append(t.slice(at, i), el("mark", null, t.slice(i, i + q.length)));
      at = i + q.length;
    }
    frag.append(t.slice(at));
    (n as ChildNode).replaceWith(frag);
  }
}

function fetchPage(v, before) {
  const base = { site: v.site, source: v.source.id, before };
  return filtered(v)
    ? invoke("log_search", { ...base, query: v.query, level: v.level === "all" ? null : v.level })
    : invoke("log_read", base);
}

/** Load older: first rows already fetched, then the next page. */
export async function more(v) {
  if (v.shown >= v.entries.length && v.trimmed) return loadLogs(v);
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
      inlineError(btn, null);
    } catch (e) {
      inlineError(btn, String(e));
    }
    btn.disabled = false;
    btn.textContent = "Load older";
  }
  showMore(v);
}

/** Adjacent entries with the same level and message, as one row each: [[newest, …], …]. */
const sameEntry = (a, b) => a.level === b.level && a.message === b.message;
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
  let r = newest ? list.firstElementChild : list.lastElementChild;
  if (r?.classList.contains("follow-pill")) r = r.nextElementSibling; // paused, it is on top
  return r?.entries ? r : null;
};

/** Merge into an existing row: it is drawn again, still open if it was. */
function grow(v, li, entries, older) {
  const group = older ? [...li.entries, ...entries] : [...entries, ...li.entries];
  const row = entryRow(v, group);
  li.replaceWith(row);
  if ($(".log-head", li).getAttribute("aria-expanded") === "true") toggleEntry(v, row);
}

function showMore(v) {
  const list = $(".logs-list", v.node);
  const next = v.entries.slice(v.shown, v.shown + STEP);
  const groups = groupEntries(next);
  // The page before ended in the middle of a run of repeats.
  const oldest = edgeRow(v, false);
  if (oldest && groups.length && sameEntry(oldest.entries[0], groups[0][0])) grow(v, oldest, groups.shift(), true);
  list.append(...groups.map((g) => entryRow(v, g)));
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
  $(".logs-more", v.node).hidden = v.shown >= v.entries.length && !v.page?.truncated && !v.trimmed;
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

/** Context worth showing: not missing, not an empty object. */
const hasContext = (e) => !!e.context && !/^\{\s*\}$/.test(e.context);
/** A row opens only when there is more than it shows: repeats' times, a trace, context or
 * more lines. A row without any shows its whole message, wrapped, and doesn't open. */
const hasDetail = (group) => group.length > 1 || group[0].trace.length > 0 || hasContext(group[0]) || group[0].message.includes("\n");

/** One row: time of the newest, level as a dot and a word, the first line, ×N for repeats. */
function entryRow(v, group) {
  const e = group[0];
  const opens = hasDetail(group);
  const li = el("li", `log lv-${levelGroup(e.level)}${opens ? "" : " flat"}`);
  li.entries = group;
  const head = el(opens ? "button" : "div", "log-head");
  if (opens) head.setAttribute("aria-expanded", "false");
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
const FRAME = /(\/[^\s():'"]+\.[A-Za-z0-9]+)(?:\((\d+)\)|:(\d+))/;
const VENDOR = /\/(vendor|node_modules)\//;

/** The frame's file relative to the app, or null when it isn't the app's. A path from a
 * container (/var/www/html/app/…) is matched by its first app folder.
 * ponytail: a custom container layout without those folders gets no editor link. */
function appFile(root, file) {
  if (VENDOR.test(file)) return null;
  if (file.startsWith(`${root}/`)) return file.slice(root.length + 1);
  return file.match(/\/((?:app|src|routes|config|database|resources|tests|bootstrap|lib)\/.+)$/)?.[1] ?? null;
}

/** A file as short as it reads: the app's relative path, a package's path from its name
 * (Laravel's from Illuminate/), else the path without the app's folder. */
const shortPath = (root, file) =>
  appFile(root, file) ??
  (VENDOR.test(file)
    ? file.replace(/^.*\/vendor\/laravel\/framework\/src\//, "").replace(/^.*\/(vendor|node_modules)\//, "")
    : file.replace(`${root}/`, ""));

/** file:line, short; an editor link when it is the app's, the full path in the tooltip. */
function fileLabel(v, file, lineNo) {
  const rel = appFile(v.path, file);
  const label = `${shortPath(v.path, file)}:${lineNo}`;
  if (!rel || !snap.editor) {
    const s = el("span", "log-file", label);
    s.title = file;
    return s;
  }
  const b = el("button", "log-file", label);
  b.title = `${file}\nOpen in ${snap.editor} at line ${lineNo}`;
  b.onclick = () => invoke("open_in_editor", { site: v.site, path: rel, line: lineNo }).catch((err) => showError(String(err)));
  return b;
}

/** The stack trace, a frame a line: number, file:line, the function muted. The app's own
 * frames show and the rest fold into "+N vendor frames"; with none of the app's, the
 * first three show and the rest fold into "+N more". */
function traceList(v, trace) {
  const ol = el("ol", "log-trace");
  let own = 0;
  for (const line of trace) {
    const m = line.match(FRAME);
    const rel = m ? appFile(v.path, m[1]) : null;
    const li = el("li", rel ? "own" : "vendor");
    const before = m ? line.slice(0, m.index).trim() : line.trim();
    const no = before.match(/^#\d+/)?.[0] ?? "";
    li.append(el("span", "log-no", no));
    if (m) li.append(fileLabel(v, m[1], Number(m[2] ?? m[3])));
    // Node writes "at fn (/x.js:1:2)": keep the function, drop the brackets.
    const fn = [
      before
        .slice(no.length)
        .replace(/^at\s+/, "")
        .replace(/\s*\($/, ""),
      m ? line.slice(m.index! + m[0].length).replace(/^:\s*|^\)$/g, "") : "",
    ]
      .map((x) => x.trim())
      .filter(Boolean)
      .join(" ");
    if (fn) li.append(el("span", "log-fn", fn));
    li.title = line;
    if (rel) own++;
    ol.append(li);
  }
  if (own === trace.length) return [ol];
  const fold = own ? [...ol.querySelectorAll(".vendor")] : [...ol.children].slice(3);
  if (!fold.length) return [ol];
  for (const li of fold) li.classList.add("fold");
  ol.classList.add("folded");
  const n = fold.length;
  const more = el("button", "log-vendor", own ? `+${n} vendor ${n === 1 ? "frame" : "frames"}` : `+${n} more`);
  more.onclick = () => {
    ol.classList.remove("folded");
    more.remove();
  };
  return [ol, more];
}

/** Context JSON, indented, with quiet colors for keys, strings and literals. */
function jsonView(text) {
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

// Message tokens: a tag's name, an attribute's name, a "string" (not 'quotes': prose has "don't"), {…} or ... (elided), %s.
const TOKENS =
  /(<\/?)([A-Za-z][\w.:-]*)|(?<=\s)([A-Za-z_:][\w:.-]*)(?==)|("(?:[^"\\\n]|\\.)*")|(\{\{[^{}\n]*\}\}|\{[^{}\n]*\}|\.\.\.|…)|(%[sdifoOc])/g;
// A diff line of React's hydration error: ">" marks the mismatch, "+" and "-" what differs.
// React indents after + and -, so a "- bullet" of the prose above the tree isn't one.
const MARK = /^(\s*)(>(?=\s|$)|[+-](?=\s\s|$))/;

/** A multi-line message, a JSX tree (React's hydration error) as quietly highlighted text:
 * tags, attributes, strings, elided {…}, %s placeholders and the >/+/- lines. Text is only
 * ever set as text, never as HTML. Plain messages get placeholders muted, nothing more. */
export function messageView(text: string) {
  const pre = el("pre", "log-text log-msgtext");
  const jsx = /<\/?[A-Za-z][\w.:-]*[\s>/]/.test(text);
  for (const l of text.split("\n")) {
    const mark = jsx ? l.match(MARK) : null;
    const row = el("span", `hl-line${mark ? ` hl-${{ ">": "here", "+": "add", "-": "del" }[mark[2]]}` : ""}`);
    let at = 0;
    if (mark) {
      row.append(mark[1], el("span", "hl-mark", mark[2]));
      at = mark[0].length;
    }
    const rest = l.slice(at);
    let i = 0;
    for (const m of rest.matchAll(TOKENS)) {
      if (!jsx && !m[6]) continue;
      row.append(rest.slice(i, m.index));
      if (m[2]) row.append(m[1], el("span", "hl-tag", m[2]));
      else if (m[4]) {
        // A placeholder stays muted inside a string too: title="%s".
        const str = el("span", "hl-str");
        m[4].split(/(%[sdifoOc])/).forEach((part, j) => str.append(j % 2 ? el("span", "hl-mute", part) : part));
        row.append(str);
      } else row.append(el("span", m[3] ? "hl-attr" : "hl-mute", m[0]));
      i = m.index! + m[0].length;
    }
    row.append(rest.slice(i));
    pre.append(row);
  }
  return pre;
}

/** Laravel's context.exception, "[object] (Class(code: 0): message at /file:line)", in parts. */
export function parseException(s) {
  const m = typeof s === "string" && s.trim().match(/^\[object\] \(([\w\\]+)\(code: [^)]*\): ([\s\S]*) at (\/.+?):(\d+)\)$/);
  return m ? { cls: m[1], message: m[2], file: m[3], line: Number(m[4]) } : null;
}

const DAY = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short" });
const HM = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit" });
const HMS = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
/** "5 Oct 13:23"; without the day, or with seconds. */
const stamp = (d, day = true, secs = false) => `${day ? `${DAY.format(d)} ` : ""}${(secs ? HMS : HM).format(d)}`;

/** When, for one entry or repeats (newest first): "24 times · first 5 Oct 13:23 · last 16:24". */
export function timesSummary(times) {
  const ds = times.map(logTime);
  if (times.length === 1) return ds[0] ? stamp(ds[0], true, true) : (times[0] ?? "");
  const [last, first] = [ds[0], ds.at(-1)];
  const out = [`${times.length} times`];
  if (first && last) out.push(`first ${stamp(first)}`, `last ${stamp(last, first.toDateString() !== last.toDateString())}`);
  return out.join(" · ");
}

const entryText = (e) => [`[${e.time ?? ""}] ${e.level}: ${e.message}`, e.context, ...e.trace].filter(Boolean).join("\n");

function toggleEntry(v, li) {
  const head = $(".log-head", li);
  const open = head.getAttribute("aria-expanded") !== "true";
  head.setAttribute("aria-expanded", String(open));
  if (!open) return $(".log-detail", li)?.remove();
  const e = li.entries[0];
  const box = el("div", "log-detail");
  const top = el("div", "log-meta");
  const times = li.entries.map((x) => x.time);
  const when = el("span", null, timesSummary(times));
  if (e.env) when.append(el("span", "muted", ` · ${e.env}`));
  if (times.length > 1) {
    const all = el("button", "log-link", "show all times");
    all.onclick = () => {
      const ol = el("ol", "log-times");
      for (const t of times) {
        const d = logTime(t);
        ol.append(el("li", null, d ? stamp(d, true, true) : (t ?? "")));
      }
      top.after(ol);
      all.remove();
    };
    when.append(" ", all);
  }
  const cp = el("button", "log-copy", "Copy");
  cp.onclick = () => copy(entryText(e), cp);
  top.append(when, cp);
  box.append(top);

  // Laravel's exception context only repeats the message: its class and where it was thrown.
  let ctx = hasContext(e) ? e.context : null;
  let obj;
  try {
    obj = JSON.parse(ctx);
  } catch {}
  const ex = parseException(obj?.exception);
  if (ex) {
    const line = el("div", "log-exc");
    const cls = el("span", "log-exc-cls", ex.cls.split("\\").pop());
    cls.title = ex.cls;
    line.append(cls, fileLabel(v, ex.file, ex.line));
    box.append(line);
    delete obj.exception;
    ctx = Object.keys(obj).length ? JSON.stringify(obj) : null;
  }
  // The row shows the first line, cut at the edge: the whole message only if it didn't fit.
  const msg = $(".log-msg", li);
  if (e.message.includes("\n") || msg.scrollWidth > msg.clientWidth) box.append(messageView(e.message));
  const json = ctx && jsonView(ctx);
  if (json) box.append(el("h4", null, "Context"), json);
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
  // A plain source has no page: its output is read as text (loadText).
  if (!v?.source || (!plain(v.source) && !v.page) || !v.node.isConnected || v.node.closest("[hidden]")) return follow(false);
  if (!shown() || !document.hasFocus()) return;
  const token = v.token;
  if (plain(v.source)) {
    const t = v.tail;
    const fresh = t && (await readTail(t));
    if (fresh && token === v.token && t === v.tail && lv === v) drawText(v, fresh);
    return;
  }
  // One at a time: a slow read must not be followed by a second one from the same offset.
  if (v.tailing) return;
  v.tailing = true;
  let page;
  try {
    page = await invoke("log_tail", { site: v.site, source: v.source.id, from: v.tailFrom });
  } catch {
    return;
  } finally {
    v.tailing = false;
  }
  // Another file, filter or app since: this answer is for a view no longer shown.
  if (token !== v.token || lv !== v) return;
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
  v.list.keep(() => {
    if (newest && sameEntry(newest.entries[0], groups.at(-1)![0])) grow(v, newest, groups.pop(), false);
    list.prepend(...groups.map((g) => entryRow(v, g)));
  });
  v.list.follow();
  v.entries.unshift(...fresh);
  v.shown += fresh.length;
  if (v.entries.length > MAX_ENTRIES) trimEntries(v);
  status(v);
}

export const MAX_ENTRIES = 2000; // entries kept while following

/** A long follow: the oldest rows and entries go. Load older then starts again from the
 * newest page, since what was dropped has no offset to read on from. */
function trimEntries(v) {
  let old;
  while (v.shown > MAX_ENTRIES && (old = edgeRow(v, false))) {
    v.shown -= old.entries.length;
    old.remove();
  }
  v.entries.length = MAX_ENTRIES;
  v.trimmed = true;
  $(".logs-more", v.node).hidden = false;
}

function clearLog(v, at: HTMLElement) {
  const src = v.source;
  confirmPop(
    at,
    `Clear ${src.label}? Everything in ${tilde(src.path)} (${bytes(src.bytes)}) is deleted. This can’t be undone.`,
    "Clear",
    () => clearNow(v, src),
  );
}

async function clearNow(v, src) {
  try {
    await invoke("log_clear", { site: v.site, source: src.id });
    src.bytes = 0;
    drawFiles(v);
    // Show the file as it is now: empty, not the entries read before.
    await loadLogs(v);
    inlineError($(".logs-menu", v.node), null);
  } catch (e) {
    inlineError($(".logs-menu", v.node), String(e));
  }
  loadLogs(v);
}

/** Open the Logs tab on one of the app's sources (e.g. "proc:queue"). */
export function showLog(site: string, source: string) {
  logSource.set(site, source);
  if (lv?.site === site) lv = null;
  showTab($("#site"), "logs");
}

// app.js may have drawn the tab before this file loaded.
if (tab === "logs") window.logsTab($("#site"), true);
