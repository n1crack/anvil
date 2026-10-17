import { $, ago, el, invoke, renderMain, seg, selected, showError, snap, tab } from "./app";
import { late } from "./late";

/* Packages tab: an app's dependencies from its manifests and lock files, one list per
 * package manager, with newer versions inline. Scripts live in the Overview's Commands. */

export const pkgCache = new Map<string, any>(); // site name -> packages or { error }
// site name -> { list, at } once checked, "checking" on its way, or { error, at }
export const pkgUpdates = new Map<string, any>();
export const UPDATES_TTL = 60 * 60 * 1000; // the registries are asked again after an hour
export let pkgQuery = ""; // one search over every list
export const pkgView = new Map<string, string>(); // site name -> "all" or "outdated"
export const REGISTRY = {
  composer: "https://packagist.org/packages/",
  npm: "https://www.npmjs.com/package/",
  cargo: "https://crates.io/crates/",
  go: "https://pkg.go.dev/",
  nuget: "https://www.nuget.org/packages/",
};
export const REFRESH = `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v3h-3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

window.renderPackages = async (s) => {
  drawPackages(s.name);
  if (tab === "packages") packagesTab(s.name);
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

/** The tab was opened: check for updates unless that happened in the last hour. */
export function packagesTab(name) {
  const up = pkgUpdates.get(name);
  if (up === "checking" || (up && Date.now() - up.at < UPDATES_TTL)) return;
  checkUpdates(name);
}
late.packagesTab = packagesTab;

export async function checkUpdates(name) {
  pkgUpdates.set(name, "checking");
  drawPackages(name);
  try {
    pkgUpdates.set(name, { list: await invoke("package_updates", { site: name }), at: Date.now() });
  } catch (e) {
    pkgUpdates.set(name, { error: String(e), at: Date.now() });
  }
  if (selected === name) drawPackages(name);
}

/** The manager's name for a section: Composer, or pnpm/npm/yarn/bun for npm packages. */
export const managerLabel = (e) => (e.id === "npm" && e.manager ? e.manager.split(" ")[0] : e.label);

export function drawPackages(name) {
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

  // What the app is built on, as one quiet line: Laravel 13.12 · Vite 8.0 · PHP ^8.3.
  const facts = [...p.highlights, ...p.ecosystems.flatMap((e) => e.facts)];
  if (facts.length) {
    const line = el("p", "pkg-summary");
    facts.forEach(([label, v], i) => {
      if (i) line.append(el("span", "pkg-dot", "·"));
      const f = el("span", "pkg-fact", label);
      if (v) f.append(el("b", null, v));
      line.append(f);
    });
    body.append(line);
  }

  const up = pkgUpdates.get(name);
  const updates = up?.list ?? [];
  const view = pkgView.get(name) ?? "all";

  // Search, All · Outdated, and the update check with what it found.
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
  const filter = seg(
    "Show",
    [
      ["all", "All"],
      ["outdated", updates.length ? `Outdated ${updates.length}` : "Outdated"],
    ],
    view,
    (id) => {
      pkgView.set(name, id);
      drawPackages(name);
    },
  );
  const check = el("button", `icon-btn pkg-check${up === "checking" ? " spinning" : ""}`);
  check.innerHTML = REFRESH;
  check.title = "Check for updates";
  check.setAttribute("aria-label", "Check for updates");
  check.disabled = up === "checking";
  check.onclick = () => checkUpdates(name);
  tools.append(q, filter, check, updatesStatus(up));
  body.append(tools);

  drawLists(body, name, p.ecosystems, updates, view);
  if (focused) {
    const again = $(".pkg-search", body);
    again.focus();
    again.setSelectionRange(again.value.length, again.value.length);
  }
}

/** "7 updates · 3 major · checked 5 min ago", next to the refresh button. */
export function updatesStatus(up) {
  const s = el("span", "pkg-status muted");
  if (!up) return s;
  if (up === "checking") s.textContent = "Checking for updates…";
  else if (up.error) {
    s.classList.add("error");
    s.textContent = up.error;
  } else {
    const n = up.list.length;
    const major = up.list.filter((u) => u.level === "major").length;
    s.textContent = [
      n ? `${n} ${n === 1 ? "update" : "updates"}` : "Up to date",
      major && `${major} major`,
      `checked ${ago(new Date(up.at))}`,
    ]
      .filter(Boolean)
      .join(" · ");
  }
  return s;
}

/** One section per package manager, its packages by name; development ones tagged "dev". */
export function drawLists(body, name, ecosystems, updates, view) {
  const needle = pkgQuery.trim().toLowerCase();
  const newer = new Map(updates.map((u) => [`${u.ecosystem}/${u.name}`, u]));
  let shown = 0;
  for (const e of ecosystems) {
    const rows = e.packages
      .filter((x) => (!needle || x.name.toLowerCase().includes(needle)) && (view !== "outdated" || newer.has(`${e.id}/${x.name}`)))
      .sort((a, b) => a.name.localeCompare(b.name));
    if (!rows.length) continue;
    shown += rows.length;
    const list = el("dl", newer.size ? "group pkg-list updates" : "group pkg-list");
    const head = el("div", "pkg-head");
    head.append(el("span", null, managerLabel(e)));
    const version = e.manager?.split(" ")[1];
    if (version) head.append(el("span", "pkg-head-ver", version));
    head.append(el("span", "pkg-head-count", rows.length));
    list.append(head);
    for (const x of rows) list.append(pkgRow(e.id, x, newer.get(`${e.id}/${x.name}`)));
    body.append(list);
  }
  if (shown) return;
  const text = needle
    ? `Nothing matches “${pkgQuery.trim()}”.`
    : view !== "outdated"
      ? "None."
      : pkgUpdates.get(name) === "checking"
        ? "Checking for updates…"
        : pkgUpdates.get(name)?.list
          ? "Everything is up to date."
          : "Not checked for updates yet.";
  body.append(el("p", "muted pkg-none", text));
}

export function pkgRow(eco, x, update) {
  const row = el("div", "row");
  const dt = el("dt");
  const a = el("a", null, x.name);
  a.href = REGISTRY[eco] + x.name;
  a.onclick = (ev) => {
    ev.preventDefault();
    invoke("open_url", { url: a.href }).catch((err) => showError(String(err)));
  };
  dt.append(a);
  if (x.dev) dt.append(el("span", "pkg-dev", "dev"));
  if (x.constraint) dt.title = `Wants ${x.constraint}`;
  const dd = el("dd", "pkg-ver");
  const v = el("span", null, x.version ?? "–");
  v.title = x.released ? `Released ${x.released}` : x.version ? "" : "Not in the lock file";
  dd.append(v);
  if (update) {
    const to = el("span", `pkg-new ${update.level}`, update.latest);
    to.title = `${update.level[0].toUpperCase()}${update.level.slice(1)} update`;
    dd.append(el("span", "pkg-arrow", "→"), to);
  }
  row.append(dt, dd);
  return row;
}

// The first snapshot can arrive before this file loads.
if (snap) renderMain();
