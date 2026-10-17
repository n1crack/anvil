/* Packages tab: an app's dependencies from its manifests and lock files.
 * Shares $, el, invoke, selected and showError with app.js. */

const pkgCache = new Map(); // site name -> packages or { error }
const pkgUpdates = new Map(); // site name -> updates, "checking" or { error }
let pkgQuery = ""; // one search over every list
const pkgView = new Map(); // site name -> the list shown: deps, dev, scripts or updates
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


function drawPackages(name) {
  const body = $("#site .pkg-body");
  if (!body) return;
  const p = pkgCache.get(name);
  const focused = document.activeElement?.classList.contains("pkg-search");
  // A picked tab is redrawn; focus goes back to the selected tab of its row.
  const tabRow = document.activeElement?.closest(".pkg-seg")?.getAttribute("aria-label");
  body.replaceChildren();
  if (!p) return body.append(el("p", "muted", "Reading…"));
  if (p.error) return body.append(el("p", "error", p.error));
  if (!p.ecosystems.length) {
    return body.append(el("p", "muted", "No composer.json, package.json, Cargo.toml, go.mod or .csproj in this folder."));
  }

  // What the app is built on, as one quiet line: Laravel 13.12 · Vite 8.0 · PHP ^8.3.
  const facts = [...p.highlights, ...p.ecosystems.flatMap((e) => e.facts)];
  if (facts.length) {
    const line = el("p", "pkg-summary");
    facts.forEach(([label, v], i) => {
      if (i) line.append(el("span", "pkg-dot", "·"));
      line.append(el("span", null, label));
      if (v) line.append(el("b", null, v));
    });
    body.append(line);
  }

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
  drawLists(body, name, p.ecosystems, updates);
  if (tabRow) $(`.pkg-seg[aria-label="${tabRow}"] [aria-selected=true]`, body)?.focus();
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

/** One list for every package manager, filtered by kind: dependencies,
 * development packages, scripts, or (after a check) what has updates. */
function drawLists(body, name, ecosystems, updates) {
  const needle = pkgQuery.trim().toLowerCase();
  const many = ecosystems.length > 1;
  const newer = new Map(updates.map((u) => [`${u.ecosystem}/${u.name}`, u]));
  const pkgs = ecosystems.flatMap((e) => e.packages.map((x) => ({ ...x, eco: e })));
  const hit = (x) => !needle || x.name.toLowerCase().includes(needle);
  const parts = [
    ["deps", "Dependencies", pkgs.filter((x) => !x.dev && hit(x))],
    ["dev", "Development", pkgs.filter((x) => x.dev && hit(x))],
    ["scripts", "Scripts", ecosystems.flatMap((e) => e.scripts.map(([n, cmd]) => ({ name: n, cmd, eco: e })))
      .filter((x) => !needle || `${x.name} ${x.cmd}`.toLowerCase().includes(needle))],
  ];
  if (updates.length) parts.push(["updates", "Updates", pkgs.filter((x) => newer.has(`${x.eco.id}/${x.name}`) && hit(x))]);
  const shown = parts.filter(([id, , rows]) => rows.length || id === "deps");
  const part = shown.find(([id]) => id === pkgView.get(name)) ?? shown.find(([, , rows]) => rows.length) ?? shown[0];

  const seg = el("div", "pkg-seg");
  seg.setAttribute("role", "tablist");
  seg.setAttribute("aria-label", "Show");
  for (const [id, label, rows] of shown) {
    const b = el("button", "chip-btn", label);
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", id === part[0]);
    b.append(el("span", "pkg-count", rows.length));
    b.onclick = () => {
      pkgView.set(name, id);
      drawPackages(name);
    };
    seg.append(b);
  }
  body.append(seg);

  const [id, , rows] = part;
  if (!rows.length) {
    return body.append(el("p", "muted pkg-none", needle ? `Nothing matches “${pkgQuery.trim()}”.` : "None."));
  }
  // The manager is only worth a word when there is more than one.
  const tag = (e) => (many ? el("span", "pkg-eco", e.manager && e.id === "npm" ? e.manager : e.id) : "");
  const list = el("dl", `group res-list ${id === "scripts" ? "pkg-scripts" : "pkg-list"}`);
  for (const x of rows) {
    if (id === "scripts") {
      const row = el("div", "row");
      const dt = el("dt", null, x.name);
      dt.append(tag(x.eco));
      row.append(dt, el("dd", null, x.cmd));
      list.append(row);
    } else {
      const row = pkgRow(x.eco.id, x, newer.get(`${x.eco.id}/${x.name}`));
      $("dt", row).append(tag(x.eco));
      list.append(row);
    }
  }
  body.append(list);
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
