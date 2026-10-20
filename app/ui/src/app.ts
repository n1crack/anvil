import type { Snapshot } from "./types";
import { late } from "./late";
import { ownError, shown } from "./live";
import { FIELDS, control } from "./hooksform";

export const { invoke } = window.__TAURI__.core;

export const $ = (sel: string, root: ParentNode = document): any => root.querySelector(sel);
export let snap: Snapshot = null as unknown as Snapshot; // set by the first `state`
export let selected = localStorage.getItem("selected");
export let filter = "";
// The sidebar shows applications or services; services.ts draws the latter.
export let mode = localStorage.getItem("mode") === "services" ? "services" : "apps";
// List filters per mode: a kind (or null for all) and running only.
export const filters = JSON.parse(localStorage.getItem("filters") ?? "{}");
export const filterOf = (m) => (filters[m] ??= { kind: null, running: false });

export const tilde = (p) => p.replace(/^\/Users\/[^/]+/, "~");
// A standard port (443, or 80 without HTTPS) isn't written out.
// Sites whose HTTPS is being turned on or off (the web server reloads meanwhile).
export const securing = new Set<string>();

// Compose projects and plain folders get no address from Anvil.
export const serves = (s) => !["compose", "folder"].includes(s.kind);

export const url = (s) => {
  const [scheme, port, std] = s.secure ? ["https", snap.https_port, 443] : ["http", snap.http_port, 80];
  return `${scheme}://${s.host}${port === std ? "" : `:${port}`}`;
};

const BRANCH = `<svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="4.5" cy="3.5" r="1.5"/><circle cx="4.5" cy="12.5" r="1.5"/><circle cx="11.5" cy="5.5" r="1.5"/><path d="M4.5 5v6M11.5 7c0 2.5-3.5 2-7 4"/></svg>`;

const LOCK = `<svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><rect x="3" y="7" width="10" height="7" rx="1.5" fill="currentColor"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>`;

let snapAt = 0; // when the last snapshot arrived
let snapAsked = 0; // numbers each request for a snapshot…
let snapShown = 0; // …and this one's is in `snap`

/** A snapshot from `cmd`, unless one asked for later already came: true when it is newest. */
async function askSnap(cmd: string, args?: Record<string, unknown>) {
  const n = ++snapAsked;
  const next = await invoke(cmd, args);
  if (!next) return true;
  if (n < snapShown) return false;
  snapShown = n;
  snap = next;
  snapAt = Date.now();
  return true;
}

export async function call(cmd: string, args?: Record<string, unknown>) {
  try {
    if (!(await askSnap(cmd, args))) return;
    showError(null, cmd);
    render();
  } catch (e) {
    showError(String(e), cmd);
  }
}

const errors = new Map<string, string>(); // owner -> its error, see ownError
/** The error banner: `owner`'s error, or null to clear only that one. Without an owner it
 * stays until dismissed. */
export function showError(msg, owner = "") {
  const text = ownError(errors, msg, owner);
  $("#error-text").textContent = text;
  $("#error").hidden = !text;
}

let toastTimer;
/** A short confirmation at the bottom of the window. */
export function toast(msg: string, undo?: () => unknown) {
  const t = $("#toast");
  t.textContent = msg;
  if (undo) {
    const b = el("button", null, "Undo");
    b.onclick = () => {
      t.hidden = true;
      undo();
    };
    t.append(b);
  }
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), undo ? 6000 : 2400);
}

window.refresh = () => {
  // A command's own answer already carries the new state; the "changed" signal
  // that follows it doesn't need a second read.
  // ponytail: a Settings change landing within 0.5 s of a click here waits for the next refresh.
  if (Date.now() - snapAt > 500) call("state");
  window.loadServices?.();
  loadSetup();
};

/* ---------- Setup: what still stands between this Mac and working addresses ---------- */

// [step, field in setup_status, label, what it does]
// What setting up this Mac means, beyond PHP: local domains, ports 80/443, the HTTPS
// certificate and the terminal commands. One button does them all, one password.
const SETUP_FIELDS = ["resolver", "ports", "trusted", "shims"];

async function loadSetup(busy?: string) {
  let st;
  try {
    st = await invoke("setup_status");
  } catch {
    return;
  }
  const bar = $("#setup");
  const machine = SETUP_FIELDS.some((f) => !st[f]);
  // `git` is false only where Anvil can install it (Windows).
  bar.hidden = !machine && st.php && st.git !== false;
  bar.replaceChildren();
  if (machine) {
    const b = el("button", "setup-go", busy === "all" ? "Setting up…" : "Set up Anvil");
    if (busy === "all") b.prepend(el("span", "spin-dot"));
    b.title =
      "Local .test domains, ports 80 and 443, the HTTPS certificate and php, composer, laravel and node in your terminal. Asks for your password once.";
    b.disabled = !!busy;
    b.onclick = async () => {
      loadSetup("all");
      try {
        const after = await invoke("setup_fix", { step: "all" });
        // macOS may refuse to trust the certificate system-wide without asking;
        // the login keychain then takes it, with its own prompt.
        if (after && !after.trusted) await invoke("setup_fix", { step: "trust" });
      } catch (e) {
        showError(String(e));
      }
      loadSetup();
    };
    bar.append(b);
  }
  if (!st.php) {
    const b = el("button", null, busy === "php" ? "Installing PHP…" : "Install PHP 8.4");
    b.title = "PHP for PHP applications; no password needed";
    b.disabled = !!busy;
    b.onclick = async () => {
      loadSetup("php");
      try {
        await invoke("setup_fix", { step: "php" });
      } catch (e) {
        showError(String(e));
      }
      loadSetup();
    };
    bar.append(b);
  }
  if (st.git === false) {
    const b = el("button", null, busy === "git" ? "Installing Git…" : "Install Git");
    if (busy === "git") b.prepend(el("span", "spin-dot"));
    b.title = "Git for Windows, through winget: new apps get a repository, and worktrees work. Windows may ask once.";
    b.disabled = !!busy;
    b.onclick = async () => {
      loadSetup("git");
      try {
        await invoke("setup_fix", { step: "git" });
      } catch (e) {
        showError(String(e));
      }
      loadSetup();
    };
    bar.append(b);
  }
}

export function render() {
  showError(snap.error, "snapshot");

  const power = $("#power");
  power.dataset.on = snap.running;
  // Serving, it opens a menu; stopped, a click just starts.
  if (snap.running) power.setAttribute("aria-haspopup", "menu");
  else power.removeAttribute("aria-haspopup");
  $("#power-label").textContent = snap.running ? "Serving" : "Stopped";

  if (!snap.sites.some((s) => s.name === selected)) selected = snap.sites[0]?.name;
  renderMain();
}

export function renderMain() {
  renderFilter();
  if (mode === "services") {
    late.renderServiceList();
    late.renderService();
  } else {
    renderList();
    renderSite();
  }
}

export const icons = new Map<string, string | null>(); // site name -> data URL or null

export function mark(s) {
  const m = document.createElement("span");
  m.className = "mark";
  m.setAttribute("aria-hidden", "true");
  m.textContent = (s.name.match(/[a-z0-9]/) ?? ["?"])[0].toUpperCase();
  const img = (src) => {
    const i = new Image();
    i.src = src;
    i.alt = "";
    return i;
  };
  // The first time, the letter until the icon has loaded; after that the icon at once:
  // every redraw (a right-click, a refresh) showing the letter first made it flash.
  const show = (src) => {
    if (!src) return;
    const i = img(src);
    i.onload = () => m.replaceChildren(i);
  };
  const known = icons.get(s.name);
  if (known) m.replaceChildren(img(known));
  else if (icons.has(s.name)) return m;
  else
    invoke("icon", { site: s.name }).then(
      (src) => {
        icons.set(s.name, src);
        show(src);
      },
      () => icons.set(s.name, null),
    );
  return m;
}

let sortBy = localStorage.getItem("sortBy") ?? "name";
// How the list is split after your own groups: by parked folder, by type, or not at all.
let groupBy = localStorage.getItem("groupBy") ?? "folder";
export let showHidden = localStorage.getItem("showHidden") === "1";

/** Apps hidden from Anvil, shaped like sites, while "Show Hidden Applications" is on. They aren't served. */
function hiddenSites(): any[] {
  if (!showHidden) return [];
  return (snap.hidden ?? []).map((h) => ({
    ...h,
    hiddenApp: true,
    host: `${h.name}.${snap.tld}`,
    root: h.path,
    secure: false,
    linked: false,
    parked_in: h.path.slice(0, h.path.lastIndexOf("/")),
    modified: 0,
    php: null,
    php_pinned: false,
  }));
}
// Sidebar sections folded from their heading, by "mode:title".
export const folded = new Set(JSON.parse(localStorage.getItem("sections") ?? "[]"));

/** A sidebar section: a heading with a count that folds the rows under it.
 * While searching every section stays open. */
export function listSection(key, title, rows, count) {
  const open = !!filter || !folded.has(key);
  const head = el("button", "section-head");
  head.setAttribute("aria-expanded", String(open));
  head.append(el("span", "chevron", "›"), el("span", null, title), el("span", "section-count", count));
  const box = el("div", "rows");
  box.append(...rows);
  const inner = el("div");
  inner.append(box);
  const fold = el("div", open ? "fold" : "fold closed");
  fold.append(inner);
  head.onclick = () => {
    const opening = head.getAttribute("aria-expanded") !== "true";
    if (opening) folded.delete(key);
    else folded.add(key);
    localStorage.setItem("sections", JSON.stringify([...folded]));
    head.setAttribute("aria-expanded", String(opening));
    fold.classList.toggle("closed", !opening);
  };
  const h = el("h2");
  h.append(head);
  return [h, fold];
}
const wtFolded = new Set(JSON.parse(localStorage.getItem("wt-folded") ?? "[]")); // apps whose many worktrees were folded once
const collapsed = new Set(JSON.parse(localStorage.getItem("collapsed") ?? "[]"));

// Fold an app's worktrees in place (no re-render), so the slide can animate.
export function toggle(name, open = collapsed.has(name)) {
  if (open) collapsed.delete(name);
  else collapsed.add(name);
  localStorage.setItem("collapsed", JSON.stringify([...collapsed]));
  const parent = $(`.item[data-name="${CSS.escape(name)}"]`);
  if (!parent) return;
  parent.setAttribute("aria-expanded", String(open));
  $(".disclosure", parent).title = open ? "Hide worktrees" : "Show worktrees";
  parent.nextElementSibling?.classList.toggle("closed", !open);
}

/** A sidebar mark with its run state as a badge on the corner: `on`, `warm` (starting)
 * or `bad` (failed); none when stopped. Apps and services both use it. */
export function markBox(m, badge) {
  const box = el("span", "mark-box");
  box.append(m);
  if (badge) {
    const b = el("span", `run-badge ${badge}`);
    b.title = { on: "Running", warm: "Starting", bad: "Failed to start" }[badge];
    box.append(b);
    box.classList.add("running");
  }
  return box;
}

export function row(s, { child = false, kids = 0, path = true, kind: showKind = true } = {}) {
  const b = document.createElement("button");
  b.className = child ? "item child" : "item";
  if (s.hiddenApp) b.classList.add("hidden-app");
  b.dataset.name = s.name;
  b.setAttribute("aria-current", String(s.name === selected));
  if (child) {
    // A worktree: green while it runs, hollow when stopped.
    const run = ["running", "starting"].includes(snap.app_states[s.name]);
    b.append(el("span", `dot${run ? " on" : ""}`));
  }
  const text = document.createElement("span");
  text.className = "text";
  text.innerHTML = `<span class="name"></span><span class="sub"></span>`;
  // The folder's name: not every project is a site, so no address in the list.
  // A worktree is named by its branch.
  $(".name", text).textContent = child || s.worktree_of ? (s.branch ?? s.name) : s.name;
  // HTTPS shows as a lock after the name; it's turned on and off from the right-click menu.
  // While it changes only a spinner shows: words would be cut off here; the header says them.
  // Folders and Compose projects have no address: a leftover HTTPS flag means nothing there.
  if (s.hiddenApp || !serves(s)) {
    /* no lock */
  } else if (securing.has(s.name)) {
    const busy = el("span", "securing");
    busy.append(el("span", "spin-dot"));
    busy.title = s.secure ? "Turning off HTTPS…" : "Turning on HTTPS…";
    $(".name", text).append(busy);
  } else if (s.secure) {
    const lock = el("span", "lock");
    lock.innerHTML = LOCK;
    lock.title = "HTTPS";
    $(".name", text).append(lock);
  }
  // In a folder's section the path only repeats the heading; linked apps and groups keep it.
  if (path && !child) $(".sub", text).textContent = tilde(s.path) + (s.path.includes("\\") ? "\\" : "/");
  else $(".sub", text).remove();
  if (!path && !child) b.classList.add("compact");

  // Kind labels line up in one column: the version comes before the kind, and every
  // row ends in the same fixed slot (the aliases' "2 ›", or empty).
  const end = document.createElement("span");
  end.className = "end";
  // Only call out sites that differ from the default.
  if (s.php_pinned) {
    const ver = document.createElement("span");
    ver.className = "ver";
    ver.textContent = `PHP ${s.php}`;
    end.append(ver);
  }
  // A plain PHP app shows only its pinned version, not "PHP" twice.
  // Under a type heading the type would only repeat it.
  if (s.hiddenApp) end.append(el("span", "kind", "hidden"));
  else if (showKind && !child && s.kind !== "static" && !(s.kind === "php" && s.php_pinned)) {
    const kind = document.createElement("span");
    kind.className = `kind kind-${s.kind}`;
    kind.textContent = snap.kinds.find(([id]) => id === s.kind)?.[1] ?? s.kind;
    end.append(kind);
  }
  const d = el("span", "disclosure");
  if (kids) {
    const open = !collapsed.has(s.name);
    b.setAttribute("aria-expanded", String(open));
    d.title = open ? "Hide worktrees" : "Show worktrees";
    d.innerHTML = `${BRANCH}<span class="count"></span><span class="chevron" aria-hidden="true">›</span>`;
    $(".count", d).textContent = kids;
    d.onclick = (e) => {
      e.stopPropagation();
      toggle(s.name);
    };
  }
  end.append(d);
  if (child) b.append(text, end);
  else {
    const state = s.hiddenApp ? undefined : snap.app_states[s.name];
    // Green while anything of the app runs, amber while its server starts, red after a crash.
    const box = markBox(mark(s), state && { running: "on", starting: "warm", crashed: "bad" }[state]);
    if (state === "crashed") $(".run-badge", box).title = "A process crashed";
    b.append(box, text, end);
  }
  b.onclick = () => select(s.name);
  b.oncontextmenu = (e) => {
    e.preventDefault();
    hideCard();
    cardMuted = s.name;
    select(s.name);
    siteMenu(s);
  };
  b.onmouseenter = (e) => {
    // The list redraws while the mouse rests on it (a starting app polls): keep that row's card.
    if (cardFor === s.name || s.hiddenApp) return;
    hideCard();
    // Not while dragging, nor on a row whose context menu was opened until the mouse leaves it.
    if (e.buttons || cardMuted === s.name) return;
    cardFor = s.name;
    hoverTimer = setTimeout(() => showCard(s, b), 500);
  };
  b.onmouseleave = () => {
    hideCard();
    cardMuted = null;
  };
  b.onmousedown = hideCard;
  return b;
}

/* ---------- Hover card ---------- */

let hoverTimer: ReturnType<typeof setTimeout> | null = null;
let cardMuted: string | null = null; // the row a context menu was opened on
let cardFor: string | null = null; // the site whose card is shown or on its way
const cardProcs = new Map<string, any>(); // site name -> { at, req: promise of the `processes` overview }

function hideCard() {
  clearTimeout(hoverTimer ?? undefined);
  hoverTimer = cardFor = null;
  $(".hover-card")?.remove();
}

/** A short summary next to the list: kind, address, what runs, branch and last edit. */
function showCard(s, row) {
  const card = el("div", "hover-card");
  card.setAttribute("role", "tooltip");
  const draw = () => {
    const lines = [el("strong", null, s.name), el("span", "muted", kindLabel(s.kind))];
    if (serves(s)) lines.push(el("span", "hc-addr", url(s).replace("https://", "")));
    const o = cardProcs.get(s.name)?.data;
    const up = o
      ? [
          ...(o.rows ?? []).filter((r) => r.kind !== "site" && r.running).map((r) => r.name),
          ...(o.containers ?? []).filter((c) => c.state === "running").map((c) => c.service),
        ]
      : [];
    const state = snap.app_states[s.name];
    if (up.length) lines.push(el("span", "hc-run", `${up.length} running: ${up.join(", ")}`));
    else if (state) lines.push(el("span", "hc-run", { running: "Running", starting: "Starting", crashed: "A process crashed" }[state]));
    const g = late.gitCache ? late.gitCache.get(s.name) : null;
    if (g && !g.error && g.branch) lines.push(el("span", "branch", g.branch));
    const secs = touchedOf(s, () => card.isConnected && draw());
    if (secs) lines.push(el("span", "muted", `edited ${ago(new Date(secs * 1000))}`));
    card.replaceChildren(...lines);
  };
  // `processes` at most once per hover, kept ~10 s.
  const c = cardProcs.get(s.name);
  if (!c || Date.now() - c.at > 10000) {
    const entry = { at: Date.now(), data: c?.data };
    cardProcs.set(s.name, entry);
    invoke("processes", { site: s.name })
      .then(
        (o) => {
          entry.data = o;
        },
        () => {},
      )
      .then(() => card.isConnected && draw());
  }
  draw();
  const r = row.getBoundingClientRect();
  const list = $("#list").getBoundingClientRect();
  Object.assign(card.style, { left: `${list.right + 8}px`, top: `${Math.max(8, r.top)}px` });
  document.body.append(card);
}

/** "Laravel only" with a Clear button above a filtered list. */
export function filterNote(m, label) {
  const f = filterOf(m);
  if (!f.kind && !f.running) return null;
  const what = [f.kind && label(f.kind), f.running && "running"].filter(Boolean).join(", ");
  const note = el("div", "filter-note");
  note.append(el("span", null, `Showing ${what} only`));
  const clear = el("button", null, "Clear");
  clear.onclick = () => {
    Object.assign(f, { kind: null, running: false });
    localStorage.setItem("filters", JSON.stringify(filters));
    renderMain();
  };
  note.append(clear);
  return note;
}

/** First run: one clear next step instead of an empty pane. */
export function welcome(root) {
  const box = el("div", "welcome");
  const icon = el("img");
  icon.src = "icon.png";
  icon.alt = "";
  const add = el("button", "primary", "Add Projects Folder…");
  add.onclick = async () => {
    const path = await window.__TAURI__.dialog.open({ directory: true, title: "Every project inside this folder becomes an application" });
    if (path) await call("park_folder", { path });
  };
  const create = el("button", "link", "or create a new application");
  create.onclick = () => late.openNewApp();
  box.append(
    icon,
    el("h1", null, "Welcome to Anvil"),
    el("p", "muted", `Add the folder where your projects live. Each project in it gets its own .${snap.tld} address.`),
    add,
    create,
  );
  root.replaceChildren(box);
}

function renderList() {
  const list = $("#list");
  list.replaceChildren();
  const note = filterNote("apps", kindLabel);
  const hiddenApps = hiddenSites();
  if (note && (snap.sites.length || hiddenApps.length)) list.append(note);

  if (!snap.sites.length && !hiddenApps.length) {
    list.append(el("p", "empty", "No applications yet."));
    return;
  }

  // Worktrees nest under their app. A linked name pointing at a parked project's folder is
  // an alias: it shows in that app's header, not as a row.
  const apps = new Set(snap.sites.filter((s) => !s.linked && !s.worktree_of).map((s) => s.name));
  // A worktree whose app is gone stands alone, in its folder's section else Linked.
  const orphan = (s) => s.worktree_of && !apps.has(s.worktree_of);
  const parked = [...snap.sites.filter((s) => (!s.linked && !s.worktree_of) || (orphan(s) && s.parked_in)), ...hiddenApps];
  const wts = new Map<string, any[]>();
  for (const s of snap.sites) if (s.worktree_of && !orphan(s)) wts.set(s.worktree_of, [...(wts.get(s.worktree_of) ?? []), s]);
  const parkedPaths = new Set(parked.map((s) => s.path));
  const linked = snap.sites.filter((s) => (s.linked && !parkedPaths.has(s.path)) || (orphan(s) && !s.parked_in));

  const order = sortBy === "modified" ? (a, b) => b.modified - a.modified : (a, b) => a.name.localeCompare(b.name);

  const q = filter.toLowerCase();
  const f = filterOf("apps");
  const hit = (s) =>
    (!f.kind || s.kind === f.kind) &&
    (!f.running || ["running", "starting"].includes(snap.app_states[s.name])) &&
    // Name, folder or type: "django" finds every Django app.
    (!q || `${s.host} ${s.path} ${kindLabel(s.kind)}`.toLowerCase().includes(q));
  let count = 0;

  // `key` remembers the fold; folders and Linked keep their old keys.
  const section = (title, sites, withChildren, path = true, kind = true, key = `apps:${title}`) => {
    const rows: any[] = [];
    for (const s of [...sites].sort(order)) {
      const kids = withChildren ? (wts.get(s.name) ?? []).sort(order) : [];
      // More than five worktrees start folded; after that the toggle is remembered.
      if (kids.length > 5 && !wtFolded.has(s.name)) {
        wtFolded.add(s.name);
        collapsed.add(s.name);
        localStorage.setItem("wt-folded", JSON.stringify([...wtFolded]));
        localStorage.setItem("collapsed", JSON.stringify([...collapsed]));
      }
      const parentHit = hit(s);
      const shownKids = kids.filter((k) => parentHit || hit(k));
      if (!parentHit && !shownKids.length) continue;
      rows.push(row(s, { kids: kids.length, path, kind }));
      if (shownKids.length) {
        // Always rendered so folding can slide; while searching, matches stay open.
        const wrap = document.createElement("div");
        wrap.className = q || !collapsed.has(s.name) ? "kids" : "kids closed";
        const inner = document.createElement("div");
        inner.append(...shownKids.map((k) => row(k, { child: true })));
        wrap.append(inner);
        rows.push(wrap);
      }
    }
    if (!rows.length) return;
    count += rows.length;
    list.append(...listSection(key, title, rows, rows.filter((r) => r.classList.contains("item")).length));
  };

  // User groups first (Favorites leads), then one section per parked folder, then linked folders.
  const grouped = new Set(Object.values(snap.groups).flat());
  const groupNames = Object.keys(snap.groups).sort((a, b) => (a === "Favorites" ? -1 : b === "Favorites" ? 1 : a.localeCompare(b)));
  for (const g of groupNames) {
    section(
      g,
      parked.filter((s) => snap.groups[g].includes(s.name)),
      true,
      true,
      true,
      `apps:group:${g}`,
    );
  }
  const rest = [...parked, ...linked].filter((s) => !grouped.has(s.name));
  if (groupBy === "type") {
    const kinds = [...new Set(rest.map((s) => s.kind))].sort((a, b) => kindLabel(a).localeCompare(kindLabel(b)));
    for (const k of kinds)
      section(
        kindLabel(k),
        rest.filter((s) => s.kind === k),
        true,
        true,
        false,
        `apps:type:${k}`,
      );
  } else if (groupBy === "none") {
    section("Applications", rest, true, true, true, "apps:all");
  } else {
    const dirs = [...new Set(parked.map((s) => s.parked_in))].sort();
    for (const dir of dirs) {
      section(
        tilde(dir),
        parked.filter((s) => s.parked_in === dir && !grouped.has(s.name)),
        true,
        false,
      );
    }
    section("Linked", linked, false);
  }

  if (!count) {
    const p = document.createElement("p");
    p.className = "empty";
    const what = f.kind ? `${kindLabel(f.kind)} applications` : "applications";
    p.textContent = filter ? `No ${what} match “${filter}”.` : `No ${f.running ? "running " : ""}${what}.`;
    list.append(p);
  }
}

/* ---------- Context menu ---------- */

export const M = window.__TAURI__.menu;

/** Turn HTTPS on or off, showing it on the row until the web server has reloaded. */
async function setSecure(s) {
  securing.add(s.name);
  renderList();
  drawMeta();
  try {
    await call("set_secure", { site: s.name, on: !s.secure });
  } finally {
    securing.delete(s.name);
    renderList();
    drawMeta();
  }
}

async function siteMenu(s) {
  const sep = () => M.PredefinedMenuItem.new({ item: "Separator" });
  const groupOf = Object.keys(snap.groups).find((g) => snap.groups[g].includes(s.name));
  const groups = [...new Set(["Favorites", ...Object.keys(snap.groups)])];

  if (s.hiddenApp) {
    (await M.Menu.new({ items: [await M.MenuItem.new({ text: "Show in Anvil", action: () => unhide(s) })] })).popup();
    return;
  }
  const items: any[] = [await M.MenuItem.new({ text: "Open Folder", action: () => open("finder", s.name) }), await sep()];
  if (serves(s)) {
    items.push(await M.CheckMenuItem.new({ text: "Use HTTPS", checked: s.secure, action: () => setSecure(s) }), await sep());
  }
  if (!s.linked) {
    items.push(await M.MenuItem.new({ text: "Add Alias…", action: () => addAlias(s) }));
    const groupItems = await Promise.all(
      groups.map((g) =>
        M.CheckMenuItem.new({
          text: g,
          checked: g === groupOf,
          action: () => call("set_group", { site: s.name, group: g === groupOf ? null : g }),
        }),
      ),
    );
    groupItems.push(await sep(), await M.MenuItem.new({ text: "New Group…", action: () => newGroup(s) }));
    items.push(await M.Submenu.new({ text: "Add to Group", items: groupItems }), await sep());
  }
  if (s.worktree_of) {
    items.push(await M.MenuItem.new({ text: "Remove Worktree…", action: () => removeWorktree(s) }), await sep());
  } else if (mayHaveWorktrees(s) && (await invoke("git_info", { site: s.name }).catch(() => null))) {
    items.push(await M.MenuItem.new({ text: "New Worktree…", action: () => newWorktree(s) }), await sep());
  }
  items.push(
    await M.MenuItem.new({
      text: s.linked ? "Remove Link" : "Hide from Anvil",
      action: async () => {
        await call("remove_site", { site: s.name });
        if (!s.linked) toast(`Hid ${s.name}.${snap.tld}`, () => call("unhide_site", { site: s.name }));
      },
    }),
  );
  (await M.Menu.new({ items })).popup();
}

function setView(key, value) {
  if (key === "sortBy") sortBy = value;
  else groupBy = value;
  localStorage.setItem(key, value);
  renderList();
}

const unhide = (s) => call("unhide_site", { site: s.name });

function setShowHidden(on) {
  showHidden = on;
  localStorage.setItem("showHidden", on ? "1" : "0");
  renderList();
  renderSite();
}

export type Ask = {
  title: string;
  /** The text field's name; none: a sheet that only confirms (`body`). */
  field?: string;
  /** An empty field is an answer too (`preview` says what it does). */
  optional?: boolean;
  placeholder?: string;
  value?: string;
  /** What the text will make, under the field. */
  preview?: (v: string) => string;
  /** Checkboxes under the field, each with a line about it. */
  checks?: { label: string; checked?: boolean; note?: string }[];
  body?: Node;
  /** An error it opens with, inline. */
  error?: string;
  ok: string;
  /** Does the work: a failure shows in the sheet, which stays open. */
  submit: (value: string, checked: boolean[]) => Promise<unknown>;
};

/** A prompt as a sheet, the New application sheet sized to what it asks: its title, a text
 * field and checkboxes, an inline error, Cancel and `ok`. Enter runs `submit`; Esc and Cancel
 * close it. Resolves to the text and checkboxes once `submit` succeeded, else null. */
export function ask(o: Ask): Promise<{ value: string; checked: boolean[] } | null> {
  const dlg = $("#ask");
  const input = $("#ask-input");
  const hint = $("#ask-hint");
  const error = $("#ask-error");
  $("#ask-label").textContent = o.title;
  $("#ask-body").replaceChildren(...(o.body ? [o.body] : []));
  $("#ask-field").hidden = !o.field;
  $("#ask-name").textContent = o.field ?? "";
  input.placeholder = o.placeholder ?? "";
  input.value = o.value ?? "";
  const boxes = (o.checks ?? []).map((c) => Object.assign(el("input"), { type: "checkbox", checked: !!c.checked }));
  $("#ask-checks").replaceChildren(
    ...(o.checks ?? []).map((c, i) => {
      const row = el("div", "field");
      const label = el("label", "opt-check");
      label.append(boxes[i], ` ${c.label}`);
      row.append(el("span"), label);
      if (c.note) row.append(el("small", null, c.note));
      return row;
    }),
  );
  const ok = $("#ask-ok");
  ok.textContent = o.ok;
  error.textContent = o.error ?? "";
  error.hidden = !o.error;
  const show = () => (hint.textContent = o.preview ? o.preview(input.value.trim()) : "");
  input.oninput = show;
  show();
  dlg.showModal();
  // A sheet that only confirms starts on Cancel: a held Return doesn't run anything.
  (o.field ? input : $("#ask-cancel")).focus();
  return new Promise((resolve) => {
    const finish = (answer: { value: string; checked: boolean[] } | null) => {
      dlg.onclose = null;
      dlg.close();
      resolve(answer);
    };
    // Esc closes the dialog by itself.
    dlg.onclose = () => resolve(null);
    $("#ask-cancel").onclick = () => finish(null);
    $("#ask-form").onsubmit = async (e) => {
      e.preventDefault();
      const value = input.value.trim();
      if (o.field && !value && !o.optional) return input.focus();
      const checked = boxes.map((b) => b.checked);
      ok.disabled = true;
      try {
        await o.submit(value, checked);
        finish({ value, checked });
      } catch (err) {
        error.textContent = String(err);
        error.hidden = false;
      } finally {
        ok.disabled = false;
      }
    };
  });
}

/** `call` that throws instead of showing the banner: for a sheet that says it itself. */
export async function callOrThrow(cmd: string, args?: Record<string, unknown>) {
  if (!(await askSnap(cmd, args))) return;
  showError(null, cmd);
  render();
}

async function addAlias(s) {
  await ask({
    title: `New domain for ${s.name}`,
    field: "Domain",
    placeholder: `e.g. admin.${s.name}`,
    preview: (v) => `Opens ${(v || `admin.${s.name}`).replace(new RegExp(`\\.${snap.tld}$`), "")}.${snap.tld}`,
    ok: "Add",
    submit: (alias) => callOrThrow("add_alias", { site: s.name, alias }),
  });
}

/** An app's own folder, linked or parked; not a worktree, nor an alias (a link to another app's folder). */
const mayHaveWorktrees = (s) =>
  !s.worktree_of && !(s.linked && snap.sites.some((o) => o.name !== s.name && !o.linked && o.path === s.path));
late.mayHaveWorktrees = mayHaveWorktrees;

const afterRunning = new Set<string>(); // worktrees whose after-create commands run
const afterErrors = new Map<string, string>(); // the last run's failure, across re-renders

/** The New worktree sheet: the branch, whether it's new, and "Copy the database too" as the
 * app's hooks set it (only when main's database can be copied). Once it's made, its hooks'
 * after-create commands are offered in a second sheet and run only on Run. */
async function newWorktree(s) {
  const db = await invoke("worktree_new_db", { site: s.name }).catch(() => null);
  const copyable = !!db?.main_database && db.can_copy;
  let made: string | null = null;
  const answer = await ask({
    title: `New worktree of ${s.name}`,
    field: "Branch",
    placeholder: "feature/login",
    preview: (v) => `Its address: ${s.name}--${(v || "feature/login").toLowerCase().replace(/[^a-z0-9.-]/g, "-")}.${snap.tld}`,
    checks: [
      { label: "New branch, from the app’s current one" },
      ...(copyable
        ? [
            {
              label: "Copy the database too",
              checked: db.copy,
              note: `Off: an empty database of its own. On: a copy of ${db.main_database}.`,
            },
          ]
        : []),
    ],
    ok: "Create",
    submit: async (branch, [isNew, copy]) => {
      await callOrThrow("worktree_add", { site: s.name, branch, new: isNew, copyDb: copyable ? copy : null });
      made = snap.sites.find((x) => x.worktree_of === s.name && x.branch === branch)?.name ?? null;
    },
  });
  if (!answer || !made) return;
  const site: string = made;
  select(site);
  // Worked out from the new worktree, and exactly these run. A failure isn't "none": it says so.
  let commands: string[] = [];
  const fetchList = async () => {
    commands = await invoke("worktree_after_create_list", { site });
  };
  const failed = await fetchList().then(
    () => null,
    (e) => String(e),
  );
  if (failed !== null) {
    const retried = await ask({
      title: `${site} is ready`,
      body: el("p", null, "Anvil couldn't work out the commands to run in it."),
      error: failed,
      ok: "Try again",
      submit: fetchList,
    });
    if (!retried) return;
  }
  if (!commands.length) return;
  const list = el("ol", "ask-list");
  list.append(...commands.map((c) => el("li", null, c)));
  const body = el("div");
  body.append(el("p", null, "Run these in it now?"), list);
  // Started here and left to run: the Overview says it runs, and how it ended if it failed.
  const run = () => {
    afterRunning.add(site);
    afterErrors.delete(site);
    renderSite();
    invoke("worktree_after_create", { site, commands })
      .catch((e) => afterErrors.set(site, String(e)))
      .finally(() => {
        afterRunning.delete(site);
        call("state");
      });
    return Promise.resolve();
  };
  await ask({ title: `${site} is ready`, body, ok: "Run", submit: run });
}
late.newWorktree = newWorktree;

const count = (n: number, what: string) => `${n} ${what}${n === 1 ? "" : "s"}`;

/** Confirm under the worktree's row: what would be lost, and whether its database copy goes too.
 * Uncommitted changes or unpushed commits (or no way to tell) make it "Remove anyway". */
async function removeWorktree(s) {
  const [info, losses] = await Promise.all([
    invoke("git_info", { site: s.name }).catch(() => null),
    invoke<string[]>("worktree_losses", { site: s.name }).catch(() => []),
  ]);
  const copies = snap.worktrees?.[s.name]?.copies ?? [];
  const said = [`Remove ${s.name}? Its folder goes${s.branch ? ", its branch stays" : ""}.`];
  if (!info) said.push("Anvil couldn't check it for uncommitted changes.");
  if (info?.changed) said.push(`It has ${count(info.changed, "uncommitted change")}, lost with the folder.`);
  if (info?.ahead) said.push(`It has ${count(info.ahead, "unpushed commit")}, kept on ${s.branch ?? "its branch"}.`);
  const force = !info || info.changed > 0 || info.ahead > 0;
  const pop = el("div", "pop-confirm wide");
  pop.append(el("p", null, said.join(" ")));
  // Git ignores these, so nothing above counts them: the SQLite files, an edited .env.
  if (losses.length) pop.append(el("p", null, `These will be deleted with the folder: ${losses.join(", ")}.`));
  let drop: HTMLInputElement | null = null;
  if (copies.length) {
    const label = el("label");
    drop = Object.assign(el("input"), { type: "checkbox" });
    label.append(drop, ` Also delete ${copies.join(", ")}`);
    pop.append(label);
  }
  const no = el("button", null, "Cancel");
  const yes = el("button", "primary", force ? "Remove anyway" : "Remove");
  pop.append(no, yes);
  const anchor = $(`#list [data-name="${CSS.escape(s.name)}"]`) ?? $("#site .site-name");
  const close = popAt(anchor, pop, yes);
  no.onclick = close;
  yes.onclick = async () => {
    if (force) {
      close();
      call("worktree_remove", { site: s.name, dropDb: !!drop?.checked, force });
      return;
    }
    // Refused (commits on no branch, say): said here, and the button becomes Remove anyway.
    try {
      await askSnap("worktree_remove", { site: s.name, dropDb: !!drop?.checked, force: false });
      close();
      render();
    } catch (e) {
      $("p", pop).textContent = `${String(e)}.`;
      yes.textContent = "Remove anyway";
      yes.onclick = () => {
        close();
        call("worktree_remove", { site: s.name, dropDb: !!drop?.checked, force: true });
      };
    }
  };
}

async function newGroup(s) {
  await ask({
    title: `Group for ${s.name}`,
    field: "Group",
    placeholder: "Group name",
    ok: "Add",
    submit: (group) => callOrThrow("set_group", { site: s.name, group }),
  });
}

export let tab = (localStorage.getItem("tab") ?? "overview").replace(/^watch$/, "dev");
let wanted = tab; // the tab the user chose

/** A tab is always selected: the chosen one when this app shows it, else the first shown.
 * Tabs appear and disappear as their data arrives (Git, Dev, Containers, Logs). */
const watchedTabs = new WeakSet<Element>();
function keepTabSelected(root) {
  const bar = $(".tabs", root);
  const reconcile = () => {
    const shown = [...bar.querySelectorAll("[role=tab]:not([hidden])")] as HTMLElement[];
    if (!shown.length) return;
    const has = (t) => shown.some((b) => b.dataset.tab === t);
    if (has(wanted)) {
      if (tab !== wanted) showTab(root, wanted, true);
    } else if (!has(tab)) showTab(root, shown[0].dataset.tab, true);
  };
  if (!watchedTabs.has(bar)) {
    watchedTabs.add(bar);
    new MutationObserver(reconcile).observe(bar, { subtree: true, attributes: true, attributeFilter: ["hidden"] });
  }
  reconcile();
}
const kindLabel = (k) => snap.kinds.find(([id]) => id === k)?.[1] ?? k;
const RUNTIME = {
  next: "Node.js",
  nuxt: "Node.js",
  vite: "Node.js",
  node: "Node.js",
  rust: "Rust",
  go: "Go",
  dotnet: ".NET",
  django: "Python",
  fastapi: "Python",
  flask: "Python",
};

/** Show a tab. The user's choice is remembered; a `fallback` (the app lacks the chosen
 * tab) shows another without forgetting it, so the choice comes back with an app that has it. */
export function showTab(root, name, fallback = false) {
  if (!fallback) {
    wanted = name;
    localStorage.setItem("tab", name);
  }
  tab = name;
  // One stop in the Tab order; the arrow keys move between tabs.
  for (const b of root.querySelectorAll(".tabs [role=tab]")) {
    b.setAttribute("aria-selected", String(b.dataset.tab === name));
    b.tabIndex = b.dataset.tab === name ? 0 : -1;
  }
  for (const p of root.querySelectorAll("[role=tabpanel]")) p.hidden = p.dataset.panel !== name;
  $(".res-reload", root).hidden = name !== "resources";
  window.logsTab?.(root, name === "logs");
  window.requestsTab?.(root, name === "requests");
  window.mailTab?.(root, name === "mail");
  if (name === "dev") late.devTab?.();
  if (name === "containers") late.containersTab?.();
  // Measuring (du, lsof, docker) only happens when someone looks at it.
  if (name === "resources" && selected) loadResources(selected);
}

/** "just now", "5 min ago", "2 h ago", "3 d ago", then the date (header and logs). */
export function ago(d) {
  const s = Math.round((Date.now() - d) / 1000);
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 7 * 86400) return `${Math.round(s / 86400)} d ago`;
  return d.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: d.getFullYear() === new Date().getFullYear() ? undefined : "numeric",
  });
}

const touchedCache = new Map<string, { at: number; secs: number | null }>(); // site name -> { at: ms when asked, secs: newest edit }

/** The newest edit (seconds) as known; asks again after 30 s and calls `done` with the answer. */
function touchedOf(s, done) {
  const t = touchedCache.get(s.name);
  if (!t || Date.now() - t.at > 30000) {
    touchedCache.set(s.name, { at: Date.now(), secs: t?.secs ?? null });
    invoke("touched", { site: s.name })
      .catch(() => null)
      .then((secs) => {
        touchedCache.set(s.name, { at: Date.now(), secs });
        done();
      });
  }
  return t?.secs;
}

/** The line under the name: address (+ aliases), type, last edit and git branch. */
export function drawMeta() {
  const box = $("#site .meta");
  const s = snap?.sites.find((x) => x.name === selected);
  if (!box || !s) return;
  const parts: any[] = [];
  if (serves(s)) {
    const addr = el("span", "addr");
    const link = el("a", "site-url", url(s).replace("https://", ""));
    link.href = "#";
    link.onclick = (e) => {
      e.preventDefault();
      if (snap.running) open("browser");
    };
    // A stopped server can't answer: the address looks and acts off.
    link.classList.toggle("off", !snap.running);
    link.title = snap.running ? "Open in browser (⌘O)" : "Start Anvil to open this address";
    if (s.secure && !securing.has(s.name)) {
      const lock = el("span", "lock");
      lock.innerHTML = LOCK;
      lock.title = "HTTPS";
      addr.append(lock);
    }
    addr.append(link);
    if (securing.has(s.name)) {
      const busy = el("span", "securing", s.secure ? "Turning off HTTPS…" : "Turning on HTTPS…");
      busy.prepend(el("span", "spin-dot"));
      addr.append(busy);
    }
    // Aliases are linked names for the same folder.
    // A worktree's are the hosts derived from its app's aliases.
    const aliases = s.worktree_of
      ? s.aliases.map((host) => ({ host, secure: s.secure }))
      : s.linked
        ? []
        : snap.sites.filter((k) => k.linked && k.path === s.path);
    if (aliases.length) {
      const chip = el("button", "alias-chip", `+${aliases.length}`);
      chip.title = "All addresses";
      chip.onclick = () => aliasPop(chip, [s, ...aliases]);
      addr.append(chip);
    }
    parts.push(addr);
  }
  parts.push(el("span", null, kindLabel(s.kind)));
  const secs = touchedOf(s, () => selected === s.name && drawMeta());
  if (secs) {
    const d = new Date(secs * 1000);
    const edited = el("span", null, `edited ${ago(d)}`);
    edited.title = `Last file change ${d.toLocaleString()}`;
    parts.push(edited);
  }
  // git.js may not have loaded when the first state arrives.
  const g = late.gitCache ? late.gitCache.get(s.name) : null;
  if (g && !g.error && g.branch) parts.push(el("span", "branch", g.branch));
  box.replaceChildren(...parts.flatMap((p, i) => (i ? [el("span", "sep", "·"), p] : [p])));
  drawHttps(s);
}

/** The Settings tab's HTTPS switch: on or off, and while it changes, where it goes
 * with a spinner beside it. */
function drawHttps(s) {
  const box = $("#app-https");
  if (!box) return;
  const busy = securing.has(s.name);
  box.checked = busy ? !s.secure : s.secure;
  box.disabled = busy;
  const note = box.nextElementSibling;
  note.hidden = !busy;
  $(".securing-text", note).textContent = s.secure ? "Turning off…" : "Turning on…";
  box.onchange = () => {
    const now = snap.sites.find((x) => x.name === s.name);
    if (now && !securing.has(now.name)) setSecure(now);
  };
}

/** Every address of an app, each with its own Open. */
function aliasPop(anchor, sites) {
  const pop = el("div", "alias-pop");
  const buttons = sites.map((a) => {
    const row = el("div", "alias-row");
    const go = el("button", null, "Open");
    go.onclick = () => {
      close();
      invoke("open_url", { url: url(a) }).catch((e) => showError(String(e)));
    };
    row.append(el("span", "alias-url", url(a).replace("https://", "")), go);
    pop.append(row);
    return go;
  });
  const close = popAt(anchor, pop, buttons[0]);
}

/** A small bubble under `anchor`, right-aligned with it. Esc or a click elsewhere
 * closes it; the close function is returned. */
export function popAt(anchor: Element, pop: HTMLElement, focus?: HTMLElement | null) {
  document.querySelector(".pop")?.remove();
  pop.classList.add("pop");
  pop.setAttribute("role", "dialog");
  const close = () => {
    pop.remove();
    document.removeEventListener("mousedown", outside, true);
  };
  const outside = (e) => !pop.contains(e.target) && close();
  pop.addEventListener("keydown", (e) => {
    if (e.key === "Escape") close();
  });
  const r = anchor.getBoundingClientRect();
  Object.assign(pop.style, { top: `${r.bottom + window.scrollY + 6}px`, right: `${document.documentElement.clientWidth - r.right}px` });
  document.body.append(pop);
  document.addEventListener("mousedown", outside, true);
  focus?.focus();
  return close;
}

/** Finder, the terminal and the editor as outline icons; which app each opens is in the tooltip. */
function drawOpenWith(root) {
  const label = (a) =>
    a
      .split("/")
      .pop()
      .replace(/\.app$/, "");
  const apps = { finder: "Finder", terminal: label(snap.terminal ?? "Terminal"), editor: snap.editor && label(snap.editor) };
  for (const b of root.querySelectorAll(".open-with [data-open]")) {
    const name = apps[b.dataset.open];
    b.hidden = !name;
    if (!name) continue;
    b.title = `Open in ${name}${b.dataset.open === "editor" ? " (⌘E)" : ""}`;
    b.setAttribute("aria-label", String(b.title));
    // Plain outline icons here, like the rest of the toolbar; the app's name is in the tooltip.
  }
}

export function renderSite() {
  const root = $("#site");
  const s = snap.sites.find((x) => x.name === selected) ?? hiddenSites().find((x) => x.name === selected);
  if (!snap.sites.length) return welcome(root);
  if (!s) {
    root.innerHTML = `<div class="placeholder">Select an application</div>`;
    return;
  }
  const process = PROCESS_KINDS.has(s.kind);

  root.replaceChildren($("#site-tpl").content.cloneNode(true));
  $(".site-name", root).textContent = s.name;
  $(".site-title .mark", root).replaceWith(Object.assign(mark(s), { className: "mark big" }));
  if (s.hiddenApp) {
    // Not served: the header and a notice, no tabs.
    for (const x of root.querySelectorAll(".site-meta, .head-side, .run-error, .tabs, [role=tabpanel]")) x.remove();
    const note = el("p", "hidden-note", "Hidden from Anvil — not served.");
    const show = el("button", "inline-action", "Show in Anvil");
    show.onclick = () => unhide(s);
    note.append(show);
    root.append(note);
    return;
  }
  drawMeta();
  drawOpenWith(root);
  for (const b of root.querySelectorAll("[data-open]")) b.onclick = () => open(b.dataset.open);
  for (const el of root.querySelectorAll(".php-only")) el.hidden = process;
  for (const el of root.querySelectorAll(".process-only")) el.hidden = !process;

  // Caddy serves neither a compose project nor a plain folder.
  $('.tabs [data-tab="requests"]', root).hidden = s.kind === "compose" || s.kind === "folder";
  // Only while the mail catcher is on (Settings › Plugins); its switch lives in the tab.
  $('.tabs [data-tab="mail"]', root).hidden = !snap.catcher || s.kind === "compose" || s.kind === "folder";
  for (const b of root.querySelectorAll(".tabs [role=tab]")) b.onclick = () => showTab(root, b.dataset.tab);
  $(".tabs", root).onkeydown = (e) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const tabs = [...root.querySelectorAll(".tabs [role=tab]:not([hidden])")];
    const i = tabs.findIndex((t) => t.dataset.tab === tab);
    const next = tabs[(i + (e.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length];
    showTab(root, next.dataset.tab);
    next.focus();
  };
  showTab(root, wanted, true);
  keepTabSelected(root);

  // Overview: the type and what runs it in one line, e.g. "Laravel on PHP 8.4".
  const engine = process
    ? `${RUNTIME[s.kind]}${s.runtime_version && s.kind !== "dotnet" && s.kind !== "rust" ? ` ${s.runtime_version}` : ""}${s.runtime_version ? "" : " (not installed)"}`
    : s.php
      ? `PHP ${s.php}${s.php_pinned ? "" : fromFile(s, "php") === s.php ? " (.anvil.toml)" : " (default)"}`
      : PHP_KINDS.has(s.kind)
        ? "PHP, none installed"
        : null;
  const kind = s.kind === "static" ? "Static files" : kindLabel(s.kind);
  const runs =
    s.kind === "compose" ? "Docker Compose" : !engine || kind === engine.split(" ")[0] ? (engine ?? kind) : `${kind} on ${engine}`;
  $('[data-field="runtime"]', root).textContent = s.kind === s.kind_detected ? runs : `${runs} (type set by you)`;
  // A plain folder runs on nothing; its path row says what it is.
  if (s.kind === "folder") $('[data-field="runtime"]', root).closest(".row")?.remove();
  if (PHP_KINDS.has(s.kind) && !s.php && !fromFile(s, "php")) {
    const install = el("button", "inline-action", "Install PHP 8.4");
    install.onclick = async () => {
      install.disabled = true;
      install.textContent = "Installing PHP 8.4…";
      // Returns the Settings view, not a snapshot: refresh the state afterwards.
      await invoke("php_install", { version: "8.4" }).catch((e) => showError(String(e)));
      call("state");
    };
    $('[data-field="runtime"]', root).append(" ", install);
  }
  projectNeeds(s, root);
  worktreeLines(s, root);
  leftoverLines(s, root);
  $('[data-field="path"]', root).textContent = tilde(s.path);
  $('[data-field="root"]', root).textContent = tilde(s.root);
  // Nothing is served from a folder or a Compose project: no web root, no HTTPS.
  if (!serves(s)) {
    $('[data-field="root"]', root).closest(".row")?.remove();
    for (const x of root.querySelectorAll(".serves-only")) x.remove();
  }

  if (process) renderRun(s, root);
  if (s.kind === "django") djangoHint(root);
  if (s.kind === "next" && s.host !== `${s.name}.${snap.tld}`) nextHint(s, root);

  // Settings
  const kindSel = $("#kind", root);
  kindSel.add(new Option(`Detected (${kindLabel(s.kind_detected)})`, ""));
  for (const [id, name] of snap.kinds) kindSel.add(new Option(name, id));
  kindSel.value = s.kind === s.kind_detected ? "" : s.kind;
  kindSel.onchange = () => call("set_app", kindSel.value ? { site: s.name, kind: kindSel.value } : { site: s.name, reset: ["kind"] });

  const php = $("#php", root);
  // Unset follows the project's .anvil.toml when its version is installed, else the default.
  const filePhp = fromFile(s, "php");
  php.add(
    filePhp && snap.php_installed.includes(filePhp)
      ? new Option(`From .anvil.toml (${filePhp})`, "")
      : new Option(`Default (${snap.php_default ?? "none"})`, ""),
  );
  for (const v of snap.php_installed) php.add(new Option(`PHP ${v}`, v));
  php.value = s.php_pinned ? s.php : "";
  php.onchange = () => call("set_php", { site: s.name, version: php.value || null });

  if (!process && serves(s)) webRoot(s, root);
  if (!process && s.php) appExtensions(s, root);
  else root.querySelector(".app-exts-row")?.remove();

  // Per-app php.ini overrides; placeholders show what applies without one.
  const base = { memory_limit: "512M", upload_max_filesize: "100M", max_execution_time: "120" };
  for (const input of root.querySelectorAll("[data-ini]")) {
    const key = input.dataset.ini;
    input.value = s.ini[key] ?? "";
    input.placeholder = `${snap.ini[key] ?? base[key]} (default)`;
    input.onchange = async () => {
      const value = input.value.trim() || null;
      await call("set_site_ini", { site: s.name, key, value });
      if (key === "upload_max_filesize") await call("set_site_ini", { site: s.name, key: "post_max_size", value });
    };
  }
  const known = new Set([...Object.keys(base), "post_max_size"]);
  const extra = $("#ini-extra", root);
  extra.value = Object.entries(s.ini)
    .filter(([k]) => !known.has(k))
    .map(([k, v]) => `${k} = ${v}`)
    .join("\n");
  extra.onchange = async () => {
    const wanted = new Map();
    for (const line of extra.value.split("\n")) {
      const m = line.match(/^\s*([^=\s]+)\s*=\s*(.*?)\s*$/);
      if (m) wanted.set(m[1], m[2]);
    }
    for (const k of Object.keys(s.ini)) {
      if (!known.has(k) && !wanted.has(k)) await call("set_site_ini", { site: s.name, key: k, value: null });
    }
    for (const [k, v] of wanted) {
      if (s.ini[k] !== v) await call("set_site_ini", { site: s.name, key: k, value: v });
    }
  };

  // Resources (also feeds the Overview's disk and services rows)
  $(".res-reload", root).onclick = () => {
    freedNote.delete(s.name);
    loadResources(s.name, true);
  };
  window.renderGit?.(s);
  window.renderProcesses?.(s);
  window.renderSiteServices?.(s, root);
  window.renderShare?.(s, root);
  window.renderPackages?.(s, root);
  if (mayHaveWorktrees(s)) renderHooks(s, root);
}

const rootErrors = new Map<string, [string, string]>(); // app -> [what was typed, why refused]
const appExtErrors = new Map<string, string>(); // app -> why its last extension change failed
const appExtBuilding = new Set<string>(); // `<app>@<extension>` being turned on (maybe built)

/** The Settings tab's PHP extensions: the catalog as switches, as in Settings › PHP, for
 * this app only. One on for every app on its version shows on and held, saying where. One
 * on without a build for the app's PHP shows off, "Not built"; on builds it. */
function appExtensions(s, root) {
  const cell = $(".app-exts", root);
  cell.replaceChildren();
  const line = el("div", "ver-exts app-ext-list");
  const shared = snap.php_ext?.[s.php] ?? [];
  const built = snap.php_ext_built?.[s.php];
  for (const name of snap.php_ext_catalog ?? []) {
    const forAll = shared.includes(name);
    const building = appExtBuilding.has(`${s.name}@${name}`);
    const on = forAll || s.ext.includes(name);
    const unbuilt = on && !!built && !built.includes(name) && !building;
    const label = el("label", "ver-ext");
    const input = Object.assign(document.createElement("input"), {
      type: "checkbox",
      checked: on && !unbuilt,
      disabled: forAll || building,
    });
    input.setAttribute("role", "switch");
    input.onchange = () => setAppExt(s.name, name, input.checked);
    if (forAll) label.title = `On for every app on PHP ${s.php} (Settings › PHP)`;
    label.append(input, el("span", "ver-ext-name", name));
    if (building) label.append(el("span", "muted", "Building…"));
    if (unbuilt) label.append(el("span", "muted", "Not built"));
    line.append(label);
  }
  cell.append(line);
  const why = appExtErrors.get(s.name);
  if (!why) return;
  const error = el("div", "ver-error app-ext-error");
  error.append(el("span", null, why));
  if (why.includes("xcode-select --install")) {
    const tools = el("button", "inline-action", "Install command line tools");
    tools.onclick = () => invoke("xcode_tools_install");
    error.append(tools);
  }
  cell.append(error);
}

/** On builds it first unless built for the app's PHP, then the web server restarts. */
async function setAppExt(site: string, name: string, on: boolean) {
  const key = `${site}@${name}`;
  if (appExtBuilding.has(key)) return;
  if (on) appExtBuilding.add(key);
  appExtErrors.delete(site);
  render();
  try {
    await askSnap("app_ext", { site, name, on });
  } catch (e) {
    appExtErrors.set(site, String(e));
  }
  appExtBuilding.delete(key);
  render();
}

/** The Settings tab's Web root: a folder inside the project, the detected one as the
 * placeholder, Reset when set; a refusal stays under the field with what was typed. */
function webRoot(s, root) {
  const input = $("#app-root", root);
  const reset = $(".root-reset", root);
  const fail = $(".root-error", root);
  const failed = rootErrors.get(s.name);
  input.value = failed?.[0] ?? s.root_custom ?? "";
  input.placeholder = s.root_detected === "." ? "Project folder (detected)" : `${s.root_detected} (detected)`;
  fail.textContent = failed?.[1] ?? "";
  fail.hidden = !failed;
  reset.hidden = !s.root_custom;
  const save = async (dir: string | null) => {
    try {
      const ok = await askSnap("set_root", { site: s.name, dir });
      rootErrors.delete(s.name);
      if (ok) render();
    } catch (e) {
      rootErrors.set(s.name, [input.value, String(e)]);
      fail.textContent = String(e);
      fail.hidden = false;
    }
  };
  input.onchange = () => save(input.value.trim() || null);
  reset.onclick = () => save(null);
}

const FROM = (from: string) => (from === "this app" ? "Set for this app" : `From ${from === "Settings" ? "Settings › Worktrees" : from}`);

/** The Settings tab's Worktrees section, for a git repository: each hook as it applies, where
 * it comes from, changed here for this app only; Use preset drops this app's own. */
async function renderHooks(s, root) {
  const box = $(".wt-hooks", root);
  let v;
  try {
    v = await invoke("worktree_hooks", { site: s.name });
  } catch {
    return; // not a git repository: no worktrees to set up
  }
  if (box.contains(document.activeElement)) return; // being typed in
  const head = el("dd", "muted", `${v.hooks.preset} preset, edited in Settings › Worktrees`);
  if (Object.keys(v.own).length) {
    const reset = el("button", "inline-action", "Use preset");
    reset.onclick = () =>
      invoke("set_app_hooks", { site: s.name, field: null, value: null }).then(
        () => renderHooks(s, root),
        (e) => showError(String(e)),
      );
    head.append(" ", reset);
  }
  // A layer with a mistake (a typo in config.toml or .anvil.toml): one line each.
  for (const p of v.hooks.problems ?? []) {
    const line = el("small", "error-text", p);
    head.append(line);
  }
  const top = el("div", "row");
  top.append(el("dt", null, "Worktrees"), head);
  const rows = FIELDS.map(([field, name]) => {
    const row = el("div", "row row-top");
    const dd = el("dd");
    const fail = el("small", "error-text");
    fail.hidden = true;
    const save = (value: unknown) => invoke("set_app_hooks", { site: s.name, field, value }).then(() => renderHooks(s, root));
    dd.append(control(field, v.hooks, save, fail), el("small", null, FROM(v.hooks.from[field])), fail);
    row.append(el("dt", null, name), dd);
    return row;
  });
  box.replaceChildren(top, ...rows);
  box.hidden = false;
}

/** Django checks the host name and, behind HTTPS, the scheme: two settings it needs once. */
/** The version the app's .anvil.toml asks for, if it asks. */
function fromFile(s, kind: string): string | null {
  return snap.projects?.[s.name]?.find((n) => n.kind === kind)?.version ?? null;
}

const installing = new Set<string>(); // "php:8.3" while it installs, across re-renders
const needExtErrors = new Map<string, string>(); // app -> why the line's Add of an extension failed

/** One quiet line when the app's .anvil.toml asks for something Anvil doesn't have yet:
 * Install for PHP and Node.js, Add opens the add-service sheet with the kind and version,
 * Add for an extension turns it on for this app (built first unless built). */
function projectNeeds(s, root) {
  const missing = (snap.projects?.[s.name] ?? []).filter((n) => !n.met && !n.overridden);
  if (!missing.length) return;
  const labels = missing.map((n) => n.label);
  const list = labels.length > 1 ? `${labels.slice(0, -1).join(", ")} and ${labels.at(-1)}` : labels[0];
  const dd = el("dd", "muted", `This project asks for ${list}`);
  const fail = el("span", "muted", needExtErrors.has(s.name) ? ` ${needExtErrors.get(s.name)}` : "");
  for (const n of missing) {
    const runtime = n.kind === "php" || n.kind === "node";
    const key = `${n.kind}:${n.install}`;
    const b = el("button", "inline-action", runtime ? `Install ${n.label}` : `Add ${n.label}`);
    if (installing.has(key)) {
      b.disabled = true;
      b.textContent = `Installing ${n.label}…`;
    }
    if (n.kind === "ext") {
      const building = `${s.name}@${n.install}`;
      if (appExtBuilding.has(building)) {
        b.disabled = true;
        b.textContent = "Building…";
      }
      b.onclick = async () => {
        if (appExtBuilding.has(building)) return;
        appExtBuilding.add(building);
        needExtErrors.delete(s.name);
        render();
        try {
          await askSnap("app_ext", { site: s.name, name: n.install, on: true });
        } catch (e) {
          needExtErrors.set(s.name, String(e));
        }
        appExtBuilding.delete(building);
        render();
      };
      dd.append(" ", b);
      continue;
    }
    b.onclick = async () => {
      if (!runtime) return late.openAddService({ kind: n.kind, version: n.install });
      installing.add(key);
      b.disabled = true;
      b.textContent = `Installing ${n.label}…`;
      fail.textContent = "";
      try {
        if (n.kind === "php") await invoke("php_install", { version: n.install });
        else await invoke("runtime_install", { id: "node", version: n.install });
      } catch (e) {
        fail.textContent = ` ${String(e)}`;
        b.disabled = false;
        b.textContent = `Install ${n.label}`;
      } finally {
        installing.delete(key);
      }
      call("state");
    };
    dd.append(" ", b);
  }
  dd.append(fail);
  const row = el("div", "row");
  row.append(el("dt", null, "Project"), dd);
  const runsOn = $('[data-field="runtime"]', root)?.closest(".row");
  if (runsOn) runsOn.after(row);
  else $('[data-panel="overview"] .group', root).prepend(row);
}

const prepareErrors = new Map<string, string>(); // a failed Prepare database, across re-renders
const preparing = new Set<string>();
const rewriteErrors = new Map<string, string>(); // a failed Rewrite with the hooks

/** A worktree's Overview: what it's a worktree of, its database and which it is, and one
 * line for each thing it shares with or lacks from its app. */
function worktreeLines(s, root) {
  const v = s.worktree_of ? snap.worktrees?.[s.name] : null;
  if (!v) return;
  const main = s.worktree_of;
  const group = $('[data-panel="overview"] .group', root);
  group.before(el("p", "worktree-of muted", `Worktree of ${main} · ${s.branch ? `branch ${s.branch}` : "detached"}`));
  const rows: HTMLElement[] = [];
  const add = (dt: string, dd: HTMLElement) => {
    const r = el("div", "row");
    r.append(el("dt", null, dt), dd);
    rows.push(r);
  };
  if (v.database) {
    const how = !v.choices.includes(v.db)
      ? "on a server Anvil doesn't manage"
      : v.adopted
        ? "existing"
        : v.db === "copy"
          ? `copy of ${v.main_database ?? main}`
          : "empty";
    const dd = el("dd", null, `${v.database} (${how})`);
    if (v.preparing) dd.append(el("div", "muted", v.preparing));
    else if (v.pending) {
      const b = el("button", "inline-action", preparing.has(s.name) ? "Preparing…" : "Prepare database");
      b.disabled = preparing.has(s.name);
      b.onclick = async () => {
        preparing.add(s.name);
        prepareErrors.delete(s.name);
        b.disabled = true;
        b.textContent = "Preparing…";
        try {
          await invoke("worktree_prepare", { site: s.name });
        } catch (e) {
          prepareErrors.set(s.name, String(e));
        } finally {
          preparing.delete(s.name);
        }
        call("state");
      };
      dd.append(" ", b);
    }
    const fails = [v.error, prepareErrors.get(s.name), ...startErrors(s.name)];
    for (const fail of new Set(fails)) {
      if (!fail) continue;
      const line = el("div", "muted", fail);
      // A database that is there already but isn't Anvil's: used only when asked.
      const db = v.pending && /(\S+) already exists and Anvil didn't make it/.exec(fail)?.[1];
      if (db) {
        const use = el("button", "inline-action", `Use existing ${db}`);
        use.onclick = () =>
          late.confirmPop(use, `Use ${db} for this worktree? Its migrations and tests then change it.`, "Use", async () => {
            prepareErrors.delete(s.name);
            await call("worktree_adopt", { site: s.name });
          });
        line.append(" ", use);
      }
      dd.append(line);
    }
    add("Database", dd);
  }
  // The hooks' check: keys its env files still give main's value (bun or an IDE read them).
  if (v.same.length) {
    const dd = el("dd", "muted", `${v.same.join(", ")} ${v.same.length === 1 ? "has" : "have"} ${main}'s value in its env files `);
    const b = el("button", "inline-action", "Rewrite with the hooks");
    b.onclick = () =>
      late.confirmPop(b, "Write the hooks' keys into its env files? Only those lines change.", "Rewrite", async () => {
        rewriteErrors.delete(s.name);
        try {
          await invoke("worktree_rewrite", { site: s.name });
        } catch (e) {
          rewriteErrors.set(s.name, String(e));
        }
        call("state");
      });
    dd.append(b);
    const fail = rewriteErrors.get(s.name);
    if (fail) dd.append(el("div", "muted", fail));
    add("Env", dd);
  }
  if (afterRunning.has(s.name)) add("After create", el("dd", "muted", "Running its after-create commands…"));
  else if (afterErrors.has(s.name)) add("After create", el("dd", "muted", afterErrors.get(s.name)));
  setupLines(s, v, main, add);
  add("Shared", el("dd", "muted", `S3 buckets and search indexes are shared with ${main}`));
  if (v.compose) add("Docker", el("dd", "muted", "Anvil doesn't start this worktree's Docker services"));
  if (v.cached_config) {
    add("Config", el("dd", "muted", "Cached config ignores the worktree's database; run php artisan config:clear"));
  }
  group.prepend(...rows);
}

/** The errors of the worktree's last Start, by process. */
const startErrors = (name: string) =>
  Object.entries(snap.app_errors ?? {})
    .filter(([k]) => k.split(/[@#]/)[0] === name)
    .map(([, e]) => e);

const setupErrors = new Map<string, string>(); // a failed copy or install, across re-renders
const wtInstalling = new Set<string>(); // "<site> <id>" while it runs

/** What a worktree opened outside Anvil lacks: the files its hooks copy, its packages. Installs are confirmed first. */
function setupLines(s, v, main, add) {
  const m = v.missing;
  if (!m || (!m.copy.length && !m.installs.length)) return;
  const dd = el("dd");
  if (m.copy.length) {
    const b = el("button", "inline-action", `Copy ${m.copy.join(", ")} from ${main}`);
    b.onclick = async () => {
      setupErrors.delete(s.name);
      try {
        await invoke("worktree_copy_files", { site: s.name });
      } catch (e) {
        setupErrors.set(s.name, String(e));
      }
      call("state");
    };
    dd.append(el("span", "muted", `No ${m.copy.join(", ")} `), b);
  }
  for (const [id, command] of m.installs) {
    const key = `${s.name} ${id}`;
    const b = el("button", "inline-action", wtInstalling.has(key) ? "Installing…" : command);
    b.disabled = wtInstalling.has(key);
    b.onclick = () =>
      late.confirmPop(b, `Run ${command} in ${s.name}?`, "Install", async () => {
        wtInstalling.add(key);
        setupErrors.delete(s.name);
        b.disabled = true;
        b.textContent = "Installing…";
        try {
          await invoke("worktree_install", { site: s.name, what: id });
        } catch (e) {
          setupErrors.set(s.name, String(e));
        } finally {
          wtInstalling.delete(key);
        }
        call("state");
      });
    dd.append(" ", b);
  }
  const fail = setupErrors.get(s.name);
  if (fail) dd.append(el("div", "muted", fail));
  add("Setup", dd);
}

const leftSizes = new Map<string, number>(); // "service/database" -> bytes, once read

/** The app's Overview: one quiet line per database left from a removed worktree. */
function leftoverLines(s, root) {
  const group = $('[data-panel="overview"] .group', root);
  for (const l of (snap.leftovers ?? []).filter((x) => x.app === s.name)) {
    const key = `${l.service}/${l.database}`;
    const text = document.createTextNode(`Database left from ${l.worktree}`);
    const sized = () => leftSizes.has(key) && (text.data = `Database left from ${l.worktree} (${bytes(leftSizes.get(key))})`);
    sized();
    if (!leftSizes.has(key)) {
      // Its server may be stopped: then the line goes without a size.
      invoke("service_contents", { id: l.service })
        .then((c) => {
          const found = c?.databases?.find(([n]) => n === l.database);
          if (found) leftSizes.set(key, found[1]);
          sized();
        })
        .catch(() => {});
    }
    const del = el("button", "inline-action", "Delete");
    del.onclick = () =>
      late.confirmPop(del, `Delete ${l.database}? It can't be undone.`, "Delete", () =>
        call("leftover_drop", { service: l.service, database: l.database }),
      );
    const keep = el("button", "inline-action", "Keep");
    keep.title = "Keep the database; Anvil forgets it was this worktree's";
    keep.onclick = () => call("leftover_keep", { app: l.app, worktree: l.worktree });
    const dd = el("dd", "leftover muted");
    dd.append(text, " ", del, keep);
    const r = el("div", "row");
    r.append(el("dt", null, "Left over"), dd);
    group.append(r);
  }
}

function djangoHint(root) {
  // Only in debug: in production another proxy decides what these headers may say.
  const lines = `if DEBUG:  # Anvil\n    ALLOWED_HOSTS += [".${snap.tld}", "localhost", "127.0.0.1"]\n    SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")`;
  const box = el("div", "django-hint");
  box.append(
    el("h3", "res-sub", "Django settings"),
    el(
      "p",
      "muted",
      "Add to the end of settings.py so Django accepts the address and trusts HTTPS from Anvil; without it Django answers 400 Bad Request, and forms fail their CSRF check.",
    ),
  );
  const env = el("div", "env");
  const copyBtn = el("button", null, "Copy");
  copyBtn.onclick = () => late.copy(lines, copyBtn);
  env.append(el("pre", null, lines), copyBtn);
  box.append(env);
  $('[data-panel="overview"] .group', root).after(box);
}

/** Next.js only works at .localhost until its config allows the domain suffix. */
function nextHint(s, root) {
  const box = el("div", "next-hint");
  const use = el("button", null, `Use ${s.name}.${snap.tld}`);
  use.onclick = () => call("allow_next_origin", { site: s.name });
  box.append(
    el("h3", "res-sub", "Address"),
    el(
      "p",
      "muted",
      `Next.js only becomes interactive at .${snap.tld} once next.config allows it, so this app opens at ${s.host}. Anvil can add allowedDevOrigins to next.config for you.`,
    ),
    use,
  );
  $('[data-panel="overview"] .group', root).after(box);
}

/* ---------- Resources ---------- */

export const resCache = new Map<string, any>(); // site name -> resources
const SAFETY = {
  safe: ["Safe to delete", "Nothing is lost"],
  rebuild: ["Rebuildable", "Delete to free space; it can be recreated"],
  keep: ["Keep", "Your code or data"],
};

export function bytes(n) {
  if (n == null) return "–";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n >= 10 || i === 0 ? Math.round(n) : n.toFixed(1)} ${u[i]}`;
}

async function loadResources(name, force = false) {
  if (!force && resCache.has(name)) return renderResources(name);
  if (force) resCache.delete(name);
  if (selected === name) renderResources(name);
  try {
    resCache.set(name, await invoke("resources", { site: name }));
  } catch (e) {
    resCache.set(name, { error: String(e) });
  }
  if (selected === name) renderResources(name);
}

export function el(tag: string, cls?: string | null, text?: string | number | null): any {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = String(text);
  return e;
}

/** A segmented control: one pressed item among a few, e.g. All · Errors · Warnings. */
export function seg(label: string, items: [string, string][], current: string, pick: (id: string) => void) {
  const box = el("div", "seg");
  box.setAttribute("role", "group");
  box.setAttribute("aria-label", label);
  for (const [id, text] of items) {
    const b = el("button", null, text);
    b.dataset.id = id;
    b.setAttribute("aria-pressed", String(id === current));
    b.onclick = () => {
      for (const o of box.children) o.setAttribute("aria-pressed", String(o === b));
      pick(id);
    };
    box.append(b);
  }
  return box;
}

function renderResources(name) {
  const body = $("#site .res-body");
  if (!body) return;
  const r = resCache.get(name);
  body.replaceChildren();
  if (!r) return body.append(el("p", "muted", "Measuring…"));
  if (r.error) return body.append(el("p", "error", r.error));

  const sumOf = (kind) => r.items.filter((i) => i.safety === kind).reduce((a, i) => a + i.bytes, 0);
  const safe = sumOf("safe");

  // Total on the left, the one-click clean-up on the right.
  const head = el("div", "res-head");
  head.append(el("strong", "res-total", bytes(r.total)));
  const freed = freedNote.get(name);
  if (freed) head.append(el("span", "muted res-freed", freed));
  const safeItems = r.items.filter((i) => i.safety === "safe" && i.bytes > 0);
  if (safeItems.length) {
    const all = el("button", "clean-all", `Clean up ${bytes(safe)}`);
    all.title = `Deletes ${safeItems.map((i) => i.label.toLowerCase()).join(", ")}`;
    all.onclick = () => cleanItems(name, safeItems);
    head.append(all);
  }
  body.append(head);

  // One bar split by what can be deleted, with a short legend instead of prose.
  const bar = el("div", "res-bar");
  bar.setAttribute("aria-hidden", "true");
  const legend = el("div", "res-legend");
  for (const kind of ["keep", "rebuild", "safe"]) {
    const sum = sumOf(kind);
    if (!sum || !r.total) continue;
    const seg = el("span", `seg ${kind}`);
    seg.style.flexGrow = sum;
    bar.append(seg);
    const key = el("span", "key");
    key.append(el("i", `swatch ${kind}`), document.createTextNode(`${SAFETY[kind][0]} ${bytes(sum)}`));
    key.title = SAFETY[kind][1];
    legend.append(key);
  }
  body.append(bar, legend);

  const list = el("dl", "group res-list");
  for (const i of r.items.filter((i) => i.bytes > 0)) {
    const row = el("div", "row");
    const dt = el("dt");
    // A swatch says keep, rebuild or safe, in the legend's colours; the words are in its tooltip.
    const label = el("span");
    const sw = el("i", `swatch ${i.safety}`);
    sw.setAttribute("role", "img");
    sw.setAttribute("aria-label", String(SAFETY[i.safety][0]));
    // The whole name carries the explanation: a bigger target than the swatch.
    label.title = `${SAFETY[i.safety][0]}: ${i.note}`;
    label.append(sw, i.label);
    dt.append(label);
    if (i.path) dt.append(el("small", "path", i.path.startsWith("/") ? tilde(i.path) : i.path));
    const dd = el("dd", "res-cells");
    dd.append(el("span", "size", bytes(i.bytes)));
    if (i.safety !== "keep") {
      const del = el("button", "del", "Delete");
      del.setAttribute("aria-label", String(`Delete ${i.label}`));
      del.onclick = () => cleanItems(name, [i]);
      dd.append(del);
    } else {
      dd.append(el("span", "del-space")); // keeps the size column aligned
    }
    row.append(dt, dd);
    list.append(row);
  }
  body.append(list);

  if (r.processes.length) {
    body.append(el("h3", "res-sub", "Running"));
    const procs = el("dl", "group res-list");
    for (const p of r.processes) {
      const row = el("div", "row");
      const dd = el("dd", "res-cells");
      dd.append(el("span", "muted", `${p.cpu.toFixed(1)}% CPU`), el("span", "size", bytes(p.memory)));
      row.append(el("dt", null, p.label), dd);
      procs.append(row);
    }
    body.append(procs);
  }

  body.append(el("h3", "res-sub", "Services"));
  if (!r.services.length) {
    body.append(el("p", "muted", "None found in .env"));
  } else {
    const svcs = el("dl", "group res-list");
    for (const sv of r.services) {
      const row = el("div", "row");
      const dt = el("dt");
      dt.append(el("span", null, sv.label));
      const on = sv.port ? ` on port ${sv.port}` : "";
      const where = {
        anvil: `Anvil service ${sv.detail}, ${sv.running ? "running" : "stopped"}${on}, shared by apps using it`,
        docker: sv.detail?.startsWith("compose service ")
          ? `Docker Compose service ${sv.detail.slice(16)}`
          : `Docker ${sv.detail ? `container ${sv.detail}` : "container"}${on}`,
        local: `${sv.detail}${on} (not Anvil)`,
        remote: `Remote: ${sv.detail}`,
        none: sv.port ? `Nothing is listening on port ${sv.port}` : "",
      }[sv.source];
      if (where) dt.append(el("small", "path", where));
      if (sv.anvil) {
        const go = el("button", "link", "Show service");
        go.onclick = () => {
          setMode("services");
          late.selectService(sv.anvil);
        };
        dt.append(go);
      }
      const dd = el("dd", "res-cells");
      if (sv.memory != null) dd.append(el("span", "muted", `${bytes(sv.memory)} memory`));
      if (sv.data_bytes != null) dd.append(el("span", "size", `${bytes(sv.data_bytes)} data`));
      row.append(dt, dd);
      svcs.append(row);
    }
    body.append(svcs);
  }
}

const freedNote = new Map<string, string>(); // site -> what the last clean-up freed

/** Confirm with a native dialog, then delete the items one by one. */
async function cleanItems(name, items) {
  const total = items.reduce((a, i) => a + i.bytes, 0);
  const what =
    items.length === 1
      ? `${items[0].label} (${items[0].path.startsWith("/") ? tilde(items[0].path) : items[0].path})`
      : `${items.length} items`;
  const how = items.length === 1 ? items[0].note : items.map((i) => `• ${i.label}: ${i.note}`).join("\n");
  const ok = await window.__TAURI__.dialog.ask(`${how}\n\nThis frees about ${bytes(total)} and can't be undone.`, {
    title: `Delete ${what}?`,
    kind: "warning",
    okLabel: "Delete",
    cancelLabel: "Cancel",
  });
  if (!ok) return;
  let freed = 0;
  for (const i of items) {
    try {
      freed += await invoke("clean", { site: name, path: i.path });
    } catch (e) {
      showError(String(e));
      break;
    }
  }
  // Said next to Clean up, not in a toast; it stays until the next clean-up or reload.
  if (freed) freedNote.set(name, `Freed ${bytes(freed)}`);
  loadResources(name, true);
}

export const PHP_KINDS = new Set(["laravel", "statamic", "symfony", "php"]);
export const PROCESS_KINDS = new Set(["next", "nuxt", "vite", "node", "rust", "go", "dotnet", "django", "fastapi", "flask"]);

function renderRun(s, root) {
  renderRunState(s, root);
  renderRunSettings(s, root);
}

/** The run error: safe to redraw while the user types elsewhere. The run box is
 * the Overview's (processes.js), for every process of the app. */
function renderRunState(s, root) {
  const error = snap.app_errors[s.name];
  const err = $(".run-error", root);
  err.textContent = error ?? "";
  err.hidden = !error;
  if (error && $('.tabs [data-tab="logs"]', root)) {
    const logs = el("button", "inline-action", "Show Logs");
    logs.onclick = () => showTab(root, "logs");
    err.append(" ", logs);
  }
}

function renderRunSettings(s, root) {
  // Language version: automatic (project file or newest) or pinned.
  const rtId = {
    next: "node",
    nuxt: "node",
    vite: "node",
    node: "node",
    rust: "rust",
    go: "go",
    dotnet: "dotnet",
    django: "python",
    fastapi: "python",
    flask: "python",
  }[s.kind];
  const versions = snap.runtimes?.[rtId] ?? [];
  const rt = $("#app-runtime", root);
  $("#app-runtime-label", root).textContent = `${RUNTIME[s.kind]} version`;
  const autoVersion = s.runtime_pinned ? versions[0] : s.runtime_version;
  rt.add(new Option(autoVersion ? `Automatic (${autoVersion})` : "Automatic", ""));
  for (const v of versions) rt.add(new Option(v, v));
  rt.value = s.runtime_pinned ? s.runtime_version : "";
  const pinFile = { node: ".anvil.toml, .nvmrc or .node-version", python: ".python-version" }[rtId];
  rt.title = pinFile
    ? `Automatic uses ${pinFile} when the project has one, otherwise the newest installed`
    : "Automatic uses the newest installed";
  rt.disabled = versions.length < 2;
  rt.onchange = () => call("set_runtime", { site: s.name, version: rt.value || null });
  // dotnet picks the SDK from the project's global.json: nothing to pin.
  if (rtId === "dotnet") rt.closest(".row").remove();

  const cmd = $("#app-command", root);
  cmd.value = s.command_custom ? s.command : "";
  cmd.placeholder = s.command_custom ? "" : (s.command ?? "");
  cmd.onchange = () =>
    call("set_app", cmd.value.trim() ? { site: s.name, command: cmd.value.trim() } : { site: s.name, reset: ["command"] });

  const port = $("#app-port", root);
  port.value = s.port ?? "";
  port.onchange = () => {
    const n = parseInt(port.value, 10);
    call("set_app", Number.isNaN(n) ? { site: s.name, reset: ["port"] } : { site: s.name, port: n });
  };

  const auto = $("#app-autostart", root);
  auto.checked = s.autostart;
  auto.onchange = () => call("set_app", { site: s.name, autostart: auto.checked });
}

// While an app is starting, check back until its port answers. Only the list dots
// and the run box change, so nothing the user is typing gets redrawn.
let starting = false; // that check on its way
setInterval(async () => {
  if (starting || !shown() || mode !== "apps" || !snap || !Object.values(snap.app_states).includes("starting")) return;
  starting = true;
  try {
    if (!(await askSnap("state"))) return;
  } catch {
    return;
  } finally {
    starting = false;
  }
  if (mode !== "apps") return;
  renderList();
  const s = snap.sites.find((x) => x.name === selected);
  const root = $("#site");
  if (s && PROCESS_KINDS.has(s.kind) && $(".run-box", root)) renderRunState(s, root);
}, 1500);

export function select(name) {
  selected = name;
  localStorage.setItem("selected", name);
  renderList();
  renderSite();
  $(`.item[data-name="${CSS.escape(name)}"]`)?.focus({ preventScroll: false });
}

/** Open the selected app (or `site`) in the browser, Finder, a terminal or the editor. */
export async function open(target, site = selected) {
  try {
    await invoke("open", { site, target });
  } catch (e) {
    showError(String(e));
  }
}

/** Applications served, each folder once: an alias shares its app's folder. */
function servedCount() {
  // Worktrees count as apps; aliases share their app's folder.
  const parked = new Set(snap.sites.filter((s) => !s.linked).map((s) => s.path));
  return snap.sites.filter((s) => !s.linked || !parked.has(s.path)).length;
}

// Stopped: one click starts. Serving: a menu with what is served, Restart and Stop.
$("#power").onclick = async (e) => {
  const btn = e.currentTarget;
  const run = async (on) => {
    btn.disabled = true;
    await call("set_running", { on });
    btn.disabled = false;
  };
  if (!snap.running) return run(true);
  const n = servedCount();
  const menu = await M.Menu.new({
    items: await Promise.all([
      M.MenuItem.new({ text: `Serving ${n} ${n === 1 ? "application" : "applications"}`, enabled: false }),
      M.PredefinedMenuItem.new({ item: "Separator" }),
      M.MenuItem.new({ text: "Restart", action: () => run(true) }),
      M.MenuItem.new({ text: "Stop", action: () => run(false) }),
    ]),
  });
  const r = btn.getBoundingClientRect();
  const dpi = (window.__TAURI__ as any).dpi;
  menu.popup(dpi ? new dpi.LogicalPosition(r.left, r.bottom + 4) : undefined);
};

$("#open-settings").onclick = () => invoke("show_settings");
$("#open-processes").onclick = () => invoke("show_processes");

/* ---------- New application ---------- */

// The sheet itself is newapp.ts.

/** Serve one existing project folder. Resolves to whether one was added. */
export async function addAppFolder() {
  const path = await window.__TAURI__.dialog.open({ directory: true, title: "Choose a project folder" });
  if (!path) return false;
  try {
    const name = await invoke("link_folder", { path });
    await call("state");
    select(name);
    return true;
  } catch (e) {
    showError(String(e));
    return false;
  }
}

$("#add").onclick = async () => {
  if (mode === "services") return late.openAddService();
  const pick = (title) => window.__TAURI__.dialog.open({ directory: true, title });
  const menu = await M.Menu.new({
    items: await Promise.all([
      M.MenuItem.new({ text: "New Application…", accelerator: "CmdOrCtrl+N", action: () => late.openNewApp() }),
      M.PredefinedMenuItem.new({ item: "Separator" }),
      M.MenuItem.new({
        text: "Add Folder of Applications…",
        action: async () => {
          const path = await pick("Every project inside this folder becomes an application");
          if (!path) return;
          await call("park_folder", { path });
        },
      }),
      M.MenuItem.new({ text: "Add Application Folder…", action: addAppFolder }),
    ]),
  });
  menu.popup();
};

/** Filter menu: the kinds present in the list, with counts, plus running only. */
$("#filter").onclick = async () => {
  const apps = mode === "apps";
  const f = filterOf(mode);
  const items = apps ? snap.sites.filter((s) => !s.linked) : late.services();
  const counts = new Map();
  for (const x of items) counts.set(x.kind, (counts.get(x.kind) ?? 0) + 1);
  const label = (k) => (apps ? kindLabel(k) : late.services().find((x) => x.kind === k).name);
  const set = (change) => {
    Object.assign(f, change);
    localStorage.setItem("filters", JSON.stringify(filters));
    renderFilter();
    renderMain();
  };
  const sep = () => M.PredefinedMenuItem.new({ item: "Separator" });
  const kinds = [...counts.keys()].sort((a, b) => label(a).localeCompare(label(b)));
  // Applications: how the list is grouped and sorted comes first, then what it shows.
  const pick = (key, current, options) =>
    Promise.all(
      options.map(([value, text]) => M.CheckMenuItem.new({ text, checked: current === value, action: () => setView(key, value) })),
    );
  const view = apps
    ? [
        M.Submenu.new({
          text: "Group By",
          items: await pick("groupBy", groupBy, [
            ["folder", "Folder"],
            ["type", "Type"],
            ["none", "None"],
          ]),
        }),
        M.Submenu.new({
          text: "Sort By",
          items: await pick("sortBy", sortBy, [
            ["name", "Name"],
            ["modified", "Date Modified"],
          ]),
        }),
        M.CheckMenuItem.new({ text: "Show Hidden Applications", checked: showHidden, action: () => setShowHidden(!showHidden) }),
        sep(),
      ]
    : [];
  const menu = await M.Menu.new({
    items: await Promise.all([
      ...view,
      M.CheckMenuItem.new({ text: "All Types", checked: !f.kind, action: () => set({ kind: null }) }),
      sep(),
      ...kinds.map((k) =>
        M.CheckMenuItem.new({ text: `${label(k)} (${counts.get(k)})`, checked: f.kind === k, action: () => set({ kind: k }) }),
      ),
      sep(),
      M.CheckMenuItem.new({ text: "Running Only", checked: f.running, action: () => set({ running: !f.running }) }),
    ]),
  });
  menu.popup();
};

/** The filter button lights up while a filter hides something. */
function renderFilter() {
  const f = filterOf(mode);
  const on = !!f.kind || f.running;
  const btn = $("#filter");
  btn.classList.toggle("active", on);
  const what = mode === "apps" ? "applications" : "services";
  btn.title = on
    ? `Filtered: ${[f.kind && "one type", f.running && "running only"].filter(Boolean).join(", ")}`
    : mode === "apps"
      ? "Group, sort and filter applications"
      : `Filter ${what}`;
  btn.setAttribute("aria-label", String(btn.title));
}

$("#reload").onclick = async (e) => {
  const btn = e.currentTarget;
  btn.classList.add("spinning");
  icons.clear();
  await call("refresh");
  btn.classList.remove("spinning");
};

// No browser context menu (Reload, Inspect…) outside text fields; sites have their own.
document.addEventListener("contextmenu", (e) => {
  if (!(e.target as HTMLElement).closest("input, textarea")) e.preventDefault();
});

$("#search").oninput = (e) => {
  filter = e.target.value.trim();
  if (mode === "services") late.renderServiceList();
  else renderList();
};

// Arrow keys move through the list like a native outline: up/down to move, left/right to fold.
// The hover card goes when the list (or what holds it) scrolls; logs scrolling elsewhere don't count.
document.addEventListener("scroll", (e) => (e.target as HTMLElement).contains?.($("#list")) && hideCard(), true);
$("#list").onkeydown = (e) => {
  if (mode === "services") return late.serviceKeys(e);
  const cur = $(`.item[data-name="${CSS.escape(selected ?? "")}"]`);
  if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && cur?.hasAttribute("aria-expanded")) {
    e.preventDefault();
    toggle(selected, e.key === "ArrowRight");
    return;
  }
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const items = [...document.querySelectorAll<HTMLElement>(".item")].filter((b) => !b.closest(".closed"));
  const i = items.findIndex((b) => b.dataset.name === selected);
  const next = items[i + (e.key === "ArrowDown" ? 1 : -1)];
  if (next) select(next.dataset.name);
};

$("#error-close").onclick = () => {
  errors.clear();
  showError(null);
};

// Keyboard: ⌘1–9 tabs in order, ⌘O browser, ⌘E editor, ⌘T terminal, ⌘G git app, ⌘L log.
/* ---------- Shortcuts: the menu bar and the keyboard run the same actions ---------- */

const TAB_KEYS = { 1: "overview", 2: "git", 3: "dev", 4: "logs", 5: "packages", 6: "resources", 7: "settings" };
let lastAction = { name: "", at: 0 };

/** Run a menu action. The same key can reach both the page and the menu bar, so
 * a repeat within 300 ms is ignored. Returns whether it did something. */
function runAction(name) {
  if (lastAction.name === name && Date.now() - lastAction.at < 300) return true;
  const root = $("#site");
  const [kind, arg] = name.split(":");
  let done = true;
  if (kind === "tab") {
    // ⌘3 is Dev, or Containers for an app without Dev.
    const visible = (t) => $(`.tabs [data-tab="${t}"]`, root)?.hidden === false;
    const to = arg === "dev" && !visible("dev") ? "containers" : arg;
    const t = $(`.tabs [data-tab="${to}"]`, root);
    if (t && !t.hidden) showTab(root, to);
    else done = false;
  } else if (kind === "open") {
    const off = { editor: !snap.editor, git: $(".git-btn", root)?.hidden !== false, browser: !snap.running }[arg];
    if (mode === "apps" && selected && !off) open(arg);
    else done = false;
  } else if (kind === "mode") {
    setMode(arg);
  } else if (name === "logs") {
    const svcLog = $(".show-log", root);
    if ($('.tabs [data-tab="logs"]', root)) showTab(root, "logs");
    else if (svcLog) svcLog.click();
    else done = false;
  } else if (name === "new") {
    if (mode === "services") late.openAddService();
    else late.openNewApp();
  } else if (name === "park") {
    window.__TAURI__.dialog
      .open({ directory: true, title: "Every project inside this folder becomes an application" })
      .then((path) => path && call("park_folder", { path }));
  } else if (name === "reload") {
    $("#reload").click();
  } else if (name === "find") {
    $("#search").focus();
  } else if (name === "settings") {
    invoke("show_settings");
  } else {
    done = false;
  }
  if (done) lastAction = { name, at: Date.now() };
  return done;
}

window.__TAURI__.event?.listen("menu", (e) => runAction(e.payload));

document.addEventListener("keydown", (e) => {
  if (!e.metaKey || e.shiftKey || e.altKey) return;
  // In text fields only the app-wide keys apply; the rest belong to editing.
  const typing = (e.target as HTMLElement).closest("input, textarea, select");
  const name = TAB_KEYS[e.key]
    ? `tab:${TAB_KEYS[e.key]}`
    : {
        o: "open:browser",
        e: "open:editor",
        t: "open:terminal",
        g: "open:git",
        l: "logs",
        n: "new",
        r: "reload",
        f: "find",
        ",": "settings",
      }[e.key];
  if (!name || (typing && !["find", "reload", "settings", "new"].includes(name))) return;
  if (runAction(name)) e.preventDefault();
});

window.refresh();

export function setMode(m) {
  mode = m;
  localStorage.setItem("mode", m);
  filter = "";
  $("#search").value = "";
  late.renderModes();
  renderMain();
}
