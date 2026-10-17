import type { Snapshot } from "./types";
import { late } from "./late";

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

export const LOCK = `<svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><rect x="3" y="7" width="10" height="7" rx="1.5" fill="currentColor"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>`;

export let snapAt = 0; // when the last snapshot arrived

export async function call(cmd: string, args?: Record<string, unknown>) {
  try {
    snap = (await invoke(cmd, args)) ?? snap;
    snapAt = Date.now();
    render();
  } catch (e) {
    showError(String(e));
  }
}

export function showError(msg) {
  $("#error-text").textContent = msg ?? "";
  $("#error").hidden = !msg;
}

export let toastTimer;
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
export const SETUP_FIELDS = ["resolver", "ports", "trusted", "shims"];

export async function loadSetup(busy?: string) {
  let st;
  try {
    st = await invoke("setup_status");
  } catch {
    return;
  }
  const bar = $("#setup");
  const machine = SETUP_FIELDS.some((f) => !st[f]);
  bar.hidden = !machine && st.php;
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
}

export function render() {
  showError(snap.error);

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
  const show = (src) => {
    if (!src) return;
    const img = new Image();
    img.src = src;
    img.alt = "";
    img.onload = () => m.replaceChildren(img);
  };
  if (icons.has(s.name)) show(icons.get(s.name));
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

export let sortBy = localStorage.getItem("sortBy") ?? "name";
// How the list is split after your own groups: by parked folder, by type, or not at all.
export let groupBy = localStorage.getItem("groupBy") ?? "folder";
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
export const collapsed = new Set(JSON.parse(localStorage.getItem("collapsed") ?? "[]"));

// Fold a site's linked domains in place (no re-render), so the slide can animate.
export function toggle(name, open = collapsed.has(name)) {
  if (open) collapsed.delete(name);
  else collapsed.add(name);
  localStorage.setItem("collapsed", JSON.stringify([...collapsed]));
  const parent = $(`.item[data-name="${CSS.escape(name)}"]`);
  if (!parent) return;
  parent.setAttribute("aria-expanded", String(open));
  $(".disclosure", parent).title = open ? "Hide linked domains" : "Show linked domains";
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
  b.dataset.name = s.name;
  b.setAttribute("aria-current", String(s.name === selected));
  if (child) {
    const branch = document.createElement("span");
    branch.className = "branch";
    branch.setAttribute("aria-hidden", "true");
    branch.textContent = "↪";
    b.append(branch);
  }
  const text = document.createElement("span");
  text.className = "text";
  text.innerHTML = `<span class="name"></span><span class="sub"></span>`;
  // The folder's name: not every project is a site, so no address in the list.
  $(".name", text).textContent = s.name;
  // HTTPS shows as a lock after the name; it's turned on and off from the right-click menu.
  // While it changes only a spinner shows: words would be cut off here; the header says them.
  if (securing.has(s.name)) {
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
  if (path && !child) $(".sub", text).textContent = tilde(s.path) + "/";
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
  if (child) end.append(el("span", "alias", "alias"));
  // A plain PHP app shows only its pinned version, not "PHP" twice.
  // Under a type heading the type would only repeat it.
  if (showKind && !child && s.kind !== "static" && !(s.kind === "php" && s.php_pinned)) {
    const kind = document.createElement("span");
    kind.className = `kind kind-${s.kind}`;
    kind.textContent = snap.kinds.find(([id]) => id === s.kind)?.[1] ?? s.kind;
    end.append(kind);
  }
  const d = el("span", "disclosure");
  if (kids) {
    const open = !collapsed.has(s.name);
    b.setAttribute("aria-expanded", String(open));
    d.title = open ? "Hide linked domains" : "Show linked domains";
    d.innerHTML = `<span class="count"></span><span class="chevron" aria-hidden="true">›</span>`;
    $(".count", d).textContent = kids;
    d.onclick = (e) => {
      e.stopPropagation();
      toggle(s.name);
    };
  }
  end.append(d);
  if (child) b.append(text, end);
  else {
    const state = snap.app_states[s.name];
    b.append(markBox(mark(s), state && (state === "running" ? "on" : "warm")), text, end);
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
    if (cardFor === s.name) return;
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

export let hoverTimer: ReturnType<typeof setTimeout> | null = null;
export let cardMuted: string | null = null; // the row a context menu was opened on
export let cardFor: string | null = null; // the site whose card is shown or on its way
export const cardProcs = new Map<string, any>(); // site name -> { at, req: promise of the `processes` overview }

export function hideCard() {
  clearTimeout(hoverTimer ?? undefined);
  hoverTimer = cardFor = null;
  $(".hover-card")?.remove();
}

/** A short summary next to the list: kind, address, what runs, branch and last edit. */
export function showCard(s, row) {
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
    else if (state) lines.push(el("span", "hc-run", state === "running" ? "Running" : "Starting"));
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
  create.onclick = openNewApp;
  box.append(
    icon,
    el("h1", null, "Welcome to Anvil"),
    el("p", "muted", `Add the folder where your projects live. Each project in it gets its own .${snap.tld} address.`),
    add,
    create,
  );
  root.replaceChildren(box);
}

export function renderList() {
  const list = $("#list");
  list.replaceChildren();
  const note = filterNote("apps", kindLabel);
  if (note && snap.sites.length) list.append(note);

  if (!snap.sites.length) {
    list.append(el("p", "empty", "No applications yet."));
    return;
  }

  // A linked name pointing at a parked project's folder is an extra domain for that project.
  const parked = snap.sites.filter((s) => !s.linked);
  const byPath = new Map(parked.map((s) => [s.path, []]));
  const linked: any[] = [];
  for (const s of snap.sites.filter((s) => s.linked)) (byPath.get(s.path) ?? linked).push(s);

  const order = sortBy === "modified" ? (a, b) => b.modified - a.modified : (a, b) => a.name.localeCompare(b.name);

  const q = filter.toLowerCase();
  const f = filterOf("apps");
  const hit = (s) =>
    (!f.kind || s.kind === f.kind) &&
    (!f.running || snap.app_states[s.name]) &&
    // Name, folder or type: "django" finds every Django app.
    (!q || `${s.host} ${s.path} ${kindLabel(s.kind)}`.toLowerCase().includes(q));
  let count = 0;

  // `key` remembers the fold; folders and Linked keep their old keys.
  const section = (title, sites, withChildren, path = true, kind = true, key = `apps:${title}`) => {
    const rows: any[] = [];
    for (const s of [...sites].sort(order)) {
      const kids = withChildren ? (byPath.get(s.path) ?? []) : [];
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
export async function setSecure(s) {
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

export async function siteMenu(s) {
  const sep = () => M.PredefinedMenuItem.new({ item: "Separator" });
  const groupOf = Object.keys(snap.groups).find((g) => snap.groups[g].includes(s.name));
  const groups = [...new Set(["Favorites", ...Object.keys(snap.groups)])];

  const items: any[] = [];
  if (serves(s)) {
    items.push(await M.CheckMenuItem.new({ text: "HTTPS", checked: s.secure, action: () => setSecure(s) }), await sep());
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

export function setView(key, value) {
  if (key === "sortBy") sortBy = value;
  else groupBy = value;
  localStorage.setItem(key, value);
  renderList();
}

/** Inline text prompt under the search field. Resolves to the text, or null when cancelled. */
/** Inline text prompt under the search field. `preview` turns the text into what
 * it will create. Only Return or Esc ends it, so a stray click keeps the typing. */
export function ask(label: string, placeholder?: string, preview?: (v: string) => string) {
  const form = $("#ask");
  const input = $("#ask-input");
  const hint = $("#ask-hint");
  const base = "Return to add, Esc to cancel";
  $("#ask-label").textContent = label;
  input.placeholder = placeholder;
  input.value = "";
  hint.textContent = base;
  form.hidden = false;
  input.focus();
  input.oninput = () => {
    const v = input.value.trim();
    hint.textContent = v && preview ? `${preview(v)}. ${base}` : base;
  };
  return new Promise((resolve) => {
    const done = (value) => {
      form.hidden = true;
      form.onsubmit = input.onkeydown = input.oninput = null;
      resolve(value);
    };
    form.onsubmit = (e) => {
      e.preventDefault();
      done(input.value.trim() || null);
    };
    // A block body: returning false from an on-handler would cancel every keystroke.
    input.onkeydown = (e) => {
      if (e.key === "Escape") done(null);
    };
  });
}

export async function addAlias(s) {
  const alias = await ask(
    `New domain for ${s.name}.${snap.tld}`,
    `e.g. admin.${s.name}`,
    (v) => `Opens ${v.replace(new RegExp(`\\.${snap.tld}$`), "")}.${snap.tld}`,
  );
  if (alias) await call("add_alias", { site: s.name, alias });
}

export async function newGroup(s) {
  const group = await ask(`Group for ${s.name}.${snap.tld}`, "Group name");
  if (group) await call("set_group", { site: s.name, group });
}

export let tab = localStorage.getItem("tab") ?? "overview";
export const kindLabel = (k) => snap.kinds.find(([id]) => id === k)?.[1] ?? k;
export const RUNTIME = {
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

export function showTab(root, name) {
  tab = name;
  localStorage.setItem("tab", name);
  // One stop in the Tab order; the arrow keys move between tabs.
  for (const b of root.querySelectorAll(".tabs [role=tab]")) {
    b.setAttribute("aria-selected", String(b.dataset.tab === name));
    b.tabIndex = b.dataset.tab === name ? 0 : -1;
  }
  for (const p of root.querySelectorAll("[role=tabpanel]")) p.hidden = p.dataset.panel !== name;
  $(".res-reload", root).hidden = name !== "resources";
  window.logsTab?.(root, name === "logs");
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

export const touchedCache = new Map<string, { at: number; secs: number | null }>(); // site name -> { at: ms when asked, secs: newest edit }

/** The newest edit (seconds) as known; asks again after 30 s and calls `done` with the answer. */
export function touchedOf(s, done) {
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
    // Aliases are linked names for the same folder (the rows under it in the list).
    const aliases = s.linked ? [] : snap.sites.filter((k) => k.linked && k.path === s.path);
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
}

/** Every address of an app, each with its own Open. */
export function aliasPop(anchor, sites) {
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

export const appIcons = new Map<string, Promise<string | null>>(); // app name -> promise of its icon as a data URL, or null

/** Finder, the terminal and the editor as their own colored icons; the outline stays when there is none. */
export function drawOpenWith(root) {
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
    appIconInto(b, name);
  }
}

/** `box`'s content becomes the app's own icon once known; the outline stays otherwise. */
export function appIconInto(box, name) {
  if (!appIcons.has(name))
    appIcons.set(
      name,
      invoke("app_icon", { name }).catch(() => null),
    );
  appIcons.get(name)!.then((src) => {
    if (src) box.replaceChildren(Object.assign(new Image(), { src, alt: "" }));
  });
}

export function renderSite() {
  const root = $("#site");
  const s = snap.sites.find((x) => x.name === selected);
  if (!snap.sites.length) return welcome(root);
  if (!s) {
    root.innerHTML = `<div class="placeholder">Select an application</div>`;
    return;
  }
  const process = PROCESS_KINDS.has(s.kind);

  root.replaceChildren($("#site-tpl").content.cloneNode(true));
  $(".site-name", root).textContent = s.name;
  $(".site-title .mark", root).replaceWith(Object.assign(mark(s), { className: "mark big" }));
  drawMeta();
  drawOpenWith(root);
  for (const b of root.querySelectorAll("[data-open]")) b.onclick = () => open(b.dataset.open);
  for (const el of root.querySelectorAll(".php-only")) el.hidden = process;
  for (const el of root.querySelectorAll(".process-only")) el.hidden = !process;

  for (const b of root.querySelectorAll(".tabs [role=tab]")) b.onclick = () => showTab(root, b.dataset.tab);
  $(".tabs", root).onkeydown = (e) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const tabs = [...root.querySelectorAll(".tabs [role=tab]:not([hidden])")];
    const i = tabs.findIndex((t) => t.dataset.tab === tab);
    const next = tabs[(i + (e.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length];
    showTab(root, next.dataset.tab);
    next.focus();
  };
  showTab(root, tab);

  // Overview: the type and what runs it in one line, e.g. "Laravel on PHP 8.4".
  const engine = process
    ? `${RUNTIME[s.kind]}${s.runtime_version && s.kind !== "dotnet" && s.kind !== "rust" ? ` ${s.runtime_version}` : ""}${s.runtime_version ? "" : " (not installed)"}`
    : s.php
      ? `PHP ${s.php}${s.php_pinned ? "" : " (default)"}`
      : PHP_KINDS.has(s.kind)
        ? "PHP, none installed"
        : null;
  const kind = s.kind === "static" ? "Static files" : kindLabel(s.kind);
  const runs = !engine || kind === engine.split(" ")[0] ? (engine ?? kind) : `${kind} on ${engine}`;
  $('[data-field="runtime"]', root).textContent = s.kind === s.kind_detected ? runs : `${runs} (type set by you)`;
  if (PHP_KINDS.has(s.kind) && !s.php) {
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
  $('[data-field="path"]', root).textContent = tilde(s.path);
  $('[data-field="root"]', root).textContent = tilde(s.root);
  // Nothing is served from a folder or a Compose project: no web root.
  if (!serves(s)) $('[data-field="root"]', root).closest(".row")?.remove();

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
  php.add(new Option(`Default (${snap.php_default ?? "none"})`, ""));
  for (const v of snap.php_installed) php.add(new Option(`PHP ${v}`, v));
  php.value = s.php_pinned ? s.php : "";
  php.onchange = () => call("set_php", { site: s.name, version: php.value || null });

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
  $(".res-reload", root).onclick = () => loadResources(s.name, true);
  window.renderGit?.(s);
  window.renderProcesses?.(s);
  window.renderSiteServices?.(s, root);
  window.renderPackages?.(s, root);
}

/** Django checks the host name and, behind HTTPS, the scheme: two settings it needs once. */
export function djangoHint(root) {
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
  copyBtn.onclick = () => late.copy(lines, "Copied the Django settings");
  env.append(el("pre", null, lines), copyBtn);
  box.append(env);
  $('[data-panel="overview"] .group', root).after(box);
}

/** Next.js only works at .localhost until its config allows the domain suffix. */
export function nextHint(s, root) {
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
export const SAFETY = {
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

export async function loadResources(name, force = false) {
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

export function renderResources(name) {
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

/** Confirm with a native dialog, then delete the items one by one. */
export async function cleanItems(name, items) {
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
  if (freed) toast(`Freed ${bytes(freed)}`);
  loadResources(name, true);
}

export const PHP_KINDS = new Set(["laravel", "statamic", "symfony", "php"]);
export const PROCESS_KINDS = new Set(["next", "nuxt", "vite", "node", "rust", "go", "dotnet", "django", "fastapi", "flask"]);

export function renderRun(s, root) {
  renderRunState(s, root);
  renderRunSettings(s, root);
}

/** The run error: safe to redraw while the user types elsewhere. The run box is
 * the Overview's (processes.js), for every process of the app. */
export function renderRunState(s, root) {
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

export function renderRunSettings(s, root) {
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
  const pinFile = { node: ".nvmrc or .node-version", python: ".python-version" }[rtId];
  rt.title = pinFile
    ? `Automatic uses ${pinFile} when the project has one, otherwise the newest installed`
    : "Automatic uses the newest installed";
  rt.disabled = versions.length < 2;
  rt.onchange = () => call("set_runtime", { site: s.name, version: rt.value || null });

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
setInterval(async () => {
  if (!snap || !Object.values(snap.app_states).includes("starting")) return;
  try {
    snap = await invoke("state");
  } catch {
    return;
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

export async function open(target) {
  try {
    await invoke("open", { site: selected, target });
    toast(
      {
        browser: "Opened in your browser",
        finder: "Shown in Finder",
        root: "Shown in Finder",
        terminal: "Opened in Terminal",
        editor: `Opened in ${snap.editor}`,
        git: `Opened in ${snap.git_client}`,
      }[target],
    );
  } catch (e) {
    showError(String(e));
  }
}

/** Applications served, each folder once: an alias shares its app's folder. */
export function servedCount() {
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

export const TEMPLATES = [
  ["laravel", "Laravel", "PHP"],
  ["statamic", "Statamic", "PHP"],
  ["symfony", "Symfony", "PHP"],
  ["php", "PHP", "Plain"],
  ["static", "Static", "HTML"],
  ["next", "Next.js", "Node.js"],
  ["nuxt", "Nuxt", "Node.js"],
  ["vite", "Vite", "Frontend"],
  ["node", "Node.js", "Server"],
  ["rust", "Rust", "Cargo"],
  ["go", "Go", "Server"],
  ["dotnet", ".NET", "ASP.NET"],
  ["django", "Django", "Python"],
  ["fastapi", "FastAPI", "Python"],
  ["flask", "Flask", "Python"],
];
export let newKind = localStorage.getItem("newKind") ?? "laravel";

export function openNewApp() {
  const dlg = $("#new-app");
  const grid = $("#new-kinds");
  grid.replaceChildren();
  for (const [id, name, sub] of TEMPLATES) {
    const label = el("label", "kind-card");
    const radio = el("input");
    radio.type = "radio";
    radio.name = "kind";
    radio.value = id;
    radio.checked = id === newKind;
    radio.onchange = () => {
      newKind = id;
      localStorage.setItem("newKind", id);
    };
    label.append(radio, el("strong", null, name), el("small", null, sub));
    grid.append(label);
  }
  const parent = $("#new-parent");
  parent.replaceChildren();
  for (const p of snap.paths) parent.add(new Option(tilde(p), p));
  parent.add(new Option("Choose a folder…", ""));
  parent.onchange = async () => {
    if (parent.value) return;
    const path = await window.__TAURI__.dialog.open({ directory: true, title: "Create the application in" });
    if (path) parent.add(new Option(tilde(path), path), 0);
    parent.selectedIndex = 0;
  };
  const name = $("#new-name");
  name.value = "";
  const preview = () => ($("#new-url").textContent = `Its address: ${name.value || "name"}.${snap.tld}`);
  name.oninput = () => {
    name.value = name.value.toLowerCase().replace(/[^a-z0-9.-]/g, "-");
    preview();
  };
  preview();
  $("#new-error").hidden = true;
  $("#new-log").hidden = true;
  $("#new-existing").hidden = false;
  setBusy(false);
  dlg.showModal();
  name.focus();
}

export function setBusy(busy) {
  $("#new-create").disabled = busy;
  $("#new-create").textContent = busy ? "Creating…" : "Create";
  for (const i of $("#new-form").querySelectorAll("input, select")) i.disabled = busy;
}

/** Serve one existing project folder. Resolves to whether one was added. */
export async function addAppFolder() {
  const path = await window.__TAURI__.dialog.open({ directory: true, title: "Choose a project folder" });
  if (!path) return false;
  try {
    const name = await invoke("link_folder", { path });
    await call("state");
    select(name);
    toast(`Added ${name}.${snap.tld}`);
    return true;
  } catch (e) {
    showError(String(e));
    return false;
  }
}

$("#new-existing").onclick = async () => {
  if (await addAppFolder()) $("#new-app").close();
};
$("#new-cancel").onclick = () => $("#new-app").close();
$("#new-log").onclick = () => invoke("open_create_log", { name: $("#new-name").value });
$("#new-form").onsubmit = async (e) => {
  e.preventDefault();
  const name = $("#new-name").value.trim();
  const parent = $("#new-parent").value;
  if (!name || !parent) return;
  setBusy(true);
  $("#new-log").hidden = false;
  $("#new-existing").hidden = true;
  $("#new-error").hidden = true;
  try {
    const site = await invoke("create_app", { kind: newKind, name, parent });
    $("#new-app").close();
    await call("state");
    select(site);
    toast(`Created ${site}.${snap.tld}`);
  } catch (err) {
    $("#new-error").textContent = String(err);
    $("#new-error").hidden = false;
    setBusy(false);
  }
};

$("#add").onclick = async () => {
  if (mode === "services") return late.openAddService();
  const pick = (title) => window.__TAURI__.dialog.open({ directory: true, title });
  const menu = await M.Menu.new({
    items: await Promise.all([
      M.MenuItem.new({ text: "New Application…", accelerator: "CmdOrCtrl+N", action: openNewApp }),
      M.PredefinedMenuItem.new({ item: "Separator" }),
      M.MenuItem.new({
        text: "Add Folder of Applications…",
        action: async () => {
          const path = await pick("Every project inside this folder becomes an application");
          if (!path) return;
          await call("park_folder", { path });
          toast("Folder added");
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
export function renderFilter() {
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

$("#error-close").onclick = () => showError(null);

// Keyboard: ⌘1–9 tabs in order, ⌘O browser, ⌘E editor, ⌘T terminal, ⌘G git app, ⌘L log.
/* ---------- Shortcuts: the menu bar and the keyboard run the same actions ---------- */

export const TAB_KEYS = { 1: "overview", 2: "git", 3: "logs", 4: "packages", 5: "resources", 6: "settings" };
export let lastAction = { name: "", at: 0 };

/** Run a menu action. The same key can reach both the page and the menu bar, so
 * a repeat within 300 ms is ignored. Returns whether it did something. */
export function runAction(name) {
  if (lastAction.name === name && Date.now() - lastAction.at < 300) return true;
  const root = $("#site");
  const [kind, arg] = name.split(":");
  let done = true;
  if (kind === "tab") {
    const t = $(`.tabs [data-tab="${arg}"]`, root);
    if (t && !t.hidden) showTab(root, arg);
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
    else openNewApp();
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
