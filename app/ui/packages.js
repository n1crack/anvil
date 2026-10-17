/* Packages tab: an app's dependencies from its manifests and lock files.
 * Shares $, el, invoke, selected and showError with app.js. */

const pkgCache = new Map(); // site name -> packages or { error }
const pkgUpdates = new Map(); // site name -> updates, "checking" or { error }
let pkgQuery = ""; // one search over every list
const pkgOpen = new Set(); // "<ecosystem>/dev" and "<ecosystem>/scripts" sections left open
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
  const focused = document.activeElement?.classList.contains("pkg-search");
  body.replaceChildren();
  if (!p) return body.append(el("p", "muted", "Reading…"));
  if (p.error) return body.append(el("p", "error", p.error));
  if (!p.ecosystems.length) {
    return body.append(el("p", "muted", "No composer.json, package.json, Cargo.toml, go.mod or .csproj in this folder."));
  }

  // What the app is built on, at a glance.
  const chips = el("div", "chips pkg-chips");
  for (const [label, v] of p.highlights) chips.append(chip(label, v));
  for (const e of p.ecosystems) for (const [k, v] of e.facts) chips.append(chip(k, v));
  if (chips.children.length) body.append(chips);

  // One search for every list, and the update check.
  const tools = el("div", "pkg-tools");
  const q = el("input", "pkg-search");
  q.type = "search";
  q.placeholder = "Search packages";
  q.spellcheck = false;
  q.value = pkgQuery;
  q.setAttribute("aria-label", "Search packages");
  q.oninput = () => {
    pkgQuery = q.value;
    drawPackages(name);
  };
  tools.append(q, updatesButton(name));
  body.append(tools);
  const up = pkgUpdates.get(name);
  if (up?.error) body.append(el("p", "error", up.error));

  const updates = Array.isArray(up) ? up : [];
  for (const e of p.ecosystems) drawEcosystem(body, e, updates);
  if (focused) {
    const again = $(".pkg-search", body);
    again.focus();
    again.setSelectionRange(again.value.length, again.value.length);
  }
}

function updatesButton(name) {
  const up = pkgUpdates.get(name);
  if (up === "checking") return el("span", "muted", "Checking…");
  const major = Array.isArray(up) ? up.filter((u) => u.level === "major").length : 0;
  const label = !up
    ? "Check for Updates"
    : up.error
      ? "Try Again"
      : up.length
        ? `${up.length} ${up.length === 1 ? "update" : "updates"}${major ? `, ${major} major` : ""}`
        : "Up to date";
  const b = el("button", "pkg-check", label);
  b.title = up && !up.error ? "Check again" : "Ask the registries for newer versions";
  b.onclick = () => checkUpdates(name);
  return b;
}

function drawEcosystem(body, e, updates) {
  const newer = new Map(updates.filter((u) => u.ecosystem === e.id).map((u) => [u.name, u]));
  const needle = pkgQuery.trim().toLowerCase();
  const match = (x) => !needle || x.name.toLowerCase().includes(needle);
  const main = e.packages.filter((x) => !x.dev && match(x));
  const dev = e.packages.filter((x) => x.dev && match(x));
  if (needle && !main.length && !dev.length) return;

  const title = el("h3", "res-sub", e.label);
  if (e.manager && e.id === "npm") title.append(el("span", "muted", ` with ${e.manager}`));
  body.append(title);
  if (main.length) body.append(pkgList(e.id, main, newer));
  else if (!dev.length) body.append(el("p", "muted pkg-none", "No dependencies."));

  // Development packages and scripts stay folded unless asked for (or searched).
  const fold = (key, label, content) => {
    const d = el("details", "pkg-fold");
    d.open = pkgOpen.has(key) || (key.endsWith("/dev") && !!needle);
    d.ontoggle = () => (d.open ? pkgOpen.add(key) : pkgOpen.delete(key));
    d.append(el("summary", null, label), content);
    body.append(d);
  };
  if (dev.length) fold(`${e.id}/dev`, `Development (${dev.length})`, pkgList(e.id, dev, newer));
  if (e.scripts.length && !needle) {
    const list = el("dl", "group res-list pkg-scripts");
    for (const [script, cmd] of e.scripts) {
      const row = el("div", "row");
      const dt = el("dt", null, script);
      dt.title = cmd;
      row.append(dt);
      list.append(row);
    }
    fold(`${e.id}/scripts`, `Scripts (${e.scripts.length})`, list);
  }
}

function pkgList(eco, rows, newer) {
  const list = el("dl", "group res-list pkg-list");
  for (const x of rows) list.append(pkgRow(eco, x, newer.get(x.name)));
  return list;
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
  if (x.constraint) dt.title = `Wants ${x.constraint}`;
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
