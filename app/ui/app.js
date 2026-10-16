const { invoke } = window.__TAURI__.core;

const $ = (sel, root = document) => root.querySelector(sel);
let snap = null;
let selected = localStorage.getItem("selected");
let filter = "";

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
  const el = $("#error");
  el.textContent = msg ?? "";
  el.hidden = !msg;
}

window.refresh = () => call("state");

function render() {
  showError(snap.error);

  const power = $("#power");
  power.setAttribute("aria-checked", snap.running);
  $("#power-label").textContent = snap.running ? "Running" : "Stopped";

  if (!snap.sites.some((s) => s.name === selected)) selected = snap.sites[0]?.name;
  renderList();
  renderSite();
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

function renderList() {
  const list = $("#list");
  list.replaceChildren();

  if (!snap.sites.length) {
    list.innerHTML = `<p class="empty">No sites yet. Add a folder of projects from Terminal:<code>anvil park ~/Sites</code></p>`;
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
  const hit = (s) => !q || `${s.name}.${snap.tld} ${s.path}`.toLowerCase().includes(q);
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
    p.textContent = `No sites match “${filter}”.`;
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
    items.push(await M.Submenu.new({ text: "Add Site To", items: groupItems }), await sep());
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

function renderSite() {
  const root = $("#site");
  const s = snap.sites.find((x) => x.name === selected);
  if (!s) {
    root.innerHTML = `<div class="placeholder">Select a site</div>`;
    return;
  }

  root.replaceChildren($("#site-tpl").content.cloneNode(true));
  $(".site-name", root).textContent = s.name;
  const link = $(".site-url", root);
  link.textContent = url(s).replace("https://", "");
  link.onclick = (e) => { e.preventDefault(); open("browser"); };

  const editor = $(".editor-btn", root);
  if (snap.editor) {
    editor.hidden = false;
    editor.textContent = `Open in ${snap.editor}`;
  }
  for (const b of root.querySelectorAll("[data-open]")) b.onclick = () => open(b.dataset.open);

  const php = $("#php", root);
  php.add(new Option(`Default (${snap.php_default ?? "none"})`, ""));
  for (const v of snap.php_installed) php.add(new Option(`PHP ${v}`, v));
  php.value = s.php_pinned ? s.php : "";
  php.onchange = () => call("set_php", { site: s.name, version: php.value || null });

  $('[data-field="root"]', root).textContent = tilde(s.root);
  $('[data-field="path"]', root).textContent = tilde(s.path);
  $('[data-field="source"]', root).textContent = s.linked ? "Linked folder" : "Parked folder";

  // Per-site php.ini overrides. Placeholders show what the site gets without one.
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
}

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
  renderList();
};

// Arrow keys move through the list like a native outline: up/down to move, left/right to fold.
$("#list").onkeydown = (e) => {
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

document.addEventListener("keydown", (e) => {
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
