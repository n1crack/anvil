import { M } from "./menu";
import type { Snapshot } from "./types";
import { late } from "./late";
import { ownError, shown } from "./live";
import { NAMES, editor, summary } from "./hooksform";
import { bubbleKeys, confirmInRow } from "./confirm";
import { icon } from "./icons";
import { fileManager, keys, mac, mod } from "./platform";

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
// Worktrees being removed: their row says so until they're gone.
const removing = new Set<string>();

// Compose projects and plain folders get no address from Anvil.
export const serves = (s) => !["compose", "folder"].includes(s.kind);

export const url = (s) => {
  const [scheme, port, std] = s.secure ? ["https", snap.https_port, 443] : ["http", snap.http_port, 80];
  return `${scheme}://${s.host}${port === std ? "" : `:${port}`}`;
};

const BRANCH = `<svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="4.5" cy="3.5" r="1.5"/><circle cx="4.5" cy="12.5" r="1.5"/><circle cx="11.5" cy="5.5" r="1.5"/><path d="M4.5 5v6M11.5 7c0 2.5-3.5 2-7 4"/></svg>`;

const LOCK = `<svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><rect x="3" y="7" width="10" height="7" rx="1.5" fill="currentColor"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>`;

const OPEN_LOCK = `<svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><rect x="3" y="7" width="10" height="7" rx="1.5" fill="currentColor"/><path d="M5.5 7V5a2.5 2.5 0 0 1 4.9-.7" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>`;
/** Before an address: a green closed lock for HTTPS, a red open one for plain HTTP. */
function lockOf(secure: boolean) {
  const lock = el("span", `lock ${secure ? "on" : "off"}`);
  lock.innerHTML = secure ? LOCK : OPEN_LOCK;
  lock.title = secure ? "HTTPS" : "HTTP, not encrypted";
  return lock;
}

const CHEVRON = `<svg class="chevron" viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 2.5 8 6l-3.5 3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
const FOLDER_ICON = `<svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><path d="M1.75 4.25a1 1 0 0 1 1-1h3.5l1.5 1.5h5.5a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1H2.75a1 1 0 0 1-1-1z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>`;
const STACK_ICON = `<svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"><path d="M8 1.75 14.25 5 8 8.25 1.75 5z"/><path d="m1.75 8 6.25 3.25L14.25 8M1.75 11 8 14.25 14.25 11"/></svg>`;
/** In a lock's place for what Anvil doesn't serve: a grey folder, or a stack for Compose. */
function unservedOf(s) {
  const icon = el("span", "lock none");
  icon.innerHTML = s.kind === "compose" ? STACK_ICON : FOLDER_ICON;
  icon.title = s.kind === "compose" ? "Docker Compose: no address of Anvil's" : "A folder: not served";
  return icon;
}

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

// An app's last failed action, by app: said in its header, under its name.
export const actionErrors = new Map<string, string>();

export async function call(cmd: string, args?: Record<string, unknown>) {
  const site = typeof args?.site === "string" ? args.site : null;
  try {
    if (!(await askSnap(cmd, args))) return;
    showError(null, cmd);
    if (site) actionErrors.delete(site);
    render();
  } catch (e) {
    // About the shown app: in its header; else in the banner.
    if (site && site === selected && mode === "apps") {
      actionErrors.set(site, String(e));
      render();
    } else showError(String(e), cmd);
  }
}

const errors = new Map<string, string>(); // owner -> its error, see ownError
/** The error banner, over the top of the pane, for an error with no place of its own:
 * `owner`'s error, or null to clear only that one. Without an owner it stays until
 * dismissed. */
export function showError(msg, owner = "") {
  const text = ownError(errors, msg, owner);
  $("#error-text").textContent = text;
  $("#error").hidden = !text;
}

/** An action's error right after `at`, the button or line it was taken from; null clears
 * it. The banner only when `at` is gone. */
export function inlineError(at: Element | null, msg: string | null) {
  if (at?.nextElementSibling?.classList.contains("inline-error")) at.nextElementSibling.remove();
  if (!msg) return;
  if (at?.isConnected) at.after(el("span", "error inline-error", msg));
  else showError(msg);
}

window.refresh = () => {
  // A command's own answer already carries the new state; the "changed" signal
  // that follows it doesn't need a second read.
  // ponytail: a Settings change landing within 0.5 s of a click here waits for the next refresh.
  const asked = Date.now() - snapAt > 500 ? call("state") : null;
  window.loadServices?.();
  loadSetup();
  return asked;
};

/* ---------- Setup: what still stands between this Mac and working addresses ---------- */

// What setting up this Mac means, beyond PHP: local domains, ports 80/443, the HTTPS
// certificate and the terminal commands. One step does them all, one password.
const SETUP_FIELDS = ["resolver", "ports", "trusted", "shims"];

// The bubble closes on the mousedown before the button's click: that click must not
// open it again. Window capture runs before the bubble's own document listener.
let setupWasOpen = false;
window.addEventListener(
  "mousedown",
  (e) => (setupWasOpen = !!$("#setup")?.contains(e.target) && !!document.querySelector(".setup-pop")),
  true,
);

/** One titlebar button for every step still missing; it opens a bubble with each step, what
 * it does and its own button, so nothing runs (or asks for a password) on that first click. */
async function loadSetup(busy?: string) {
  let st;
  try {
    st = await invoke("setup_status");
  } catch {
    return;
  }
  const bar = $("#setup");
  const machine = SETUP_FIELDS.some((f) => !st[f]);
  // [step, button, while running, what it does]
  const steps: [string, string, string, string][] = [];
  if (machine)
    steps.push([
      "all",
      "Set up Anvil",
      "Setting up…",
      "Local .test domains, ports 80 and 443, the HTTPS certificate, and php, composer, laravel and node in your terminal. Asks for your administrator password once.",
    ]);
  if (!st.php) steps.push(["php", "Install PHP", "Installing PHP…", "PHP 8.4, for PHP applications. No password needed."]);
  // Only when an app needs it: a Node.js app, or a package.json (a Laravel app's Vite).
  if (st.node === false)
    steps.push([
      "node",
      "Install Node.js",
      "Installing Node.js…",
      "The newest long-term support Node.js, for your apps' npm, Vite and Node.js servers. No password needed.",
    ]);
  // `git` is false only where Anvil can install it (Windows).
  if (st.git === false)
    steps.push([
      "git",
      "Install Git",
      "Installing Git…",
      "Git for Windows, through winget: new apps get a repository, and worktrees work. Windows may ask once.",
    ]);
  bar.hidden = !steps.length;
  const running = steps.find(([step]) => step === busy);
  const label = running ? running[2] : machine ? "Set up Anvil" : steps.length > 1 ? `Finish setup (${steps.length})` : steps[0]?.[1];
  const b = el("button", "setup-go", label);
  if (running) b.prepend(el("span", "spin-dot"));
  b.disabled = !!busy;
  b.setAttribute("aria-haspopup", "dialog");
  b.onclick = () => {
    if (setupWasOpen) return (setupWasOpen = false);
    const pop = el("div", "pop-confirm wide setup-pop");
    for (const [step, name, , what] of steps) {
      const go = el("button", null, name);
      go.onclick = async () => {
        close();
        loadSetup(step);
        try {
          const after = await invoke("setup_fix", { step });
          // macOS may refuse to trust the certificate system-wide without asking;
          // the login keychain then takes it, with its own prompt.
          if (step === "all" && after && !after.trusted) await invoke("setup_fix", { step: "trust" });
        } catch (e) {
          showError(String(e));
        }
        loadSetup();
      };
      pop.append(el("p", null, what), go);
      // Another program holding .test or ports 80/443: said before the password, not blocking.
      if (step === "all") for (const c of st.conflicts ?? []) pop.append(el("p", "setup-conflict", c));
    }
    const close = popAt(b, pop, pop.querySelector("button"), "left");
  };
  bar.replaceChildren(b);
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

// Apps just hidden, by name: for a few seconds their row says so in place, with Undo.
const justHid = new Map<string, any>();

/** A just-hidden app's slim row: "<name> hidden · Undo". */
function hidRow(s) {
  const r = el("div", "hid-row");
  const undo = el("button", "link", "Undo");
  undo.onclick = () => {
    justHid.delete(s.name);
    call("unhide_site", { site: s.name });
  };
  r.dataset.name = s.name;
  r.append(el("span", null, `${s.name} hidden`), el("span", "sep", "·"), undo);
  return r;
}

/** Apps hidden from Anvil, shaped like sites, while "Show Hidden Applications" is on. They aren't served. */
function hiddenSites(): any[] {
  if (!showHidden) return [];
  return (snap.hidden ?? [])
    .filter((h) => !justHid.has(h.name))
    .map((h) => ({
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
  head.insertAdjacentHTML("beforeend", CHEVRON);
  head.append(el("span", null, title), el("span", "section-count", count));
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
export function markBox(m, badge, title?: string) {
  const box = el("span", "mark-box");
  box.append(m);
  setBadge(box, badge, title);
  return box;
}

function setBadge(box, badge, title?: string) {
  $(".run-badge", box)?.remove();
  box.classList.toggle("running", !!badge);
  if (!badge) return;
  const b = el("span", `run-badge ${badge}`);
  b.title = title ?? { on: "Running", warm: "Starting", bad: "Failed to start" }[badge];
  // Said in the row's name too, so the state isn't only a colour.
  b.setAttribute("role", "img");
  b.setAttribute("aria-label", b.title);
  box.append(b);
}

const BADGES = { running: "on", starting: "warm", crashed: "bad" };
const STATE_TITLES = { running: "Running", starting: "Starting", crashed: "A process crashed" };

/** An app row's dot: green while anything of the app runs, amber while its server starts,
 * red after a crash. */
function appBadge(box, state) {
  setBadge(box, state && BADGES[state], state === "crashed" ? STATE_TITLES.crashed : undefined);
}

/** A worktree row's branch glyph, coloured as an app's dot. */
function wtState(m, state) {
  m.className = `wt-glyph ${BADGES[state] ?? ""}`;
  m.title = STATE_TITLES[state] ?? "Stopped";
  m.setAttribute("role", "img");
  m.setAttribute("aria-label", m.title);
}

/** The names linked to one folder: the app is the parked one, else the first by name, as
 * Anvil picks it; the others are its aliases. */
function sameFolder(s) {
  const same = snap.sites.filter((k) => !k.worktree_of && k.path === s.path);
  return { main: same.find((k) => !k.linked) ?? same.sort((a, b) => a.name.localeCompare(b.name))[0], same };
}

/** The app itself, not a worktree nor an alias: only it takes aliases; its worktrees get theirs from it. */
const isMain = (s) => !s.worktree_of && sameFolder(s).main === s;

/** An app's aliases; a worktree's are derived from its app's (`admin--x`). An alias has none. */
function aliasesOf(s) {
  if (s.worktree_of) return s.aliases.map((host) => ({ host, secure: s.secure }));
  const { main, same } = sameFolder(s);
  return main === s ? same.filter((k) => k !== s) : [];
}

export function row(s, { child = false, kids = 0, path = true, kind: showKind = true } = {}) {
  const b = document.createElement("button");
  b.className = child ? "item child" : "item";
  if (s.hiddenApp) b.classList.add("hidden-app");
  b.dataset.name = s.name;
  b.setAttribute("aria-current", String(s.name === selected));
  // A worktree: a bare branch glyph on the tree line, no mark of its own (that's an app's);
  // green while it runs, amber while it starts, red after a crash.
  const childMark = () => {
    const m = el("span");
    m.innerHTML = BRANCH;
    wtState(m, snap.app_states[s.name]);
    return m;
  };
  const text = document.createElement("span");
  text.className = "text";
  text.innerHTML = `<span class="name"><span class="label"></span></span><span class="sub"></span>`;
  // The folder's name: not every project is a site, so no address in the list.
  // A worktree is named by its branch.
  $(".name .label", text).textContent = child || s.worktree_of ? (s.branch ?? s.name) : s.name;
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
  } else $(".name", text).prepend(lockOf(s.secure));
  if (!s.hiddenApp && !serves(s)) $(".name", text).prepend(unservedOf(s));
  // In a folder's section the path only repeats the heading; linked apps and groups keep it.
  if (path && !child) $(".sub", text).textContent = tilde(s.path) + (s.path.includes("\\") ? "\\" : "/");
  else $(".sub", text).remove();
  if (!path && !child) b.classList.add("compact");

  // Kind labels line up flush right: the version comes before the kind; the worktrees'
  // "⑂ 2 ›" goes after the name, not in a column of its own.
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
  else if (removing.has(s.name)) {
    // Its processes stop, its databases may go, then its folder: that takes a moment.
    const busy = el("span", "kind removing", "Removing…");
    busy.prepend(el("span", "spin-dot"));
    end.append(busy);
  } else if (showKind && !child && s.kind !== "static" && !(s.kind === "php" && s.php_pinned)) {
    const kind = document.createElement("span");
    kind.className = `kind kind-${s.kind}`;
    kind.textContent = snap.kinds.find(([id]) => id === s.kind)?.[1] ?? s.kind;
    end.append(kind);
  }
  if (kids) {
    const d = el("span", "disclosure");
    const open = !collapsed.has(s.name);
    b.setAttribute("aria-expanded", String(open));
    d.title = open ? "Hide worktrees" : "Show worktrees";
    d.innerHTML = `${BRANCH}<span class="count"></span>${CHEVRON}`;
    $(".count", d).textContent = kids;
    d.onclick = (e) => {
      e.stopPropagation();
      toggle(s.name);
    };
    $(".name", text).append(d);
  }
  if (child) b.append(childMark(), text, end);
  else {
    const box = markBox(mark(s), null);
    appBadge(box, s.hiddenApp ? undefined : snap.app_states[s.name]);
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
  const add = el("button", "primary", "Add Folder of Applications…");
  add.onclick = addAppsFolder;
  const create = el("button", "link", "or create a new application");
  create.onclick = () => late.openNewApp();
  box.append(
    icon,
    el("h1", null, "Welcome to Anvil"),
    el("p", "muted", `Add the folder where your projects live. Each project in it gets its own .${snap.tld} address.`),
    add,
    create,
  );
  const note = parkedNoteEl();
  if (note) box.append(note);
  root.replaceChildren(box);
}

// What the last Add Folder of Applications… added nothing for: a folder that is one
// project itself (its kind), or one with no applications in it yet (null).
let parked: { path: string; project: string | null } | null = null;

/** Add a folder of applications. One that is a project itself isn't added (its `vendor`,
 * `node_modules`… would become apps): a note offers adding it as one application. */
export async function addAppsFolder() {
  const path = await window.__TAURI__.dialog.open({ directory: true, title: "Every project inside this folder becomes an application" });
  if (!path) return;
  try {
    const r = await invoke("park_folder", { path });
    parked = r.project || !r.apps ? { path, project: r.project } : null;
    await call("state");
  } catch (e) {
    showError(String(e));
  }
}

/** The note for `parked`: on the welcome, or over the list. */
function parkedNoteEl() {
  if (!parked) return null;
  const { path, project } = parked;
  const note = el("div", "parked-note");
  const text = project
    ? `${tilde(path)} is one application (${kindLabel(project)}), not a folder of them.`
    : `${tilde(path)} has no applications yet. Each folder you put in it becomes one.`;
  const act = el("button", null, project ? "Add It as One Application" : "Create One…");
  act.onclick = async () => {
    if (!project) {
      parked = null;
      render();
      return late.openNewApp();
    }
    try {
      const name = await invoke("link_folder", { path });
      parked = null;
      await call("state");
      select(name);
    } catch (e) {
      inlineError(act, String(e));
    }
  };
  const close = el("button", "parked-close");
  close.append(icon("x"));
  close.title = "Dismiss";
  close.setAttribute("aria-label", "Dismiss");
  close.onclick = () => {
    parked = null;
    render();
  };
  note.append(close, el("p", null, text), act);
  return note;
}

function renderList() {
  const list = $("#list");
  list.replaceChildren();
  const note = filterNote("apps", kindLabel);
  const hiddenApps = hiddenSites();
  if (note && (snap.sites.length || hiddenApps.length)) list.append(note);
  const parkedNote = snap.sites.length && parkedNoteEl();
  if (parkedNote) list.append(parkedNote);

  if (!snap.sites.length && !hiddenApps.length) {
    list.append(el("p", "empty", "No applications yet."));
    return;
  }

  // Worktrees nest under their app, linked or parked. Another name linked to an app's folder
  // is an alias: its app's header lists it, it's no row of its own.
  const alias = new Set(snap.sites.filter((s) => !s.worktree_of).flatMap((s) => aliasesOf(s).map((a) => a.name)));
  const apps = new Set(snap.sites.filter((s) => !s.worktree_of && !alias.has(s.name)).map((s) => s.name));
  // A worktree whose app is gone stands alone, in its folder's section else Linked.
  const orphan = (s) => s.worktree_of && !apps.has(s.worktree_of);
  const gone = [...justHid.values()].filter((h) => !snap.sites.some((s) => s.name === h.name));
  const parked = [
    ...snap.sites.filter((s) => (!s.linked && !s.worktree_of) || (orphan(s) && s.parked_in)),
    ...hiddenApps,
    ...gone.map((h) => ({ ...h, justHid: true })),
  ];
  const wts = new Map<string, any[]>();
  for (const s of snap.sites) if (s.worktree_of && !orphan(s)) wts.set(s.worktree_of, [...(wts.get(s.worktree_of) ?? []), s]);
  const linked = snap.sites.filter((s) => (s.linked && !s.worktree_of && !alias.has(s.name)) || (orphan(s) && !s.parked_in));

  const order = sortBy === "modified" ? (a, b) => b.modified - a.modified : (a, b) => a.name.localeCompare(b.name);

  const q = filter.toLowerCase();
  const f = filterOf("apps");
  const hit = (s) =>
    (!f.kind || s.kind === f.kind) &&
    // A crashed app may still run its other processes, and it wants a look: it stays.
    (!f.running || ["running", "starting", "crashed"].includes(snap.app_states[s.name])) &&
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
      if (s.justHid) {
        rows.push(hidRow(s));
        continue;
      }
      rows.push(row(s, { kids: kids.length, path, kind }));
      if (shownKids.length) {
        const inner = document.createElement("div");
        inner.append(...shownKids.map((k) => row(k, { child: true })));
        // Always rendered so folding can slide; while searching, matches stay open.
        const wrap = document.createElement("div");
        wrap.className = q || !collapsed.has(s.name) ? "kids" : "kids closed";
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
    section("Linked", linked, true);
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

export { M };

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
  if (isMain(s)) items.push(await M.MenuItem.new({ text: "Add Alias…", action: () => addAlias(s) }));
  if (!s.linked) {
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
      text: s.linked ? "Remove Link…" : "Hide from Anvil",
      action: async () => {
        // Hide keeps the app's settings and has Undo; removing a link deletes them: asked first.
        if (s.linked) {
          const anchor = $(`#list [data-name="${CSS.escape(s.name)}"]`) ?? $("#site .site-name");
          const pop = el("div", "pop-confirm wide");
          const no = el("button", null, "Cancel");
          const yes = el("button", "primary", "Remove");
          const said = `Remove ${s.name} from Anvil? Its processes stop and its settings here are deleted: PHP version, HTTPS, php.ini and extensions. Its folder stays.`;
          pop.append(el("p", null, said), no, yes);
          const close = popAt(anchor, pop, yes);
          no.onclick = close;
          yes.onclick = () => {
            close();
            call("remove_site", { site: s.name });
          };
          return;
        }
        justHid.set(s.name, s);
        setTimeout(() => {
          if (justHid.delete(s.name) && mode === "apps") renderList();
        }, 6000);
        await call("remove_site", { site: s.name });
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
  preview?: (v: string) => string | Node;
  /** Checkboxes under the field, each with a line about it. */
  checks?: { label: string; checked?: boolean; note?: string }[];
  body?: Node;
  /** An error it opens with, inline. */
  error?: string;
  ok: string;
  /** `ok` while `submit` works, for work that takes a while ("Creating…"). */
  busy?: string;
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
  const show = () => hint.replaceChildren(o.preview ? o.preview(input.value.trim()) : "");
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
      error.hidden = true;
      if (o.busy) ok.replaceChildren(el("span", "spin-dot"), o.busy);
      try {
        await o.submit(value, checked);
        finish({ value, checked });
      } catch (err) {
        error.textContent = String(err);
        error.hidden = false;
      } finally {
        ok.disabled = false;
        ok.textContent = o.ok;
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
  const [db, branches] = await Promise.all([
    invoke("worktree_new_db", { site: s.name }).catch(() => null),
    invoke<string[]>("worktree_branches", { site: s.name }).catch(() => [] as string[]),
  ]);
  const copyable = !!db?.main_database && db.can_copy;
  let made: string | null = null;
  // An existing branch is checked out, any other name made from the app's current branch:
  // the preview says which, so there's nothing to tick.
  const answer = await ask({
    title: `New worktree of ${s.name}`,
    field: "Branch",
    placeholder: "feature/login",
    preview: (v) => {
      const b = v.trim();
      // The address stands out from the word before it, and can be selected.
      const line = document.createDocumentFragment();
      line.append(
        b ? (branches.includes(b) ? "Existing branch · " : "New branch · ") : "",
        el("span", "ask-addr", `${s.name}--${(b || "feature/login").toLowerCase().replace(/[^a-z0-9.-]/g, "-")}.${snap.tld}`),
      );
      return line;
    },
    checks: copyable
      ? [
          {
            label: "Copy the database too",
            checked: db.copy,
            note: `Off: an empty database of its own. On: a copy of ${db.main_database}.`,
          },
        ]
      : [],
    ok: "Create",
    busy: "Creating…",
    submit: async (raw, [copy]) => {
      const branch = raw.trim();
      await callOrThrow("worktree_add", { site: s.name, branch, new: !branches.includes(branch), copyDb: copyable ? copy : null });
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
  // The row says Removing… until the folder is gone; a failure is said under its name.
  const remove = async (force: boolean) => {
    removing.add(s.name);
    render();
    try {
      await askSnap("worktree_remove", { site: s.name, dropDb: !!drop?.checked, force });
      close();
    } finally {
      removing.delete(s.name);
      render();
    }
  };
  const anyway = async () => {
    close();
    try {
      await remove(true);
    } catch (e) {
      actionErrors.set(s.name, String(e));
      select(s.name);
    }
  };
  yes.onclick = async () => {
    if (force) return anyway();
    yes.disabled = true;
    yes.replaceChildren(el("span", "spin-dot"), "Removing…");
    // Refused (commits on no branch, say): said here, and the button becomes Remove anyway.
    try {
      await remove(false);
    } catch (e) {
      $("p", pop).textContent = `${String(e)}.`;
      yes.disabled = false;
      yes.textContent = "Remove anyway";
      yes.onclick = anyway;
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
    link.title = snap.running ? keys("Open in browser (⌘O)") : "Start Anvil to open this address";
    if (!securing.has(s.name)) addr.append(lockOf(s.secure));
    addr.append(link);
    if (securing.has(s.name)) {
      const busy = el("span", "securing", s.secure ? "Turning off HTTPS…" : "Turning on HTTPS…");
      busy.prepend(el("span", "spin-dot"));
      addr.append(busy);
    }
    // Aliases are linked names for the same folder.
    // A worktree's are the hosts derived from its app's aliases.
    const aliases = aliasesOf(s);
    if (aliases.length) {
      const chip = el("button", "alias-chip", `+${aliases.length}`);
      chip.title = "All addresses";
      chip.onclick = () => aliasPop(chip, [s, ...aliases]);
      addr.append(chip);
    }
    parts.push(addr);
  }
  if (s.worktree_of) parts.push(appLink("worktree of ", s.worktree_of));
  const kind = el("span", "addr", kindLabel(s.kind));
  if (!serves(s)) kind.prepend(unservedOf(s));
  parts.push(kind);
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
  box.replaceChildren(...metaParts(parts));
  drawHttps(s);
}

/** The header's meta items, each with its "·" before it, so a wrapped line can't start or
 * end with a lone one: the line's first "·" is clipped off (style.css, .site-meta). */
export const metaParts = (parts: HTMLElement[]) =>
  parts.map((p) => {
    const part = el("span", "part");
    part.append(el("span", "sep", "·"), p);
    return part;
  });

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

// An app's browser errors as last read (`browser_status`), by app: read again after 5 s.
const bridgeSeen = new Map<string, { at: number; st: { errors: number; since: number | null; blocked: boolean } }>();
const HM = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit" });

/** The Settings tab's "Browser errors" switch (PHP apps and dev servers, as
 * `browser::on`), the note when the app's CSP keeps the script out, and the Overview's
 * Browser line while it's on. */
function browserBridge(s, root) {
  const row = $(".browser-row", root);
  const line = $('[data-panel="overview"] .browser-line', root);
  if (!row) return;
  if (!(PHP_KINDS.has(s.kind) && s.php) && !(PROCESS_KINDS.has(s.kind) && s.port)) {
    row.remove();
    return;
  }
  const box = $("#app-browser", row);
  box.checked = !!s.browser;
  box.onchange = () => {
    bridgeSeen.delete(s.name);
    call("set_browser", { site: s.name, on: box.checked });
  };
  if (!s.browser) return;
  const draw = (st) => {
    $(".browser-blocked", row).hidden = !st.blocked;
    if (!line) return;
    line.hidden = false;
    const n = st.errors;
    $("dd", line).replaceChildren(
      st.blocked
        ? el("span", "error", "Blocked by the app’s Content-Security-Policy")
        : n
          ? el("span", null, `${n} error${n === 1 ? "" : "s"} since ${HM.format(new Date(st.since * 1000))}`)
          : el("span", "muted", "No errors"),
    );
  };
  const seen = bridgeSeen.get(s.name);
  if (seen) draw(seen.st);
  if (seen && Date.now() - seen.at < 5000) return;
  invoke("browser_status", { site: s.name })
    .then((st) => {
      bridgeSeen.set(s.name, { at: Date.now(), st });
      if (selected === s.name && row.isConnected) draw(st);
    })
    .catch(() => {});
}

// Apps whose node_modules are being installed again, and how the last one went.
const reinstalling = new Set<string>();
const reinstalled = new Map<string, { ok: boolean; text: string }>();

/** The Settings tab's "Shared dependencies" (pnpm apps only): a switch confirmed first,
 * since it installs node_modules again; Next.js keeps it off. */
function sharedDeps(s, root) {
  const row = $(".shared-row", root);
  if (!row) return;
  if (s.shared_deps === null) {
    row.remove();
    return;
  }
  const box = $("#app-shared", row);
  const busy = reinstalling.has(s.name);
  const next = s.kind === "next";
  box.checked = s.shared_deps;
  box.disabled = busy || next;
  $(".securing", row).hidden = !busy;
  const note = $(".shared-note", row);
  const last = reinstalled.get(s.name);
  const text = next ? "Off for Next.js: Turbopack doesn’t compile dependencies outside the project" : busy ? "" : (last?.text ?? "");
  note.hidden = !text;
  note.textContent = text;
  note.classList.toggle("error", !next && last?.ok === false);
  box.onchange = () => {
    const on = box.checked;
    box.checked = !on; // stays as it is until confirmed
    late.confirmPop(
      box,
      on ? "Install node_modules again as links into pnpm’s store?" : "Install node_modules again as the app’s own copy?",
      "Reinstall",
      async () => {
        reinstalling.add(s.name);
        reinstalled.delete(s.name);
        if (selected === s.name) sharedDeps(s, root);
        try {
          await invoke("set_shared_deps", { site: s.name, on });
          reinstalled.set(s.name, { ok: true, text: "Installed again; restart a running dev server so it picks up the new node_modules" });
        } catch (e) {
          reinstalled.set(s.name, { ok: false, text: String(e) });
        }
        reinstalling.delete(s.name);
        call("state");
      },
    );
  };
}

/** Every address of an app, each with its own Open. */
function aliasPop(anchor, sites) {
  const pop = el("div", "alias-pop");
  // Each address once, without its scheme: a lock says HTTPS, as in the list.
  const buttons = sites.map((a) => {
    const go = el("button", "alias-row");
    go.onclick = () => {
      close();
      invoke("open_url", { url: url(a) }).catch((e) => showError(String(e)));
    };
    go.append(lockOf(a.secure), el("span", "alias-url", url(a).replace(/^https?:\/\//, "")), el("span", "alias-open", "Open"));
    pop.append(go);
    return go;
  });
  const close = popAt(anchor, pop, buttons[0], "left");
}

/** "<words><app>", the app's name a link that selects it. */
function appLink(words: string, app: string, tag = "span", cls: string | null = null) {
  const box = el(tag, cls);
  const a = el("a", "app-link", app);
  a.href = "#";
  a.onclick = (e) => {
    e.preventDefault();
    select(app);
  };
  box.append(words, a);
  return box;
}

/** Logs and Packages only once there's something in them: asked once per app and run
 * state (an app that starts writes logs), not on every redraw. A Compose project's Logs
 * are its containers' (containers.ts). */
let tabsAsked = "";
// The header's height, for what sticks under it (the Requests and Logs filter bars).
const topSize = new ResizeObserver(([e]) => $("#site").style.setProperty("--top-h", `${(e.target as HTMLElement).offsetHeight}px`));
function hideEmptyTabs(s, root) {
  const key = `${s.name}:${snap.app_states[s.name] ?? ""}`;
  const logs = $('.tabs [data-tab="logs"]', root);
  const pkgs = $('.tabs [data-tab="packages"]', root);
  if (key === tabsAsked) {
    // A redraw: keep what was found.
    logs.hidden = s.kind !== "compose" && tabsHad.logs === false;
    pkgs.hidden = tabsHad.pkgs === false;
    return;
  }
  tabsAsked = key;
  tabsHad = {};
  if (s.kind !== "compose")
    invoke("log_sources", { site: s.name })
      .then((l: any[]) => {
        tabsHad.logs = l.length > 0;
        if (selected === s.name) $('#site .tabs [data-tab="logs"]').hidden = !tabsHad.logs;
      })
      .catch(() => {});
  invoke("packages", { site: s.name })
    .then((p: any) => {
      tabsHad.pkgs = p.ecosystems.length > 0;
      if (selected === s.name) $('#site .tabs [data-tab="packages"]').hidden = !tabsHad.pkgs;
    })
    .catch(() => {});
}
let tabsHad: { logs?: boolean; pkgs?: boolean } = {};

/** Overview › Aliases: a group of its own under the app's lines, one row per address with
 * Open, and the app's with Remove (after a confirm) and Add Alias. A worktree's are derived
 * from its app's: listed with Open, changed on the app. */
function aliasesLine(s, root) {
  const box = $(".aliases", root);
  const list = aliasesOf(s);
  const main = isMain(s);
  box.hidden = !serves(s) || (!main && !list.length);
  if (box.hidden) return;
  const add = $(".aliases-add", box);
  add.hidden = !main;
  add.onclick = () => addAlias(s);
  // A row per address: the address opens it; Remove asks in the row itself, no bubble.
  const rows = list.map((a) => {
    const row = el("div", "row alias-line");
    const dt = el("dt");
    const go = el("a", "alias-go", url(a).replace(/^https?:\/\//, ""));
    go.href = url(a);
    go.title = `Open ${url(a)}`;
    go.onclick = (e) => {
      e.preventDefault();
      invoke("open_url", { url: url(a) }).catch((err) => showError(String(err)));
    };
    dt.append(lockOf(a.secure), go);
    const dd = el("dd", "res-cells");
    const idle = () => {
      if (!main) return dd.replaceChildren();
      const remove = el("button", "alias-remove", "Remove");
      remove.onclick = () => {
        const no = el("button", "alias-remove", "Cancel");
        const yes = el("button", "alias-remove danger", "Remove");
        no.onclick = idle;
        yes.onclick = () => {
          yes.disabled = no.disabled = true;
          call("remove_site", { site: a.name });
        };
        dd.replaceChildren(el("span", "muted", "Remove this address? The folder stays."), no, yes);
      };
      dd.replaceChildren(remove);
    };
    idle();
    row.append(dt, dd);
    return row;
  });
  // A worktree's are its app's, under its own name: changed there.
  if (!main) rows.push(appLink("Added and removed on ", s.worktree_of, "p", "muted aliases-none"));
  else if (!rows.length) rows.push(el("p", "muted aliases-none", "None yet: another address for this app, e.g. admin." + s.host));
  $("dl", box).replaceChildren(...rows);
}

/** A small bubble under `anchor`, right-aligned with it (or left). Esc or a click elsewhere
 * closes it, Tab stays inside it, and the focus goes back to `anchor` when it was in the
 * bubble; the close function is returned. */
export function popAt(anchor: Element, pop: HTMLElement, focus?: HTMLElement | null, align: "left" | "right" = "right") {
  document.querySelector(".pop")?.remove();
  pop.classList.add("pop");
  pop.setAttribute("role", "dialog");
  if (!pop.hasAttribute("aria-label") && !pop.hasAttribute("aria-labelledby"))
    pop.setAttribute("aria-label", anchor.getAttribute("aria-label") || anchor.textContent?.trim() || "");
  const close = () => {
    const inside = pop.contains(document.activeElement);
    pop.remove();
    document.removeEventListener("mousedown", outside, true);
    if (inside) (anchor as HTMLElement).focus?.();
  };
  const outside = (e) => !pop.contains(e.target) && close();
  bubbleKeys(pop, close);
  const r = anchor.getBoundingClientRect();
  Object.assign(
    pop.style,
    align === "left"
      ? { top: `${r.bottom + window.scrollY + 6}px`, left: `${r.left}px` }
      : { top: `${r.bottom + window.scrollY + 6}px`, right: `${document.documentElement.clientWidth - r.right}px` },
  );
  document.body.append(pop);
  // A button near the left edge gets its bubble to the right instead of past the window.
  if (align === "right" && pop.getBoundingClientRect().left < 8) Object.assign(pop.style, { right: "", left: `${Math.max(8, r.left)}px` });
  // And one near the right edge (Packages' +) is moved left to fit, not past the window.
  const w = document.documentElement.clientWidth;
  if (align === "left" && pop.getBoundingClientRect().right > w - 8)
    pop.style.left = `${Math.max(8, w - pop.getBoundingClientRect().width - 8)}px`;
  // A button near the bottom gets it above, so its buttons aren't cut off.
  const below = pop.getBoundingClientRect();
  if (below.bottom > window.innerHeight - 8 && r.top - below.height - 6 > 8)
    pop.style.top = `${r.top + window.scrollY - below.height - 6}px`;
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
  const apps = { finder: fileManager, terminal: label(snap.terminal ?? "Terminal"), editor: snap.editor && label(snap.editor) };
  for (const b of root.querySelectorAll(".open-with [data-open]")) {
    const name = apps[b.dataset.open];
    b.hidden = !name;
    if (!name) continue;
    b.title = `Open in ${name}${b.dataset.open === "editor" ? keys(" (⌘E)") : ""}`;
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
  topSize.disconnect();
  topSize.observe($(".site-top", root));
  $(".site-name", root).textContent = s.name;
  // A worktree's mark is a branch, not its app's letter: it's a branch of another app.
  const big = s.worktree_of ? Object.assign(el("span"), { innerHTML: BRANCH }) : mark(s);
  $(".site-title .mark", root).replaceWith(Object.assign(big, { className: `mark big${s.worktree_of ? " wt-mark" : ""}` }));
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

  // Caddy serves neither a compose project nor a plain folder; Request log off keeps none.
  $('.tabs [data-tab="requests"]', root).hidden = !snap.request_log || s.kind === "compose" || s.kind === "folder";
  // Only while the mail catcher is on (Settings › Features); its switch lives in the tab.
  $('.tabs [data-tab="mail"]', root).hidden = !snap.catcher || s.kind === "compose" || s.kind === "folder";
  hideEmptyTabs(s, root);
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
  if (s.kind === "wordpress" && s.php && snap.php_no_mysqli?.includes(s.php)) mysqliLine(s, root);
  aliasesLine(s, root);
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

  renderRunState(s, root);
  if (process) renderRunSettings(s, root);
  if (s.kind === "django") djangoHint(root);
  if (s.kind === "next" && s.host !== `${s.name}.${snap.tld}`) nextHint(s, root);

  // Settings
  const kindSel = $("#kind", root);
  kindSel.add(new Option(`Detected (${kindLabel(s.kind_detected)})`, ""));
  for (const [id, name] of snap.kinds) kindSel.add(new Option(name, id));
  kindSel.value = s.kind === s.kind_detected ? "" : s.kind;
  fieldNote(kindSel, `${s.name}/kind`);
  kindSel.onchange = () => setRun(s, kindSel, "kind", "set_app", kindSel.value ? { kind: kindSel.value } : { reset: ["kind"] });

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
  php.onchange = () =>
    setPhp(s, php.value || null, php.value || (filePhp && snap.php_installed.includes(filePhp) ? filePhp : snap.php_default));
  phpState(s, php);

  if (!process && serves(s)) webRoot(s, root);
  browserBridge(s, root);
  sharedDeps(s, root);
  if (!process && s.php) appExtensions(s, root);
  else root.querySelector(".app-exts-row")?.remove();

  // Per-app php.ini overrides; placeholders show what applies without one.
  const base = { memory_limit: "512M", upload_max_filesize: "100M", max_execution_time: "120" };
  for (const input of root.querySelectorAll("[data-ini]")) {
    const key = input.dataset.ini;
    input.value = s.ini[key] ?? "";
    input.placeholder = `${snap.ini[key] ?? base[key]} (default)`;
    fieldNote(input, `${s.name}/${key}`);
    input.onchange = () => {
      const value = input.value.trim() || null;
      // The upload limit is also the largest form a POST may send: both in one restart.
      const values =
        key === "upload_max_filesize"
          ? [
              [key, value],
              ["post_max_size", value],
            ]
          : [[key, value]];
      setIni(s, input, key, values);
    };
  }
  const known = new Set([...Object.keys(base), "post_max_size"]);
  const extra = $("#ini-extra", root);
  extra.value = Object.entries(s.ini)
    .filter(([k]) => !known.has(k))
    .map(([k, v]) => `${k} = ${v}`)
    .join("\n");
  fieldNote(extra, `${s.name}/extra`);
  extra.onchange = () => {
    const wanted = new Map();
    for (const line of extra.value.split("\n")) {
      const m = line.match(/^\s*([^=\s]+)\s*=\s*(.*?)\s*$/);
      if (m) wanted.set(m[1], m[2]);
    }
    const values = [
      ...Object.keys(s.ini)
        .filter((k) => !known.has(k) && !wanted.has(k))
        .map((k) => [k, null]),
      ...[...wanted].filter(([k, v]) => s.ini[k] !== v),
    ];
    if (values.length) setIni(s, extra, "extra", values);
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

// A field whose change restarts something (a php.ini value: the web server, when it
// serves; the app's kind, start command, port or version: the app, when it runs): said
// beside the field while it does, then that it's done, or why not. By `<app>/<field>`.
const fieldNotes = new Map<string, [string, boolean]>(); // what was said, whether it failed

function fieldNote(field: HTMLElement, at: string) {
  field.parentElement?.querySelector(".field-note")?.remove();
  const said = fieldNotes.get(at);
  if (!said) return;
  const note = el("small", `field-note ${said[1] ? "error-text" : "muted"}`, said[0]);
  note.setAttribute("role", "status");
  field.after(note);
}

async function setIni(s, field: HTMLElement, name: string, values: (string | null)[][]) {
  const at = `${s.name}/${name}`;
  if (snap.running) fieldNotes.set(at, ["Restarting the web server…", false]);
  fieldNote(field, at);
  try {
    await callOrThrow("set_site_ini", { site: s.name, values });
    if (snap.running) fieldNotes.set(at, ["Applied; the web server restarted.", false]);
    else fieldNotes.delete(at);
  } catch (e) {
    fieldNotes.set(at, [String(e), true]);
  }
  if (field.isConnected) fieldNote(field, at);
  else render();
}

/** `cmd` (`set_app`, `set_runtime`) for `s`: it says whether it restarted the app. */
async function setRun(s, field: HTMLElement, name: string, cmd: string, args: Record<string, unknown>) {
  const at = `${s.name}/${name}`;
  if (["running", "starting", "crashed"].includes(snap.app_states[s.name])) fieldNotes.set(at, [`Restarting ${s.name}…`, false]);
  else fieldNotes.delete(at);
  fieldNote(field, at);
  try {
    await callOrThrow(cmd, { site: s.name, ...args });
    const failed = snap.app_errors[s.name];
    if (!snap.restarted) fieldNotes.delete(at);
    else if (failed) fieldNotes.set(at, [failed, true]);
    else fieldNotes.set(at, [`Applied; ${s.name} restarted.`, false]);
  } catch (e) {
    fieldNotes.set(at, [String(e), true]);
  }
  if (field.isConnected) fieldNote(field, at);
  else render();
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
    const needs = on ? undefined : snap.php_ext_needs?.[name];
    const label = el("label", "ver-ext");
    const input = Object.assign(document.createElement("input"), {
      type: "checkbox",
      checked: on && !unbuilt,
      disabled: forAll || building || !!needs,
    });
    input.setAttribute("role", "switch");
    input.onchange = () => setAppExt(s.name, name, input.checked);
    if (forAll) label.title = `On for every app on PHP ${s.php} (Settings › PHP)`;
    label.append(input, el("span", "ver-ext-name", name));
    if (building) label.append(el("span", "muted", "Building…"));
    if (unbuilt) label.append(el("span", "muted", "Not built"));
    if (needs) label.append(el("span", "muted", needs));
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

const phpSwitching = new Map<string, [string, string]>(); // app -> [the version picked, what it does]
const phpErrors = new Map<string, string>(); // app -> the extensions its new PHP runs without

/** A version change first builds the extensions the app uses that `minor` lacks (a pecl
 * download and compile each), then restarts PHP: said beside the select, a failure under it. */
async function setPhp(s, version: string | null, minor: string) {
  const want = [...(snap.php_ext?.[minor] ?? []), ...s.ext];
  const built = snap.php_ext_built?.[minor];
  const build = built ? (snap.php_ext_catalog ?? []).filter((n) => want.includes(n) && !built.includes(n)) : [];
  phpSwitching.set(s.name, [version ?? "", build.length ? `Building ${build.join(", ")}…` : "Switching…"]);
  phpErrors.delete(s.name);
  render();
  try {
    await askSnap("set_php", { site: s.name, version });
  } catch (e) {
    phpErrors.set(s.name, String(e));
    await askSnap("state").catch(() => {});
  }
  phpSwitching.delete(s.name);
  render();
}

/** The PHP version select's busy line and last failure. */
function phpState(s, php) {
  const doing = phpSwitching.get(s.name);
  php.disabled = !!doing;
  if (doing) {
    php.value = doing[0];
    const busy = el("span", "php-busy", doing[1]);
    busy.setAttribute("role", "status");
    busy.prepend(el("span", "spin-dot"));
    php.after(busy);
  }
  const why = phpErrors.get(s.name);
  if (!why) return;
  const error = el("div", "ver-error php-error");
  error.setAttribute("role", "status");
  error.append(el("span", null, why));
  php.parentElement.append(error);
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

const hooksOpen = new Set<string>(); // apps whose Worktrees editor is open

/** Where an app's hooks come from, said once: "From the Laravel preset", changed here. */
function hooksFrom(from: Record<string, string>): string {
  const layers = [...new Set(Object.values(from))].filter((l) => l !== "Settings");
  const others = layers.filter((l) => l !== "this app").map((l) => (l.endsWith(" preset") ? `the ${l}` : l));
  if (layers.includes("this app"))
    return others.length ? `Changed for this app; the rest from ${others.join(" and ")}` : "Changed for this app";
  return others.length ? `From ${others.join(" and ")}` : "From Settings › Worktrees";
}

/** The same, as a small tag ("Laravel preset", "Changed for this app"); the sentence on hover. */
function hooksTag(from: Record<string, string>): string {
  const layers = [...new Set(Object.values(from))].filter((l) => l !== "Settings");
  if (layers.includes("this app")) return "Changed for this app";
  return layers.join(" · ") || "Settings › Worktrees";
}

/** The Settings tab's Worktrees section, for a git repository: what a new worktree gets as a
 * short checklist, where that comes from as a tag, Customize… at its top right. Open, the
 * editors (hooksform.ts) save each change for this app only; Done and Reset to preset
 * (drops this app's own) sit at the bottom. */
async function renderHooks(s, root) {
  const box = $(".wt-hooks", root);
  let v;
  try {
    v = await invoke("worktree_hooks", { site: s.name });
  } catch {
    return; // not a git repository: no worktrees to set up
  }
  if (box.contains(document.activeElement) && document.activeElement?.tagName !== "BUTTON") return; // being typed in
  const open = hooksOpen.has(s.name);
  const toggle = () => {
    if (open) hooksOpen.delete(s.name);
    else hooksOpen.add(s.name);
    renderHooks(s, root);
  };
  const head = el("dd", "wt-summary");
  const top = el("div", "wt-head");
  const title = el("div", "wt-title");
  title.append(el("span", "wt-lead", open ? "Changes save as you make them, for this app only." : "A new worktree of this app gets:"));
  const tag = el("span", "wt-tag", hooksTag(v.hooks.from));
  tag.title = hooksFrom(v.hooks.from);
  title.append(tag);
  top.append(title);
  if (!open) {
    const b = el("button", "inline-action wt-toggle", "Customize…");
    b.onclick = toggle;
    top.append(b);
  }
  head.append(top);
  if (!open) {
    const { gets, offers } = summary(v.hooks, v.install);
    const list = el("ul", "wt-gets");
    for (const line of gets) {
      const li = el("li");
      li.append(icon("check"), el("span", null, line.text));
      if (line.title) li.title = line.title;
      list.append(li);
    }
    head.append(list);
    if (offers) head.append(el("div", "wt-offers", offers));
  }
  // A layer with a mistake (a typo in config.toml or .anvil.toml): one line each.
  for (const p of v.hooks.problems ?? []) head.append(el("small", "error-text", p));
  const row = el("div", "row row-top");
  row.append(el("dt", null, "Worktrees"), head);
  if (!open) {
    box.replaceChildren(row);
    box.hidden = false;
    return;
  }
  const save = (field: string, value: unknown) => invoke("set_app_hooks", { site: s.name, field, value }).then(() => renderHooks(s, root));
  const fields = ["copy", "copy_database", "keys", "after_create"] as const;
  const rows = fields.map((field) => {
    const r = el("div", "row row-top");
    const dd = el("dd");
    dd.append(editor(field, v.hooks, { save, install: v.install, root: s.path }));
    r.append(el("dt", null, NAMES[field]), dd);
    return r;
  });
  const foot = el("div", "row wt-foot");
  const actions = el("dd", "wt-actions");
  const done = el("button", "inline-action primary", "Done");
  done.onclick = toggle;
  actions.append(done);
  const fail = el("small", "error-text");
  fail.hidden = true;
  if (Object.keys(v.own).length) {
    const reset = el("button", "link", "Reset to preset");
    reset.title = "Drop this app’s own changes";
    reset.onclick = () =>
      invoke("set_app_hooks", { site: s.name, field: null, value: null }).then(
        () => renderHooks(s, root),
        (e) => {
          fail.textContent = String(e);
          fail.hidden = false;
        },
      );
    actions.append(reset);
  }
  actions.append(fail);
  foot.append(el("dt"), actions);
  box.replaceChildren(row, ...rows, foot);
  box.hidden = false;
}

/** Django checks the host name and, behind HTTPS, the scheme: two settings it needs once. */
/** The version the app's .anvil.toml asks for, if it asks. */
function fromFile(s, kind: string): string | null {
  return snap.projects?.[s.name]?.find((n) => n.kind === kind)?.version ?? null;
}

const mysqliErrors = new Map<string, string>(); // PHP version -> why building its mysqli failed

/** WordPress reaches MySQL through mysqli, which Anvil's macOS PHP comes without: a line
 * under the runtime that builds it (a minute, once per version). */
function mysqliLine(s, root) {
  const v = s.php;
  const key = `mysqli:${v}`;
  const b = el("button", "inline-action", installing.has(key) ? "Building mysqli…" : "Build mysqli");
  b.disabled = installing.has(key);
  b.onclick = async () => {
    installing.add(key);
    mysqliErrors.delete(v);
    render();
    try {
      await invoke("php_mysqli", { version: v });
    } catch (e) {
      mysqliErrors.set(v, String(e));
    }
    installing.delete(key);
    call("state");
  };
  const dd = el("dd", "muted", `PHP ${v} lacks mysqli, which WordPress needs for MySQL. `);
  dd.append(b);
  if (mysqliErrors.has(v)) dd.append(el("small", "error-text", ` ${mysqliErrors.get(v)}`));
  const row = el("div", "row");
  row.append(el("dt", null, "mysqli"), dd);
  $('[data-field="runtime"]', root).closest(".row")?.after(row);
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
    // The Database viewer opens it once it's there.
    if (late.browsable() && snap.db_viewer?.apps[s.name] && !v.pending && !v.preparing) dd.append(" ", late.browseButton(s.name));
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
    const all = el("button", "clean-all", `Delete Logs and Caches (${bytes(safe)})`);
    all.title = `Deletes ${safeItems.map((i) => i.label.toLowerCase()).join(", ")}`;
    const box = el("span", "actions-inline");
    all.onclick = () => cleanItems(name, safeItems, box);
    box.append(all);
    head.append(box);
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
      del.onclick = () => cleanItems(name, [i], dd);
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
    body.append(el("p", "muted", "None in this app’s .env"));
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

/** Asks in `box` (Settings › Storage's words), then deletes the items one by one. */
function cleanItems(name, items, box: HTMLElement) {
  const total = items.reduce((a, i) => a + i.bytes, 0);
  const was = [...box.childNodes];
  const back = () => {
    box.classList.remove("asking");
    box.replaceChildren(...was);
  };
  const out = items.some((i) => i.label === "Sessions") ? " Users get logged out." : "";
  confirmInRow(
    box,
    `Delete ${bytes(total)}?${out}`,
    "Delete",
    async () => {
      let freed = 0;
      for (const i of items) {
        try {
          freed += await invoke("clean", { site: name, path: i.path });
        } catch (e) {
          showError(String(e));
          break;
        }
      }
      // Said next to Delete Logs and Caches, not in a toast; it stays until the next one or reload.
      if (freed) freedNote.set(name, `Freed ${bytes(freed)}`);
      loadResources(name, true);
    },
    back,
  );
}

export const PHP_KINDS = new Set(["laravel", "statamic", "symfony", "wordpress", "php"]);
export const PROCESS_KINDS = new Set(["next", "nuxt", "vite", "node", "rust", "go", "dotnet", "django", "fastapi", "flask"]);

/** The run error: safe to redraw while the user types elsewhere. The run box is
 * the Overview's (processes.js), for every process of the app. */
function renderRunState(s, root) {
  const error = snap.app_errors[s.name] ?? actionErrors.get(s.name);
  const err = $(".run-error", root);
  err.textContent = error ?? "";
  err.hidden = !error;
  if (snap.app_errors[s.name] && $('.tabs [data-tab="logs"]', root)) {
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
  fieldNote(rt, `${s.name}/runtime`);
  rt.onchange = () => setRun(s, rt, "runtime", "set_runtime", { version: rt.value || null });
  // dotnet picks the SDK from the project's global.json: nothing to pin.
  if (rtId === "dotnet") rt.closest(".row").remove();

  const cmd = $("#app-command", root);
  cmd.value = s.command_custom ? s.command : "";
  cmd.placeholder = s.command_custom ? "" : (s.command ?? "");
  fieldNote(cmd, `${s.name}/command`);
  cmd.onchange = () => setRun(s, cmd, "command", "set_app", cmd.value.trim() ? { command: cmd.value.trim() } : { reset: ["command"] });

  const port = $("#app-port", root);
  port.value = s.port ?? "";
  fieldNote(port, `${s.name}/port`);
  port.onchange = () => {
    const n = parseInt(port.value, 10);
    setRun(s, port, "port", "set_app", Number.isNaN(n) ? { reset: ["port"] } : { port: n });
  };

  const auto = $("#app-autostart", root);
  auto.checked = s.autostart;
  auto.onchange = () => call("set_app", { site: s.name, autostart: auto.checked });
}

// While an app is starting, check back until its port answers: the dots and run errors
// alone (`run_states`, no scan). Only the dots that changed and the run box are
// redrawn, so nothing the user is typing or hovering is.
let starting = false; // that check on its way
export async function checkStarting() {
  if (starting || !shown() || mode !== "apps" || !snap || !Object.values(snap.app_states).includes("starting")) return;
  starting = true;
  const asked = snapAsked;
  let next;
  try {
    next = await invoke("run_states");
  } catch {
    return;
  } finally {
    starting = false;
  }
  // A whole snapshot asked for since is newer.
  if (mode !== "apps" || snapShown > asked) return;
  const before = snap.app_states;
  Object.assign(snap, next);
  const changed = new Set([...Object.keys(before), ...Object.keys(snap.app_states)].filter((k) => before[k] !== snap.app_states[k]));
  // "Running only" may take a row out or put one in.
  if (changed.size && filterOf("apps").running) renderList();
  else
    for (const b of document.querySelectorAll<HTMLElement>("#list .item:not(.hidden-app)"))
      if (changed.has(b.dataset.name!)) {
        const state = snap.app_states[b.dataset.name!];
        if (b.classList.contains("child")) wtState($(".wt-glyph", b), state);
        else appBadge($(".mark-box", b), state);
      }
  const s = snap.sites.find((x) => x.name === selected);
  const root = $("#site");
  if (s && PROCESS_KINDS.has(s.kind) && $(".run-box", root)) renderRunState(s, root);
}
setInterval(checkStarting, 1500);

/** Focus a sidebar row, scrolling only when it is out of view. */
export function focusRow(row: HTMLElement | null) {
  row?.focus({ preventScroll: true });
  row?.scrollIntoView?.({ block: "nearest" });
}

export function select(name) {
  if (name !== selected) actionErrors.clear();
  selected = name;
  localStorage.setItem("selected", name);
  renderList();
  renderSite();
  // Scrolled only as far as needed (keys moving past the edge): focus alone centres it.
  focusRow($(`.item[data-name="${CSS.escape(name)}"]`));
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
  const menu = await M.Menu.new({
    items: await Promise.all([
      M.MenuItem.new({ text: "New Application…", accelerator: "CmdOrCtrl+N", action: () => late.openNewApp() }),
      M.PredefinedMenuItem.new({ item: "Separator" }),
      M.MenuItem.new({ text: "Add Folder of Applications…", action: addAppsFolder }),
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

// Keyboard: ⌘1–7 tabs (Ctrl+1–9 the shown tabs without a menu bar), ⌘O browser, ⌘E editor, ⌘T terminal, ⌘G git app, ⌘L log.
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
    addAppsFolder();
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
  // ⌘ on macOS, Ctrl on Windows; Ctrl+A/C/V/X/Z aren't shortcuts here, so editing keeps them.
  if (!mod(e) || e.shiftKey || e.altKey) return;
  // In text fields only the app-wide keys apply; the rest belong to editing.
  const typing = (e.target as HTMLElement).closest("input, textarea, select");
  // macOS's View menu names a tab for each of ⌘1–7. Without that menu (Windows, Linux)
  // Ctrl+1–9 are the tabs as they show, a hidden one (Git, Dev, Mail) not counted.
  const shown = [...document.querySelectorAll<HTMLElement>("#site .tabs [data-tab]")].filter((t) => !t.hidden);
  const tab = mac ? TAB_KEYS[e.key] : /^[1-9]$/.test(e.key) && shown[Number(e.key) - 1]?.dataset.tab;
  const name = tab
    ? `tab:${tab}`
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

// A menu pick that made this window anew (it is freed after 10 minutes closed, see
// `show_main` in app/src/main.rs) runs once the first state is drawn.
const picked = window.__anvilMenu;
Promise.resolve(window.refresh()).then(() => picked && runAction(picked));

export function setMode(m) {
  mode = m;
  localStorage.setItem("mode", m);
  filter = "";
  $("#search").value = "";
  late.renderModes();
  renderMain();
}
