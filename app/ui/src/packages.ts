import { $, ago, el, invoke, renderMain, seg, selected, showError, snap } from "./app";

/* Packages tab: an app's dependencies from its manifests and lock files, a tab per
 * package manager, with newer versions and known vulnerabilities inline. Scripts live
 * in the Overview's Commands. */

const pkgCache = new Map<string, any>(); // site name -> packages or { error }
// site name -> { list, at } once checked, "checking" on its way, or { error, at }
export const pkgUpdates = new Map<string, any>();
// site name -> ecosystem id -> { list, at }, "checking", or { list?, at?, error }
export const pkgAudits = new Map<string, Record<string, any>>();
// Ecosystems whose package manager has an audit Anvil runs.
const AUDITED = new Set(["composer", "npm"]);
let pkgQuery = ""; // one search over every list
export const pkgView = new Map<string, string>(); // site name -> "all" or "outdated"
const pkgEco = new Map<string, string>(); // site name -> ecosystem shown
const REGISTRY = {
  composer: "https://packagist.org/packages/",
  npm: "https://www.npmjs.com/package/",
  cargo: "https://crates.io/crates/",
  go: "https://pkg.go.dev/",
  nuget: "https://www.nuget.org/packages/",
};
const REFRESH = `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v3h-3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

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

/** The last check, kept across restarts; the registries are only asked from the refresh button. */
function updatesOf(name) {
  if (!pkgUpdates.has(name)) {
    try {
      const saved = JSON.parse(localStorage.getItem(`pkgUpdates:${name}`) ?? "null");
      if (saved?.list) pkgUpdates.set(name, saved);
    } catch {}
  }
  return pkgUpdates.get(name);
}

async function checkUpdates(name) {
  const prev = updatesOf(name);
  pkgUpdates.set(name, "checking");
  drawPackages(name);
  try {
    const up = { list: await invoke("package_updates", { site: name }), at: Date.now() };
    pkgUpdates.set(name, up);
    localStorage.setItem(`pkgUpdates:${name}`, JSON.stringify(up));
  } catch (e) {
    // A failed check keeps what the last one found.
    pkgUpdates.set(name, { list: prev?.list, at: prev?.at, error: String(e) });
  }
  if (selected === name) drawPackages(name);
}

/** The last security check per ecosystem, kept across restarts like the update check. */
function auditsOf(name) {
  if (!pkgAudits.has(name)) {
    let saved = {};
    try {
      saved = JSON.parse(localStorage.getItem(`pkgAudits:${name}`) ?? "{}") ?? {};
    } catch {}
    pkgAudits.set(name, saved);
  }
  return pkgAudits.get(name)!;
}

async function checkAudit(name, eco) {
  const all = auditsOf(name);
  const prev = all[eco];
  all[eco] = "checking";
  drawPackages(name);
  try {
    all[eco] = { list: await invoke("package_audit", { site: name, ecosystem: eco }), at: Date.now() };
  } catch (e) {
    // A failed check keeps what the last one found.
    all[eco] = { list: prev?.list, at: prev?.at, error: String(e) };
  }
  const done = Object.fromEntries(Object.entries(all).filter(([, v]) => v !== "checking"));
  localStorage.setItem(`pkgAudits:${name}`, JSON.stringify(done));
  if (selected === name) drawPackages(name);
}

/** The manager's name for a section: Composer, or pnpm/npm/yarn/bun for npm packages. */
const managerLabel = (e) => (e.id === "npm" && e.manager ? e.manager.split(" ")[0] : e.label);

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

  const up = updatesOf(name);
  const checked = !!up?.list;
  const updates = up?.list ?? [];
  // Outdated once a check exists; until then All.
  const view = pkgView.get(name) ?? (up?.list ? "outdated" : "all");
  const newer = new Map(updates.map((u) => [`${u.ecosystem}/${u.name}`, u]));
  const needle = pkgQuery.trim().toLowerCase();
  const rowsOf = (e, v = view) =>
    e.packages
      .filter((x) => (!needle || x.name.toLowerCase().includes(needle)) && (v !== "outdated" || newer.has(`${e.id}/${x.name}`)))
      .sort((a, b) => a.name.localeCompare(b.name));
  const eco = p.ecosystems.find((e) => e.id === pkgEco.get(name)) ?? p.ecosystems[0];

  // Search, Outdated · All, the update check and when it ran; then a tab per ecosystem.
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
  const outdated = rowsOf(eco, "outdated").length;
  const filter = seg(
    "Show",
    [
      ["outdated", checked && outdated ? `Outdated ${outdated}` : "Outdated"],
      ["all", "All"],
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
  // Never checked, Outdated says so with this button in its place.
  const unchecked = view === "outdated" && !needle && !up?.list;
  if (unchecked) tools.append(q, filter, updatesStatus(up));
  else tools.append(q, filter, check, updatesStatus(up));
  body.append(tools);

  if (p.ecosystems.length > 1) {
    const tabs = seg(
      "Package manager",
      p.ecosystems.map((e) => [e.id, `${managerLabel(e)} ${rowsOf(e, checked ? view : "all").length}`]),
      eco.id,
      (id) => {
        pkgEco.set(name, id);
        drawPackages(name);
      },
    );
    tabs.classList.add("pkg-ecos");
    for (const b of tabs.children) b.title = p.ecosystems.find((e) => e.id === b.dataset.id).manager ?? "";
    body.append(tabs);
  }

  if (AUDITED.has(eco.id)) body.append(auditSection(name, eco, needle));

  const rows = rowsOf(eco);
  if (rows.length) {
    const list = el("dl", newer.size ? "group pkg-list updates" : "group pkg-list");
    for (const x of rows) list.append(pkgRow(eco.id, x, newer.get(`${eco.id}/${x.name}`)));
    body.append(list);
  } else if (unchecked) {
    const none = el("p", "muted pkg-none", "Not checked yet");
    none.append(check);
    body.append(none);
  } else {
    body.append(
      el(
        "p",
        "muted pkg-none",
        needle ? `Nothing matches “${pkgQuery.trim()}”.` : view === "outdated" ? "Everything is up to date." : "None.",
      ),
    );
  }
  if (focused) {
    const again = $(".pkg-search", body);
    again.focus();
    again.setSelectionRange(again.value.length, again.value.length);
  }
}

/** "checked 5 min ago" next to the refresh button; what it found is in the counts. */
function updatesStatus(up) {
  const s = el("span", "pkg-status muted");
  if (!up) return s;
  if (up === "checking") s.textContent = "Checking…";
  else if (up.error) {
    s.classList.add("error");
    s.textContent = up.error;
  } else s.textContent = `checked ${ago(new Date(up.at))}`;
  return s;
}

/** "Check security", what the last check found and when; its advisories, most severe first. */
function auditSection(name, eco, needle) {
  const a = auditsOf(name)[eco.id];
  const box = el("div", "pkg-audit");
  const head = el("p", "pkg-audit-head");
  const check = el("button", "inline-action pkg-audit-check", a === "checking" ? "Checking…" : "Check security");
  check.disabled = a === "checking";
  check.onclick = () => checkAudit(name, eco.id);
  const status = el("span", "pkg-status muted");
  if (a?.error) {
    status.classList.add("error");
    status.textContent = a.error;
  } else if (a?.list) {
    const n = a.list.length;
    const found = n ? `${n} known ${n === 1 ? "vulnerability" : "vulnerabilities"}` : "No known vulnerabilities";
    status.textContent = `${found} · checked ${ago(new Date(a.at))}`;
  }
  head.append(check, status);
  box.append(head);
  const list = (a?.list ?? []).filter((v) => !needle || v.package.toLowerCase().includes(needle));
  if (list.length) {
    const dl = el("dl", "group pkg-list pkg-audit-list");
    for (const v of list) dl.append(auditRow(v));
    box.append(dl);
  }
  return box;
}

function auditRow(v) {
  const row = el("div", "row");
  const dt = el("dt");
  const title = el(v.url ? "a" : "span", null, v.title || "Untitled advisory");
  if (v.url) {
    title.href = v.url;
    title.onclick = (ev) => {
      ev.preventDefault();
      invoke("open_url", { url: v.url }).catch((err) => showError(String(err)));
    };
  }
  dt.append(title, el("span", "pkg-dev", v.version ? `${v.package} ${v.version}` : v.package));
  row.append(dt, el("dd", "pkg-ver", v.fixed ? `${v.severity} · fixed in ${v.fixed}` : v.severity));
  return row;
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
