import { $, ago, call, el, invoke, popAt, renderMain, selected, showError, snap } from "./app";
import { icon } from "./icons";
import { showRun } from "./dev";
import { askProcs, drawProcs, loadProcs } from "./processes";

/* Packages tab: the app's key packages as badges (pinned per app, the detected ones by
 * default), then Check for updates and Check security, then a card per package manager
 * with its Install and Update… and what the checks found for it. Scripts live in the
 * Overview's Commands. */

const pkgCache = new Map<string, any>(); // site name -> packages or { error }
// site name -> { list, at, error? } once checked
export const pkgUpdates = new Map<string, any>();
// site name -> ecosystem id -> { list, at, error? }
export const pkgAudits = new Map<string, Record<string, any>>();
// Checks on their way: `name` for updates, `name:eco` for an audit.
const busy = new Set<string>();
// Ecosystems whose package manager has an audit Anvil runs.
const AUDITED = new Set(["composer", "npm"]);
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

/** The last check, kept across restarts; the registries are only asked from the button. */
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
  busy.add(name);
  drawPackages(name);
  try {
    const up = { list: await invoke("package_updates", { site: name }), at: Date.now() };
    pkgUpdates.set(name, up);
    localStorage.setItem(`pkgUpdates:${name}`, JSON.stringify(up));
  } catch (e) {
    // A failed check keeps what the last one found.
    const prev = updatesOf(name);
    pkgUpdates.set(name, { list: prev?.list, at: prev?.at, error: String(e) });
  }
  busy.delete(name);
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
  busy.add(`${name}:${eco}`);
  drawPackages(name);
  try {
    all[eco] = { list: await invoke("package_audit", { site: name, ecosystem: eco }), at: Date.now() };
  } catch (e) {
    // A failed check keeps what the last one found.
    all[eco] = { list: all[eco]?.list, at: all[eco]?.at, error: String(e) };
  }
  busy.delete(`${name}:${eco}`);
  localStorage.setItem(`pkgAudits:${name}`, JSON.stringify(all));
  if (selected === name) drawPackages(name);
}

// site name -> how its "Switch to pnpm" went
const switched = new Map<string, { ok: boolean; text: string }>();

async function switchToPnpm(name) {
  busy.add(`${name}:pnpm`);
  switched.delete(name);
  drawPackages(name);
  try {
    const lock = await invoke("switch_to_pnpm", { site: name });
    switched.set(name, { ok: true, text: `Switched to pnpm: ${lock} is gone and node_modules was installed again. Test the app.` });
  } catch (e) {
    switched.set(name, { ok: false, text: String(e) });
  }
  busy.delete(`${name}:pnpm`);
  const s = snap.sites.find((x) => x.name === name);
  if (s && selected === name) window.renderPackages(s);
}

// "site:composer" or "site:js" -> the Install or Update running (its id), then how it went.
const running = new Map<string, string>();
const ran = new Map<string, { ok: boolean; text: string; id: string }>();

/** Run Install or Update as Dev runs a script, then read the packages (and, when they
 * were checked, the outdated ones) again. */
async function runPkg(name, eco, id, command) {
  const key = `${name}:${eco}`;
  running.set(key, id);
  ran.delete(key);
  drawPackages(name);
  let started = false;
  let result;
  try {
    await askProcs(name, "package_run", { id });
    started = true;
    if (selected === name) drawProcs(name); // Dev lists the run from now on
    const exit = await invoke("package_wait", { site: name, id });
    result = !exit
      ? { ok: false, text: `${command} was stopped.`, id }
      : exit.code === 0
        ? { ok: true, text: `${command} finished.`, id }
        : { ok: false, text: `${command} failed (${exit.code == null ? "ended by a signal" : `exit code ${exit.code}`}).`, id };
  } catch (e) {
    result = { ok: false, text: String(e), id: started ? id : "" };
  }
  await loadProcs(name);
  running.delete(key);
  ran.set(key, result);
  const s = snap.sites.find((x) => x.name === name);
  if (s && selected === name) await window.renderPackages(s);
  if (updatesOf(name)?.at) checkUpdates(name);
}

const output = (name, id) => {
  const b = el("button", "text-act pkg-output", "Output");
  b.onclick = () => showRun(name, id);
  return b;
};

/** A question asked in place: Cancel goes back to `idle`, the action (in red) does it. */
function ask(box, question, verb, idle, go) {
  const no = el("button", "text-act", "Cancel");
  no.onclick = idle;
  const yes = el("button", "text-act danger", verb);
  yes.onclick = go;
  box.classList.add("asking");
  box.replaceChildren(el("span", "muted pkg-ask", question), no, yes);
  no.focus();
}

/** A card's Install and Update…; Update asks first. Running, the header says so; after,
 * how it went under it. Either way its output is a click away in Dev. */
function runActs(name, eco, cmds: [string, string][]) {
  const key = `${name}:${eco}`;
  const acts = el("div", "pkg-acts");
  const idle = () => {
    acts.classList.remove("asking");
    acts.replaceChildren(
      ...cmds.map(([id, command]) => {
        const update = id.endsWith("-update");
        const b = el("button", `text-act ${update ? "pkg-update" : "pkg-install"}`, update ? "Update…" : "Install");
        b.title = command;
        b.onclick = () =>
          update
            ? ask(
                acts,
                `Run ${command}? Packages move to the newest versions their constraints allow, and the lock file changes.`,
                "Update",
                idle,
                () => runPkg(name, eco, id, command),
              )
            : runPkg(name, eco, id, command);
        return b;
      }),
    );
  };
  const going = running.get(key);
  if (going) {
    const command = cmds.find(([id]) => id === going)?.[1] ?? eco;
    acts.replaceChildren(el("span", "spin-dot"), el("span", "muted", `Running ${command}…`), output(name, going));
  } else idle();
  const done = ran.get(key);
  if (!done) return [acts, null];
  const msg = el("p", done.ok ? "muted pkg-ran" : "pkg-error pkg-ran", done.text);
  if (done.id) msg.append(" ", output(name, done.id));
  return [acts, msg];
}

/** The manager's name for a section: Composer, or pnpm/npm/yarn/bun for npm packages. */
const managerLabel = (e) => (e.id === "npm" && e.manager ? e.manager.split(" ")[0] : e.label);

/** A badge's version: a platform requirement, a direct dependency or anything locked. */
function versionOf(p, key) {
  for (const e of p.ecosystems) {
    const fact = e.facts.find(([l]) => l === key);
    if (fact) return fact[1];
    const dep = e.packages.find((x) => x.name === key);
    if (dep) return dep.version ?? dep.constraint;
    const locked = e.locked.find(([n]) => n === key);
    if (locked) return locked[1];
  }
  return null;
}

// The file each manager reads, shown by its name; and the prefix of its Install/Update commands.
const FILE = { composer: "composer.json", npm: "package.json", cargo: "Cargo.toml", go: "go.mod", nuget: ".csproj" };
const RUN = { composer: "composer", npm: "js" };

export function drawPackages(name) {
  const body = $("#site .pkg-body");
  if (!body) return;
  const p = pkgCache.get(name);
  body.replaceChildren();
  if (!p) return body.append(el("p", "muted", "Reading…"));
  if (p.error) return body.append(el("p", "error", p.error));
  if (!p.ecosystems.length) {
    return body.append(el("p", "muted", "No composer.json, package.json, Cargo.toml, go.mod or .csproj in this folder."));
  }
  body.append(pinsLine(name, p));
  body.append(el("p", "muted pkg-hint", "The packages that matter most, with their versions. + adds one, × removes it from this list."));

  // The two checks above the managers; what they find goes into each manager's card.
  const up = updatesOf(name);
  const audits = auditsOf(name);
  const audited = p.ecosystems.filter((e) => AUDITED.has(e.id));
  const bar = el("div", "pkg-checkbar");
  const outdated = el("button", "pkg-btn pkg-check-outdated", busy.has(name) ? "Checking…" : "Check for updates");
  outdated.disabled = busy.has(name);
  outdated.onclick = () => checkUpdates(name);
  bar.append(outdated);
  if (audited.length) {
    const auditing = audited.some((e) => busy.has(`${name}:${e.id}`));
    const security = el("button", "pkg-btn pkg-check-security", auditing ? "Checking…" : "Check security");
    security.disabled = auditing;
    security.onclick = () => audited.forEach((e) => checkAudit(name, e.id));
    bar.append(security);
  }
  body.append(bar);
  // The update check asks every registry at once, so its failure is the app's, not a card's.
  if (up?.error) body.append(el("p", "pkg-error", up.error));
  for (const e of p.ecosystems) body.append(card(name, p, e, up, AUDITED.has(e.id) ? audits[e.id] : null));
}

/** A package manager: its name and file, Install and Update…, what the checks found and,
 * for an npm or yarn app, Switch to pnpm…. */
function card(name, p, e, up, audit) {
  const box = el("section", "group pkg-card");
  box.dataset.eco = e.id;
  const head = el("div", "pkg-head");
  const title = el("h3", "pkg-name", managerLabel(e));
  if (e.manager) title.title = e.manager;
  head.append(title);
  if (FILE[e.id]) head.append(el("span", "muted pkg-file", FILE[e.id]));
  box.append(head);
  const run = RUN[e.id];
  const cmds = run ? p.commands.filter(([id]) => id.startsWith(`pkg-${run}-`)) : [];
  if (cmds.length) {
    const [acts, msg] = runActs(name, run, cmds);
    head.append(acts);
    if (msg) box.append(msg);
  }

  // "3 updates · 1 security advisory · checked 2 min ago"; nothing before a check.
  const updates = (up?.list ?? []).filter((x) => x.ecosystem === e.id);
  const advisories = audit?.list ?? [];
  const found: [any, number][] = [];
  if (up?.at) {
    const u = updates.length;
    found.push([u ? el("span", "pkg-found", `${u} ${u === 1 ? "update" : "updates"}`) : "No updates", up.at]);
  }
  if (audit?.at) {
    const n = advisories.length;
    found.push([n ? el("span", "pkg-found bad", `${n} security ${n === 1 ? "advisory" : "advisories"}`) : "no known advisories", audit.at]);
  }
  if (found.length) {
    const line = el("p", "muted pkg-summary");
    const when = found.map(([, at]) => ago(new Date(at)));
    const apart = new Set(when).size > 1;
    found.forEach(([what], i) => {
      if (i) line.append(" · ");
      line.append(typeof what === "string" && !i ? what[0].toUpperCase() + what.slice(1) : what);
      if (apart) line.append(`, checked ${when[i]}`);
    });
    if (!apart) line.append(` · checked ${when[0]}`);
    box.append(line);
  }
  if (audit?.error) box.append(el("p", "pkg-error", audit.error));
  if (advisories.length) {
    const dl = el("dl", "pkg-adv-list");
    for (const v of advisories) dl.append(auditRow(v));
    box.append(dl);
  }
  if (updates.length) {
    const newer = new Map(updates.map((x) => [x.name, x]));
    const dl = el("dl", "pkg-list updates");
    for (const x of e.packages.filter((x) => newer.has(x.name)).sort((a, b) => a.name.localeCompare(b.name)))
      dl.append(pkgRow(e.id, x, newer.get(x.name)));
    box.append(dl);
  }
  if (e.id === "npm") {
    const foot = switchFoot(name, e);
    if (foot) box.append(foot);
  }
  return box;
}

/** An npm or yarn app can move to pnpm, only when asked and confirmed in the card. */
function switchFoot(name, e) {
  const lock = e.lock === "package-lock.json" || e.lock === "yarn.lock" ? e.lock : null;
  const going = busy.has(`${name}:pnpm`);
  const sw = switched.get(name);
  if (!lock && !going && !sw) return null;
  const foot = el("div", "pkg-foot");
  const line = el("div", "pkg-acts");
  const idle = () => {
    line.classList.remove("asking");
    const b = el("button", "text-act pkg-switch", "Switch to pnpm…");
    b.onclick = () =>
      ask(line, `Replace ${lock} with pnpm-lock.yaml and install node_modules again? Test the app afterwards.`, "Switch", idle, () =>
        switchToPnpm(name),
      );
    line.replaceChildren(b);
  };
  if (going) line.replaceChildren(el("span", "spin-dot"), el("span", "muted", "Switching to pnpm…"));
  else if (lock) idle();
  if (line.childNodes.length) foot.append(line);
  if (sw && !going) foot.append(el("p", sw.ok ? "muted pkg-ran" : "pkg-error pkg-ran", sw.text));
  return foot;
}

/** The key packages as badges: × unpins one, + pins another, Reset brings back the detected ones. */
function pinsLine(name, p) {
  const s = snap.sites.find((x) => x.name === name);
  const defaults = p.defaults.map(([k]) => k);
  const pins: string[] = s?.packages_pinned ?? defaults;
  const labels = new Map<string, string>(p.defaults);
  const save = (next) => call("set_packages_pinned", { site: name, pins: next });
  const line = el("div", "chips pkg-pins");
  for (const k of pins) {
    const label = labels.get(k) ?? k;
    const c = el("span", "chip pkg-pin", label);
    if (label !== k) c.title = k;
    const v = versionOf(p, k);
    if (v) c.append(el("b", null, v));
    const x = el("button");
    x.append(icon("x"));
    x.setAttribute("aria-label", `Unpin ${label}`);
    x.onclick = () => save(pins.filter((o) => o !== k));
    c.append(x);
    line.append(c);
  }
  const add = el("button", "pkg-pin-add");
  add.append(icon("plus"));
  add.title = "Pin a package";
  add.setAttribute("aria-label", "Pin a package");
  add.onclick = () => pinPop(add, p, pins, save);
  line.append(add);
  if (pins.join("\n") !== defaults.join("\n")) {
    const reset = el("button", "pkg-pin-reset", "Reset");
    reset.title = "Back to the detected packages";
    // A worktree's unset is its app's, so its reset is the defaults themselves.
    reset.onclick = () => save(s?.worktree_of ? defaults : null);
    line.append(reset);
  }
  return line;
}

/** The + popover: a search over everything installed, direct dependencies first; ↑↓ and Enter pick. */
function pinPop(anchor, p, pins, save) {
  const all: { key: string; version: string | null; from: string }[] = [];
  const seen = new Set(pins);
  const offer = (key, version, from) => {
    if (seen.has(key)) return;
    seen.add(key);
    all.push({ key, version, from });
  };
  const byName = (a, b) => a.localeCompare(b);
  for (const e of p.ecosystems) for (const [l, v] of e.facts) offer(l, v, managerLabel(e));
  for (const e of p.ecosystems)
    for (const x of [...e.packages].sort((a, b) => byName(a.name, b.name))) offer(x.name, x.version ?? x.constraint, managerLabel(e));
  for (const e of p.ecosystems) for (const [n, v] of [...e.locked].sort((a, b) => byName(a[0], b[0]))) offer(n, v, managerLabel(e));

  const pop = el("div", "pkg-pin-pop");
  const q = el("input");
  q.placeholder = "Search installed packages";
  q.spellcheck = false;
  q.setAttribute("role", "combobox");
  q.setAttribute("aria-label", "Pin a package");
  q.setAttribute("aria-expanded", "true");
  q.setAttribute("aria-autocomplete", "list");
  q.setAttribute("aria-controls", "pkg-pin-list");
  const list = el("div", "pkg-pin-list");
  list.id = "pkg-pin-list";
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-label", "Installed packages");
  let shown: typeof all = [];
  let active = 0;
  const pick = (key) => {
    close();
    save([...pins, key]);
  };
  const mark = () => {
    list.querySelectorAll("[role=option]").forEach((o, i) => o.setAttribute("aria-selected", String(i === active)));
    q.setAttribute("aria-activedescendant", shown.length ? `pkg-opt-${active}` : "");
    list.children[active]?.scrollIntoView?.({ block: "nearest" });
  };
  const draw = () => {
    const needle = q.value.trim().toLowerCase();
    // Names that start with the search first; ponytail: 50 shown, the search narrows the rest.
    shown = all
      .filter((o) => o.key.toLowerCase().includes(needle))
      .sort((a, b) => Number(!a.key.toLowerCase().startsWith(needle)) - Number(!b.key.toLowerCase().startsWith(needle)))
      .slice(0, 50);
    active = 0;
    list.replaceChildren(
      ...shown.map((o, i) => {
        const it = el("div", "pkg-opt");
        it.id = `pkg-opt-${i}`;
        it.setAttribute("role", "option");
        it.append(el("span", "pkg-opt-name", o.key), el("span", "pkg-opt-ver", o.version ?? ""), el("span", "pkg-opt-from", o.from));
        it.onmousedown = (e) => e.preventDefault(); // the search keeps the focus
        it.onclick = () => pick(o.key);
        return it;
      }),
    );
    if (!shown.length) list.append(el("p", "pkg-opt-none", needle ? "Nothing installed matches" : "Everything is pinned"));
    mark();
  };
  q.oninput = draw;
  q.onkeydown = (e) => {
    const step = { ArrowDown: 1, ArrowUp: -1 }[e.key];
    if (step) {
      e.preventDefault();
      active = Math.min(Math.max(active + step, 0), shown.length - 1);
      mark();
    } else if (e.key === "Enter" && shown[active]) {
      e.preventDefault();
      pick(shown[active].key);
    }
  };
  pop.append(q, list);
  draw();
  const close = popAt(anchor, pop, q, "left");
}

/** An advisory: its severity and title (to the advisory), then the package, CVE and versions. */
function auditRow(v) {
  const row = el("div", "row pkg-advisory");
  const dt = el("dt");
  const title = el(v.url ? "a" : "span", "pkg-adv-title", v.title || "Untitled advisory");
  if (v.url) {
    title.href = v.url;
    title.onclick = (ev) => {
      ev.preventDefault();
      invoke("open_url", { url: v.url }).catch((err) => showError(String(err)));
    };
  }
  dt.append(el("span", `pkg-sev ${v.severity}`, v.severity), title);
  const facts = [
    v.version ? `${v.package} ${v.version}` : v.package,
    v.cve,
    v.range && `affects ${v.range}`,
    v.fixed && `fixed in ${v.fixed}`,
  ];
  row.append(dt, el("dd", "pkg-adv-meta", facts.filter(Boolean).join(" · ")));
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
  const to = el("span", `pkg-new ${update.level}`, update.latest);
  to.title = `${update.level[0].toUpperCase()}${update.level.slice(1)} update`;
  dd.append(v, el("span", "pkg-arrow", "→"), to, el("span", "pkg-major", update.level === "major" ? "major" : ""));
  row.append(dt, dd);
  return row;
}

// The first snapshot can arrive before this file loads.
if (snap) renderMain();
