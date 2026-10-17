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
const url = (s) => `https://${s.name}.${snap.tld}:${snap.https_port}`;

async function call(cmd, args) {
  try {
    snap = (await invoke(cmd, args)) ?? snap;
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
function toast(msg) {
  const t = $("#toast");
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), 2400);
}

window.refresh = () => {
  call("state");
  window.loadServices?.();
};

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

function row(s, { child = false, kids = 0 } = {}) {
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
  $(".sub", text).textContent = child ? "Linked domain" : tilde(s.path) + "/";

  const end = document.createElement("span");
  end.className = "end";
  const state = snap.app_states[s.name];
  if (state) {
    const dot = document.createElement("span");
    dot.className = `dot ${state === "running" ? "on" : "warm"}`;
    dot.title = state === "running" ? "Running" : "Starting";
    end.append(dot);
  }
  if (!child && s.kind !== "static") {
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
  b.append(mark(s), text, end);
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

function renderList() {
  const list = $("#list");
  list.replaceChildren();
  const note = filterNote("apps", kindLabel);
  if (note && snap.sites.length) list.append(note);

  if (!snap.sites.length) {
    const p = el("div", "empty", "No applications yet. Create one, or add the folder where your projects live and each of them becomes an application.");
    const actions = el("div", "empty-actions");
    const create = el("button", null, "New Application…");
    create.onclick = openNewApp;
    const park = el("button", null, "Add Folder of Applications…");
    park.onclick = async () => {
      const path = await window.__TAURI__.dialog.open({ directory: true, title: "Every project inside this folder becomes an application" });
      if (path) await call("park_folder", { path });
    };
    actions.append(create, park);
    p.append(actions);
    list.append(p);
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

  const section = (title, sites, withChildren) => {
    const rows = [];
    for (const s of [...sites].sort(order)) {
      const kids = withChildren ? byPath.get(s.path) ?? [] : [];
      const parentHit = hit(s);
      const shownKids = kids.filter((k) => parentHit || hit(k));
      if (!parentHit && !shownKids.length) continue;
      rows.push(row(s, { kids: kids.length }));
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
    const h = document.createElement("h2");
    h.textContent = title;
    const box = document.createElement("div");
    box.className = "rows";
    box.append(...rows);
    list.append(h, box);
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
    section(tilde(dir), parked.filter((s) => s.parked_in === dir && !grouped.has(s.name)), true);
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
    action: () => call("remove_site", { site: s.name }),
  }));
  (await M.Menu.new({ items })).popup();
}

function setSort(by) {
  sortBy = by;
  localStorage.setItem("sortBy", by);
  renderList();
}

/** Inline text prompt under the search field. Resolves to the text, or null when cancelled. */
function ask(label, placeholder) {
  const form = $("#ask");
  const input = $("#ask-input");
  $("#ask-label").textContent = label;
  input.placeholder = placeholder;
  input.value = "";
  form.hidden = false;
  input.focus();
  return new Promise((resolve) => {
    const done = (value) => {
      form.hidden = true;
      form.onsubmit = input.onkeydown = input.onblur = null;
      resolve(value);
    };
    form.onsubmit = (e) => {
      e.preventDefault();
      done(input.value.trim() || null);
    };
    input.onkeydown = (e) => e.key === "Escape" && done(null);
    input.onblur = () => done(null);
  });
}

async function addAlias(s) {
  const alias = await ask(`New domain for ${s.name}.${snap.tld}`, `e.g. admin.${s.name}`);
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
  for (const b of root.querySelectorAll("[role=tab]")) b.setAttribute("aria-selected", b.dataset.tab === name);
  for (const p of root.querySelectorAll("[role=tabpanel]")) p.hidden = p.dataset.panel !== name;
  $(".res-reload", root).hidden = name !== "resources";
  window.logsTab?.(root, name === "logs");
}

function renderSite() {
  const root = $("#site");
  const s = snap.sites.find((x) => x.name === selected);
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
  link.onclick = (e) => { e.preventDefault(); open("browser"); };

  const editor = $(".editor-btn", root);
  if (snap.editor) {
    editor.hidden = false;
    editor.title = `Open in ${snap.editor}`;
    $("span", editor).textContent = snap.editor;
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

  // Overview
  $('[data-field="kind"]', root).textContent =
    s.kind === s.kind_detected ? kindLabel(s.kind) : `${kindLabel(s.kind)} (set by you)`;
  $('[data-field="runtime"]', root).textContent = process
    ? `${RUNTIME[s.kind]}${s.runtime_version && s.kind !== "dotnet" && s.kind !== "rust" ? ` ${s.runtime_version}` : ""}${s.runtime_version ? "" : " (not installed)"}`
    : s.php
      ? `PHP ${s.php}${s.php_pinned ? "" : " (default)"}`
      : "Static files";
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
    input.placeholder = snap.ini[key] ?? base[key];
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
  loadResources(s.name);
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

function renderOverviewResources(r) {
  const disk = $('#site [data-field="disk"]');
  const svc = $('#site [data-field="services"]');
  if (!disk || !r || r.error) return;
  const safe = r.items.filter((i) => i.safety === "safe").reduce((a, i) => a + i.bytes, 0);
  disk.textContent = safe ? `${bytes(r.total)}, ${bytes(safe)} can be cleaned up` : bytes(r.total);
  disk.classList.remove("muted");
  const src = { anvil: "Anvil", docker: "Docker", local: "local", remote: "remote", none: "not running" };
  svc.textContent = r.services.length
    ? r.services.map((x) => (x.port || x.host ? `${x.label} (${src[x.source]})` : x.label)).join(", ")
    : "None found in .env";
  svc.classList.toggle("muted", !r.services.length);
}

function renderResources(name) {
  const body = $("#site .res-body");
  if (!body) return;
  const r = resCache.get(name);
  renderOverviewResources(r);
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
    const tag = el("span", `safety ${i.safety}`, SAFETY[i.safety][0]);
    tag.title = i.note;
    dd.append(tag, el("span", "size", bytes(i.bytes)));
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

const PROCESS_KINDS = new Set(["next", "nuxt", "vite", "node", "rust", "go", "dotnet"]);

function renderRun(s, root) {
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

// While an app is starting, check back until its port answers.
setInterval(() => {
  if (snap && Object.values(snap.app_states).includes("starting")) call("state");
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
    toast({ browser: "Opened in your browser", finder: "Shown in Finder", terminal: "Opened in Terminal", editor: `Opened in ${snap.editor}`, git: `Opened in ${snap.git_client}` }[target]);
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
  ["php", "PHP", "Plain PHP"],
  ["static", "Static", "HTML"],
  ["next", "Next.js", "Node.js"],
  ["nuxt", "Nuxt", "Node.js"],
  ["vite", "Vite", "React"],
  ["node", "Node.js", "Server"],
  ["rust", "Rust", "Cargo"],
  ["go", "Go", "net/http"],
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
  const preview = () => ($("#new-url").textContent = name.value ? `${name.value}.${snap.tld}` : "");
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
  const items = [...document.querySelectorAll(".item")].filter((b) => !b.closest(".kids.closed"));
  const i = items.findIndex((b) => b.dataset.name === selected);
  const next = items[i + (e.key === "ArrowDown" ? 1 : -1)];
  if (next) select(next.dataset.name);
};

$("#error-close").onclick = () => showError(null);

// Keyboard: ⌘1–9 tabs in order, ⌘O browser, ⌘E editor, ⌘T terminal, ⌘G git app, ⌘L log.
document.addEventListener("keydown", (e) => {
  if (!e.metaKey || e.shiftKey || e.altKey || e.target.closest("input, textarea, select")) {
    if (!(e.metaKey && ["f", ",", "r"].includes(e.key))) return;
  }
  const root = $("#site");
  const tabKey = /^[1-9]$/.test(e.key) && root.querySelectorAll(".tabs [role=tab]")[e.key - 1];
  if (tabKey) {
    e.preventDefault();
    showTab(root, tabKey.dataset.tab);
    return;
  }
  const action = { o: "browser", e: "editor", t: "terminal", g: "git" }[e.key];
  const off = { editor: !snap.editor, git: $(".git-btn", root)?.hidden !== false }[action];
  if (action && mode === "apps" && selected && !off) {
    e.preventDefault();
    open(action);
    return;
  }
  // ⌘L: the Logs tab for an app, the log file for a service.
  if (e.key === "l") {
    const logs = $('.tabs [data-tab="logs"]', root);
    const svcLog = $(".show-log", root);
    if (logs || svcLog) {
      e.preventDefault();
      if (logs) showTab(root, "logs");
      else svcLog.click();
      return;
    }
  }
  if (e.key === "n") {
    e.preventDefault();
    if (mode === "services") openAddService();
    else openNewApp();
    return;
  }
  if (e.metaKey && e.key === ",") {
    e.preventDefault();
    invoke("show_settings");
  }
  if (e.metaKey && e.key === "r") {
    e.preventDefault();
    $("#reload").click();
  }
  if (e.metaKey && e.key === "f") {
    e.preventDefault();
    $("#search").focus();
  }
});

window.refresh();
