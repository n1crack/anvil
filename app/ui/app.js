const { invoke } = window.__TAURI__.core;

const $ = (sel, root = document) => root.querySelector(sel);
let snap = null;
let selected = localStorage.getItem("selected");
let filter = "";
// The sidebar shows applications or services; services.js draws the latter.
let mode = localStorage.getItem("mode") === "services" ? "services" : "apps";
// List filters per mode: a kind (or null for all) and running only.
const filters = JSON.parse(localStorage.getItem("filters") ?? "{}");
const filterOf = (m) => (filters[m] ??= { kind: null, running: false });

const tilde = (p) => p.replace(/^\/Users\/[^/]+/, "~");
// The standard HTTPS port needs no ":443".
const url = (s) => `https://${s.name}.${snap.tld}${snap.https_port === 443 ? "" : `:${snap.https_port}`}`;

let snapAt = 0; // when the last snapshot arrived

async function call(cmd, args) {
  try {
    snap = (await invoke(cmd, args)) ?? snap;
    snapAt = Date.now();
    render();
  } catch (e) {
    showError(String(e));
  }
}

function showError(msg) {
  $("#error-text").textContent = msg ?? "";
  $("#error").hidden = !msg;
}

let toastTimer;
/** A short confirmation at the bottom of the window. */
function toast(msg, undo) {
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
const SETUP_STEPS = [
  ["resolver", "resolver", "Local domains", "Lets local addresses reach Anvil (asks for your password)"],
  ["trust", "trusted", "HTTPS certificate", "Lets browsers trust Anvil’s local certificates"],
  ["php", "php", "PHP", "Installs PHP 8.4 for PHP applications"],
];

async function loadSetup(fixing) {
  let st;
  try {
    st = await invoke("setup_status");
  } catch {
    return;
  }
  const bar = $("#setup");
  const missing = SETUP_STEPS.filter(([, field]) => !st[field]);
  bar.hidden = !missing.length;
  bar.replaceChildren(el("span", null, "Finish setting up"));
  for (const [id, , label, why] of missing) {
    const b = el("button", null, fixing === id ? `${label}…` : label);
    b.title = why;
    b.disabled = !!fixing;
    b.onclick = async () => {
      loadSetup(id);
      try {
        await invoke("setup_fix", { step: id });
      } catch (e) {
        showError(String(e));
      }
      loadSetup();
    };
    bar.append(b);
  }
}

function render() {
  showError(snap.error);

  const power = $("#power");
  power.setAttribute("aria-checked", snap.running);
  $("#power-label").textContent = snap.running ? "Serving" : "Stopped";

  if (!snap.sites.some((s) => s.name === selected)) selected = snap.sites[0]?.name;
  renderMain();
}

function renderMain() {
  renderFilter();
  if (mode === "services") {
    renderServiceList();
    renderService();
  } else {
    renderList();
    renderSite();
  }
}

const icons = new Map(); // site name -> data URL or null

function mark(s) {
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
  else invoke("icon", { site: s.name }).then((src) => { icons.set(s.name, src); show(src); }, () => icons.set(s.name, null));
  return m;
}

let sortBy = localStorage.getItem("sortBy") ?? "name";
// Sidebar sections folded from their heading, by "mode:title".
const folded = new Set(JSON.parse(localStorage.getItem("sections") ?? "[]"));

/** A sidebar section: a heading with a count that folds the rows under it.
 * While searching every section stays open. */
function listSection(key, title, rows, count) {
  const open = !!filter || !folded.has(key);
  const head = el("button", "section-head");
  head.setAttribute("aria-expanded", open);
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
    head.setAttribute("aria-expanded", opening);
    fold.classList.toggle("closed", !opening);
  };
  const h = el("h2");
  h.append(head);
  return [h, fold];
}
const collapsed = new Set(JSON.parse(localStorage.getItem("collapsed") ?? "[]"));

// Fold a site's linked domains in place (no re-render), so the slide can animate.
function toggle(name, open = collapsed.has(name)) {
  if (open) collapsed.delete(name);
  else collapsed.add(name);
  localStorage.setItem("collapsed", JSON.stringify([...collapsed]));
  const parent = $(`.item[data-name="${CSS.escape(name)}"]`);
  if (!parent) return;
  parent.setAttribute("aria-expanded", open);
  $(".disclosure", parent).title = open ? "Hide linked domains" : "Show linked domains";
  parent.nextElementSibling?.classList.toggle("closed", !open);
}

function row(s, { child = false, kids = 0, path = true } = {}) {
  const b = document.createElement("button");
  b.className = child ? "item child" : "item";
  b.dataset.name = s.name;
  b.setAttribute("aria-current", s.name === selected);
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
  $(".name", text).textContent = `${s.name}.${snap.tld}`;
  // In a folder's section the path only repeats the heading; linked apps and groups keep it.
  if (path && !child) $(".sub", text).textContent = tilde(s.path) + "/";
  else $(".sub", text).remove();
  if (!path && !child) b.classList.add("compact");

  const end = document.createElement("span");
  end.className = "end";
  if (child) end.append(el("span", "alias", "alias"));
  // A plain PHP app shows only its pinned version, not "PHP" twice.
  if (!child && s.kind !== "static" && !(s.kind === "php" && s.php_pinned)) {
    const kind = document.createElement("span");
    kind.className = `kind kind-${s.kind}`;
    kind.textContent = snap.kinds.find(([id]) => id === s.kind)?.[1] ?? s.kind;
    end.append(kind);
  }
  // Only call out sites that differ from the default.
  if (s.php_pinned) {
    const ver = document.createElement("span");
    ver.className = "ver";
    ver.textContent = `PHP ${s.php}`;
    end.append(ver);
  }
  // The running dot always sits last, as in the services list.
  const state = snap.app_states[s.name];
  if (state) {
    const dot = document.createElement("span");
    dot.className = `dot ${state === "running" ? "on" : "warm"}`;
    dot.title = state === "running" ? "Running" : "Starting";
    end.append(dot);
  }
  if (kids) {
    const open = !collapsed.has(s.name);
    b.setAttribute("aria-expanded", open);
    const d = document.createElement("span");
    d.className = "disclosure";
    d.title = open ? "Hide linked domains" : "Show linked domains";
    d.innerHTML = `<span class="count"></span><span class="chevron" aria-hidden="true">›</span>`;
    $(".count", d).textContent = kids;
    d.onclick = (e) => {
      e.stopPropagation();
      toggle(s.name);
    };
    end.append(d);
  }
  if (child) b.append(text, end);
  else b.append(mark(s), text, end);
  b.onclick = () => select(s.name);
  b.oncontextmenu = (e) => {
    e.preventDefault();
    select(s.name);
    siteMenu(s);
  };
  return b;
}

/** "Laravel only" with a Clear button above a filtered list. */
function filterNote(m, label) {
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
function welcome(root) {
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

function renderList() {
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
  const linked = [];
  for (const s of snap.sites.filter((s) => s.linked)) (byPath.get(s.path) ?? linked).push(s);

  const order = sortBy === "modified"
    ? (a, b) => b.modified - a.modified
    : (a, b) => a.name.localeCompare(b.name);

  const q = filter.toLowerCase();
  const f = filterOf("apps");
  const hit = (s) =>
    (!f.kind || s.kind === f.kind) &&
    (!f.running || snap.app_states[s.name]) &&
    (!q || `${s.name}.${snap.tld} ${s.path}`.toLowerCase().includes(q));
  let count = 0;

  const section = (title, sites, withChildren, path = true) => {
    const rows = [];
    for (const s of [...sites].sort(order)) {
      const kids = withChildren ? byPath.get(s.path) ?? [] : [];
      const parentHit = hit(s);
      const shownKids = kids.filter((k) => parentHit || hit(k));
      if (!parentHit && !shownKids.length) continue;
      rows.push(row(s, { kids: kids.length, path }));
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
    list.append(...listSection(`apps:${title}`, title, rows, rows.filter((r) => r.classList.contains("item")).length));
  };

  // User groups first (Favorites leads), then one section per parked folder, then linked folders.
  const grouped = new Set(Object.values(snap.groups).flat());
  const groupNames = Object.keys(snap.groups).sort((a, b) =>
    a === "Favorites" ? -1 : b === "Favorites" ? 1 : a.localeCompare(b));
  for (const g of groupNames) {
    section(g, parked.filter((s) => snap.groups[g].includes(s.name)), true);
  }
  const dirs = [...new Set(parked.map((s) => s.parked_in))].sort();
  for (const dir of dirs) {
    section(tilde(dir), parked.filter((s) => s.parked_in === dir && !grouped.has(s.name)), true, false);
  }
  section("Linked", linked, false);

  if (!count) {
    const p = document.createElement("p");
    p.className = "empty";
    const what = f.kind ? `${kindLabel(f.kind)} applications` : "applications";
    p.textContent = filter
      ? `No ${what} match “${filter}”.`
      : `No ${f.running ? "running " : ""}${what}.`;
    list.append(p);
  }
}

/* ---------- Context menu ---------- */

const M = window.__TAURI__.menu;

async function siteMenu(s) {
  const sep = () => M.PredefinedMenuItem.new({ item: "Separator" });
  const groupOf = Object.keys(snap.groups).find((g) => snap.groups[g].includes(s.name));
  const groups = [...new Set(["Favorites", ...Object.keys(snap.groups)])];

  const sort = await M.Submenu.new({
    text: "Sort By",
    items: await Promise.all([
      M.CheckMenuItem.new({ text: "Name", checked: sortBy === "name", action: () => setSort("name") }),
      M.CheckMenuItem.new({ text: "Date Modified", checked: sortBy === "modified", action: () => setSort("modified") }),
    ]),
  });

  const items = [sort, await sep()];
  if (!s.linked) {
    items.push(await M.MenuItem.new({ text: "Add Alias…", action: () => addAlias(s) }));
    const groupItems = await Promise.all(groups.map((g) =>
      M.CheckMenuItem.new({ text: g, checked: g === groupOf, action: () => call("set_group", { site: s.name, group: g === groupOf ? null : g }) })));
    groupItems.push(await sep(), await M.MenuItem.new({ text: "New Group…", action: () => newGroup(s) }));
    items.push(await M.Submenu.new({ text: "Add to Group", items: groupItems }), await sep());
  }
  items.push(await M.MenuItem.new({
    text: s.linked ? "Remove Link" : "Hide from Anvil",
    action: async () => {
      await call("remove_site", { site: s.name });
      if (!s.linked) toast(`Hid ${s.name}.${snap.tld}`, () => call("unhide_site", { site: s.name }));
    },
  }));
  (await M.Menu.new({ items })).popup();
}

function setSort(by) {
  sortBy = by;
  localStorage.setItem("sortBy", by);
  renderList();
}

/** Inline text prompt under the search field. Resolves to the text, or null when cancelled. */
/** Inline text prompt under the search field. `preview` turns the text into what
 * it will create. Only Return or Esc ends it, so a stray click keeps the typing. */
function ask(label, placeholder, preview) {
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
    input.onkeydown = (e) => e.key === "Escape" && done(null);
  });
}

async function addAlias(s) {
  const alias = await ask(`New domain for ${s.name}.${snap.tld}`, `e.g. admin.${s.name}`, (v) =>
    `Opens ${v.replace(new RegExp(`\\.${snap.tld}$`), "")}.${snap.tld}`);
  if (alias) await call("add_alias", { site: s.name, alias });
}

async function newGroup(s) {
  const group = await ask(`Group for ${s.name}.${snap.tld}`, "Group name");
  if (group) await call("set_group", { site: s.name, group });
}

let tab = localStorage.getItem("tab") ?? "overview";
const kindLabel = (k) => snap.kinds.find(([id]) => id === k)?.[1] ?? k;
const RUNTIME = { next: "Node.js", nuxt: "Node.js", vite: "Node.js", node: "Node.js", rust: "Rust", go: "Go", dotnet: ".NET" };

function showTab(root, name) {
  tab = name;
  localStorage.setItem("tab", name);
  // One stop in the Tab order; the arrow keys move between tabs.
  for (const b of root.querySelectorAll("[role=tab]")) {
    b.setAttribute("aria-selected", b.dataset.tab === name);
    b.tabIndex = b.dataset.tab === name ? 0 : -1;
  }
  for (const p of root.querySelectorAll("[role=tabpanel]")) p.hidden = p.dataset.panel !== name;
  $(".res-reload", root).hidden = name !== "resources";
  window.logsTab?.(root, name === "logs");
  // Measuring (du, lsof, docker) only happens when someone looks at it.
  if (name === "resources" && selected) loadResources(selected);
}

function renderSite() {
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
  const link = $(".site-url", root);
  link.textContent = url(s).replace("https://", "");
  link.onclick = (e) => { e.preventDefault(); if (snap.running) open("browser"); };
  // A stopped server can't answer: the address and Browser look and act off.
  link.classList.toggle("off", !snap.running);
  link.title = snap.running ? "" : "Start Anvil to open this address";
  const browser = $('[data-open="browser"]', root);
  browser.disabled = !snap.running;
  browser.title = snap.running ? "Open in browser (⌘O)" : "Start Anvil to open this address";

  const editor = $(".editor-btn", root);
  if (snap.editor) {
    editor.hidden = false;
    // The tile says what it does; which app is in the tooltip, so tiles stay one width.
    editor.title = `Open in ${snap.editor} (⌘E)`;
    $("span", editor).textContent = "Editor";
  }
  for (const b of root.querySelectorAll("[data-open]")) b.onclick = () => open(b.dataset.open);
  for (const el of root.querySelectorAll(".php-only")) el.hidden = process;
  for (const el of root.querySelectorAll(".process-only")) el.hidden = !process;

  for (const b of root.querySelectorAll("[role=tab]")) b.onclick = () => showTab(root, b.dataset.tab);
  $(".tabs", root).onkeydown = (e) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const tabs = [...root.querySelectorAll("[role=tab]")];
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
  const runs = !engine || kind === engine.split(" ")[0] ? engine ?? kind : `${kind} on ${engine}`;
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

  if (process) renderRun(s, root);

  // Settings
  const kindSel = $("#kind", root);
  kindSel.add(new Option(`Detected (${kindLabel(s.kind_detected)})`, ""));
  for (const [id, name] of snap.kinds) kindSel.add(new Option(name, id));
  kindSel.value = s.kind === s.kind_detected ? "" : s.kind;
  kindSel.onchange = () =>
    call("set_app", kindSel.value ? { site: s.name, kind: kindSel.value } : { site: s.name, reset: ["kind"] });

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
  window.renderSiteServices?.(s, root);
  window.renderPackages?.(s, root);
}

/* ---------- Resources ---------- */

const resCache = new Map(); // site name -> resources
const SAFETY = {
  safe: ["Safe to delete", "Nothing is lost"],
  rebuild: ["Rebuildable", "Delete to free space; it can be recreated"],
  keep: ["Keep", "Your code or data"],
};

function bytes(n) {
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

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
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
    dt.append(el("span", null, i.label));
    if (i.path) dt.append(el("small", "path", i.path.startsWith("/") ? tilde(i.path) : i.path));
    const dd = el("dd", "res-cells");
    // Only items you can act on carry a tag; "keep" would be on every row.
    if (i.safety !== "keep") {
      const tag = el("span", `safety ${i.safety}`, SAFETY[i.safety][0]);
      tag.title = i.note;
      dd.append(tag);
    }
    dd.append(el("span", "size", bytes(i.bytes)));
    if (i.safety !== "keep") {
      const del = el("button", "del", "Delete");
      del.setAttribute("aria-label", `Delete ${i.label}`);
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
          selectService(sv.anvil);
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
async function cleanItems(name, items) {
  const total = items.reduce((a, i) => a + i.bytes, 0);
  const what = items.length === 1 ? `${items[0].label} (${items[0].path.startsWith("/") ? tilde(items[0].path) : items[0].path})` : `${items.length} items`;
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

const PHP_KINDS = new Set(["laravel", "statamic", "symfony", "php"]);
const PROCESS_KINDS = new Set(["next", "nuxt", "vite", "node", "rust", "go", "dotnet"]);

function renderRun(s, root) {
  renderRunState(s, root);
  renderRunSettings(s, root);
}

/** Only the run box and its error: safe to redraw while the user types elsewhere. */
function renderRunState(s, root) {
  const state = snap.app_states[s.name]; // "running" | "starting" | undefined
  const error = snap.app_errors[s.name];
  $(".run-box", root).hidden = false;
  $(".run-box .dot", root).className = `dot ${state === "running" ? "on" : state ? "warm" : ""}`;
  const text = $(".state-text", root);
  text.textContent = state === "running" ? "Running" : state ? "Starting…" : "Stopped";
  text.title = state === "running" ? `Port ${s.port}` : state ? "First builds can take a while" : "";
  $('[data-run="start"]', root).hidden = !!state;
  $('[data-run="restart"]', root).hidden = !state;
  $('[data-run="stop"]', root).hidden = !state;
  for (const b of root.querySelectorAll("[data-run]")) {
    b.onclick = () => call("app_run", { site: s.name, action: b.dataset.run });
  }
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
  const rtId = { next: "node", nuxt: "node", vite: "node", node: "node", rust: "rust", go: "go", dotnet: "dotnet" }[s.kind];
  const versions = snap.runtimes?.[rtId] ?? [];
  const rt = $("#app-runtime", root);
  $("#app-runtime-label", root).textContent = `${RUNTIME[s.kind]} version`;
  const autoVersion = s.runtime_pinned ? versions[0] : s.runtime_version;
  rt.add(new Option(autoVersion ? `Automatic (${autoVersion})` : "Automatic", ""));
  for (const v of versions) rt.add(new Option(v, v));
  rt.value = s.runtime_pinned ? s.runtime_version : "";
  rt.title = "Automatic uses .nvmrc or .node-version when the project has one, otherwise the newest installed";
  rt.disabled = versions.length < 2;
  rt.onchange = () => call("set_runtime", { site: s.name, version: rt.value || null });

  const cmd = $("#app-command", root);
  cmd.value = s.command_custom ? s.command : "";
  cmd.placeholder = s.command_custom ? "" : s.command ?? "";
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

function select(name) {
  selected = name;
  localStorage.setItem("selected", name);
  renderList();
  renderSite();
  $(`.item[data-name="${CSS.escape(name)}"]`)?.focus({ preventScroll: false });
}

async function open(target) {
  try {
    await invoke("open", { site: selected, target });
    toast({ browser: "Opened in your browser", finder: "Shown in Finder", root: "Shown in Finder", terminal: "Opened in Terminal", editor: `Opened in ${snap.editor}`, git: `Opened in ${snap.git_client}` }[target]);
  } catch (e) {
    showError(String(e));
  }
}

$("#power").onclick = async (e) => {
  const btn = e.currentTarget;
  btn.disabled = true;
  await call("set_running", { on: !snap.running });
  btn.disabled = false;
};

$("#open-settings").onclick = () => invoke("show_settings");

/* ---------- New application ---------- */

const TEMPLATES = [
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
];
let newKind = localStorage.getItem("newKind") ?? "laravel";

function openNewApp() {
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
  setBusy(false);
  dlg.showModal();
  name.focus();
}

function setBusy(busy) {
  $("#new-create").disabled = busy;
  $("#new-create").textContent = busy ? "Creating…" : "Create";
  for (const i of $("#new-form").querySelectorAll("input, select")) i.disabled = busy;
}

$("#new-cancel").onclick = () => $("#new-app").close();
$("#new-log").onclick = () => invoke("open_create_log", { name: $("#new-name").value });
$("#new-form").onsubmit = async (e) => {
  e.preventDefault();
  const name = $("#new-name").value.trim();
  const parent = $("#new-parent").value;
  if (!name || !parent) return;
  setBusy(true);
  $("#new-log").hidden = false;
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
  if (mode === "services") return openAddService();
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
      M.MenuItem.new({
        text: "Add Application Folder…",
        action: async () => {
          const path = await pick("Choose a project folder");
          if (!path) return;
          try {
            const name = await invoke("link_folder", { path });
            await call("state");
            select(name);
            toast(`Added ${name}.${snap.tld}`);
          } catch (e) {
            showError(String(e));
          }
        },
      }),
    ]),
  });
  menu.popup();
};

/** Filter menu: the kinds present in the list, with counts, plus running only. */
$("#filter").onclick = async () => {
  const apps = mode === "apps";
  const f = filterOf(mode);
  const items = apps ? snap.sites.filter((s) => !s.linked) : services;
  const counts = new Map();
  for (const x of items) counts.set(x.kind, (counts.get(x.kind) ?? 0) + 1);
  const label = (k) => (apps ? kindLabel(k) : services.find((x) => x.kind === k).name);
  const set = (change) => {
    Object.assign(f, change);
    localStorage.setItem("filters", JSON.stringify(filters));
    renderFilter();
    renderMain();
  };
  const sep = () => M.PredefinedMenuItem.new({ item: "Separator" });
  const kinds = [...counts.keys()].sort((a, b) => label(a).localeCompare(label(b)));
  const menu = await M.Menu.new({
    items: await Promise.all([
      M.CheckMenuItem.new({ text: "All Types", checked: !f.kind, action: () => set({ kind: null }) }),
      sep(),
      ...kinds.map((k) =>
        M.CheckMenuItem.new({ text: `${label(k)} (${counts.get(k)})`, checked: f.kind === k, action: () => set({ kind: k }) })),
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
  btn.title = on ? `Filtered: ${[f.kind && "one type", f.running && "running only"].filter(Boolean).join(", ")}` : `Filter ${what}`;
  btn.setAttribute("aria-label", btn.title);
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
  if (!e.target.closest("input, textarea")) e.preventDefault();
});

$("#search").oninput = (e) => {
  filter = e.target.value.trim();
  if (mode === "services") renderServiceList();
  else renderList();
};

// Arrow keys move through the list like a native outline: up/down to move, left/right to fold.
$("#list").onkeydown = (e) => {
  if (mode === "services") return serviceKeys(e);
  const cur = $(`.item[data-name="${CSS.escape(selected ?? "")}"]`);
  if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && cur?.hasAttribute("aria-expanded")) {
    e.preventDefault();
    toggle(selected, e.key === "ArrowRight");
    return;
  }
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const items = [...document.querySelectorAll(".item")].filter((b) => !b.closest(".closed"));
  const i = items.findIndex((b) => b.dataset.name === selected);
  const next = items[i + (e.key === "ArrowDown" ? 1 : -1)];
  if (next) select(next.dataset.name);
};

$("#error-close").onclick = () => showError(null);

// Keyboard: ⌘1–9 tabs in order, ⌘O browser, ⌘E editor, ⌘T terminal, ⌘G git app, ⌘L log.
/* ---------- Shortcuts: the menu bar and the keyboard run the same actions ---------- */

const TAB_KEYS = { 1: "overview", 2: "git", 3: "logs", 4: "packages", 5: "resources", 6: "settings" };
let lastAction = { name: "", at: 0 };

/** Run a menu action. The same key can reach both the page and the menu bar, so
 * a repeat within 300 ms is ignored. Returns whether it did something. */
function runAction(name) {
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
    if (mode === "services") openAddService();
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
  const typing = e.target.closest("input, textarea, select");
  const name = TAB_KEYS[e.key]
    ? `tab:${TAB_KEYS[e.key]}`
    : { o: "open:browser", e: "open:editor", t: "open:terminal", g: "open:git", l: "logs", n: "new", r: "reload", f: "find", ",": "settings" }[e.key];
  if (!name || (typing && !["find", "reload", "settings", "new"].includes(name))) return;
  if (runAction(name)) e.preventDefault();
});

window.refresh();
