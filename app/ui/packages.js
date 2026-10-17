/* Packages tab: an app's dependencies from its manifests and lock files.
 * Shares $, el, invoke, selected and showError with app.js. */

const pkgCache = new Map(); // site name -> packages or { error }
const pkgUpdates = new Map(); // site name -> updates, "checking" or { error }
const pkgView = {}; // ecosystem id -> { q, dev, all }
let pkgFocus = null; // ecosystem id whose filter has focus, kept across redraws
const REGISTRY = {
  composer: "https://packagist.org/packages/",
  npm: "https://www.npmjs.com/package/",
  cargo: "https://crates.io/crates/",
  go: "https://pkg.go.dev/",
  nuget: "https://www.nuget.org/packages/",
};

window.renderPackages = async (s) => {
  drawPackages(s.name);
  try {
    const p = await invoke("packages", { site: s.name });
    const same = JSON.stringify(p) === JSON.stringify(pkgCache.get(s.name));
    pkgCache.set(s.name, p);
    if (!same && selected === s.name) drawPackages(s.name);
  } catch (e) {
    pkgCache.set(s.name, { error: String(e) });
    if (selected === s.name) drawPackages(s.name);
  }
};

async function checkUpdates(name) {
  pkgUpdates.set(name, "checking");
  drawPackages(name);
  try {
    pkgUpdates.set(name, await invoke("package_updates", { site: name }));
  } catch (e) {
    pkgUpdates.set(name, { error: String(e) });
  }
  if (selected === name) drawPackages(name);
}

function chip(label, value) {
  const c = el("span", "chip");
  c.append(el("b", null, label));
  if (value) c.append(document.createTextNode(` ${value}`));
  return c;
}

function drawPackages(name) {
  const body = $("#site .pkg-body");
  if (!body) return;
  const p = pkgCache.get(name);
  body.replaceChildren();
  if (!p) return body.append(el("p", "muted", "Reading…"));
  if (p.error) return body.append(el("p", "error", p.error));
  if (!p.ecosystems.length) {
    return body.append(el("p", "muted", "No composer.json, package.json, Cargo.toml, go.mod or .csproj in this folder"));
  }

  const chips = el("div", "chips pkg-chips");
  for (const [label, v] of p.highlights) chips.append(chip(label, v));
  for (const e of p.ecosystems) {
    for (const [k, v] of e.facts) chips.append(chip(k, v));
    if (e.id === "npm" && e.manager) chips.append(chip(e.manager));
  }
  body.append(chips);

  const counts = p.ecosystems.map((e) => {
    const dev = e.packages.filter((x) => x.dev).length;
    const lock = e.locked.length ? `, ${e.locked.length} in ${e.lock}` : e.lock ? "" : ", no lock file";
    return `${e.label}: ${e.packages.length - dev} direct, ${dev} dev${lock}`;
  });
  const head = el("div", "res-head pkg-head");
  const summary = el("div", "muted");
  for (const c of counts) summary.append(el("div", null, c));
  head.append(summary);
  const up = pkgUpdates.get(name);
  if (up === "checking") {
    head.append(el("span", "muted", "Checking…"));
  } else {
    const major = Array.isArray(up) ? up.filter((u) => u.level === "major").length : 0;
    const label = !up
      ? "Check for updates"
      : up.error
        ? "Try again"
        : up.length
          ? `${up.length} ${up.length === 1 ? "update" : "updates"} available${major ? `, ${major} major` : ""}`
          : "Up to date";
    const b = el("button", "clean-all", label);
    b.title = up && !up.error ? "Check again" : "Ask the registries for newer versions of the direct dependencies";
    b.onclick = () => checkUpdates(name);
    head.append(b);
  }
  body.append(head);
  if (up?.error) body.append(el("p", "error", up.error));

  for (const e of p.ecosystems) drawEcosystem(body, e, Array.isArray(up) ? up : []);
  for (const e of p.ecosystems) drawScripts(body, e);
}

function drawEcosystem(body, e, updates) {
  const view = (pkgView[e.id] ??= { q: "", dev: false, all: false });
  const newer = new Map(updates.filter((u) => u.ecosystem === e.id).map((u) => [u.name, u]));
  body.append(el("h3", "res-sub", e.label));

  const tools = el("div", "pkg-tools");
  const q = el("input");
  q.type = "search";
  q.placeholder = "Filter by name";
  q.spellcheck = false;
  q.value = view.q;
  q.setAttribute("aria-label", `Filter ${e.label} packages`);
  q.onfocus = () => (pkgFocus = e.id);
  q.onblur = () => q.isConnected && (pkgFocus = null);
  q.oninput = () => {
    view.q = q.value;
    fill();
  };
  tools.append(q);

  const devCount = e.packages.filter((x) => x.dev).length;
  const modes = el("div", "modes pkg-modes");
  modes.setAttribute("role", "tablist");
  for (const [dev, label] of [[false, "Dependencies"], [true, "Dev"]]) {
    const b = el("button", null, `${label} ${e.packages.filter((x) => x.dev === dev).length}`);
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", !view.all && view.dev === dev);
    b.onclick = () => {
      view.dev = dev;
      view.all = false;
      drawPackages(selected);
    };
    modes.append(b);
  }
  if (devCount) tools.append(modes);
  body.append(tools);

  const slot = el("div");
  body.append(slot);
  // Lists can run into thousands of rows: draw them a slice per frame.
  const fill = () => {
    const needle = view.q.trim().toLowerCase();
    const rows = (view.all ? e.locked.map(([name, version]) => ({ name, version })) : e.packages.filter((x) => x.dev === view.dev && (devCount || !view.dev)))
      .filter((x) => !needle || x.name.toLowerCase().includes(needle));
    if (!rows.length) {
      slot.replaceChildren(el("p", "muted pkg-none", needle ? "No packages match" : "None"));
      return;
    }
    const list = el("dl", "group res-list pkg-list");
    slot.replaceChildren(list);
    let i = 0;
    const step = () => {
      if (!list.isConnected) return;
      const frag = document.createDocumentFragment();
      for (const end = Math.min(i + 150, rows.length); i < end; i++) frag.append(pkgRow(e.id, rows[i], newer.get(rows[i].name)));
      list.append(frag);
      if (i < rows.length) requestAnimationFrame(step);
    };
    step();
  };
  fill();
  if (pkgFocus === e.id) q.focus();

  if (e.locked.length) {
    const more = el("button", "link pkg-more", view.all ? "Show direct dependencies only" : `Show all ${e.locked.length} locked packages`);
    more.onclick = () => {
      view.all = !view.all;
      drawPackages(selected);
    };
    body.append(more);
  }
}

function drawScripts(body, e) {
  if (e.scripts.length) {
    const tool = e.id === "composer" ? "Composer" : (e.manager ?? "npm").split(" ")[0];
    body.append(el("h3", "res-sub", `${tool} scripts`));
    const list = el("dl", "group res-list pkg-scripts");
    for (const [name, cmd] of e.scripts) {
      const row = el("div", "row");
      row.append(el("dt", null, name), el("dd", "path", cmd));
      list.append(row);
    }
    body.append(list);
  }
}

function pkgRow(eco, x, update) {
  const row = el("div", "row");
  const dt = el("dt");
  const a = el("a", null, x.name);
  a.href = REGISTRY[eco] + x.name;
  a.onclick = (ev) => {
    ev.preventDefault();
    invoke("open_url", { url: a.href }).catch((err) => showError(String(err)));
  };
  dt.append(a);
  if (x.constraint && x.constraint !== x.version) dt.append(el("small", null, x.constraint));
  const dd = el("dd", "res-cells");
  if (update) {
    const tag = el("span", `pkg-new ${update.level}`, update.latest);
    tag.title = `Newer version available (${update.level} update)`;
    dd.append(tag);
  }
  const v = el("span", "size", x.version ?? "–");
  v.title = x.released ? `Released ${x.released}` : x.version ? "" : "Not in the lock file";
  dd.append(v);
  row.append(dt, dd);
  return row;
}

// The first snapshot can arrive before this file loads.
if (snap) renderMain();
