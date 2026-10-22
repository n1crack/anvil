import { $, ago, call, el, invoke, popAt, renderMain, selected, showError, snap } from "./app";
import { type Tail, readTail, tail, termOut } from "./console";
import { icon } from "./icons";
import { showRun } from "./dev";
import { clean, colorize } from "./term";
import { askProcs, drawProcs, loadProcs } from "./processes";

/* Packages tab, a dashboard: tiles for the framework, the updates and the advisories with
 * the two checks under them; the key packages (pinned per app, the detected ones by
 * default); then the package managers, a row each with its Install and Update… and a line
 * on what the checks found for it, the list itself in a window. Scripts live in the
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
// Runs asked for whose old output may still be there: the window reads it once started.
const starting = new Set<string>();

/** Run Install or Update as Dev runs a script, then read the packages (and, when they
 * were checked, the outdated ones) again. */
async function runPkg(name, eco, id, command, opener) {
  const key = `${name}:${eco}`;
  running.set(key, id);
  ran.delete(key);
  starting.add(key);
  drawPackages(name);
  showLog({ name, key, id, command, opener });
  let started = false;
  let result;
  try {
    await askProcs(name, "package_run", { id });
    started = true;
    starting.delete(key);
    drawLog();
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
  starting.delete(key);
  await loadProcs(name);
  running.delete(key);
  ran.set(key, result);
  drawLog();
  const s = snap.sites.find((x) => x.name === name);
  if (s && selected === name) await window.renderPackages(s);
  if (updatesOf(name)?.at) checkUpdates(name);
}

const output = (name, key, id, command, opener) => {
  const b = el("button", "text-act pkg-output", "Output");
  b.onclick = () => showLog({ name, key, id, command, opener: `${opener} .pkg-output` });
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

/** A row's Install and Update…; Update asks first. Running, the buttons' place says so; after,
 * how it went under it. Either way its output is a click away in Dev. */
function runActs(name, eco, cmds: [string, string][], row: string, missing = false) {
  const key = `${name}:${eco}`;
  const acts = el("div", "pkg-acts");
  const idle = () => {
    acts.classList.remove("asking");
    acts.replaceChildren(
      ...cmds.map(([id, command]) => {
        const update = id.endsWith("-update");
        // Nothing installed yet: Install is the thing to do, the stronger button.
        const look = !update && missing ? "pkg-btn primary" : "pkg-btn";
        const b = el("button", `${look} ${update ? "pkg-update" : "pkg-install"}`, update ? "Update packages…" : "Install packages");
        b.title = command;
        b.onclick = () =>
          update
            ? ask(
                acts,
                `Run ${command}? Packages move to the newest versions their constraints allow, and the lock file changes.`,
                "Update",
                idle,
                () => runPkg(name, eco, id, command, `${row} .pkg-update`),
              )
            : runPkg(name, eco, id, command, `${row} .pkg-install`);
        return b;
      }),
    );
  };
  const going = running.get(key);
  if (going) {
    const command = cmds.find(([id]) => id === going)?.[1] ?? eco;
    acts.replaceChildren(el("span", "spin-dot"), el("span", "muted", `Running ${command}…`), output(name, key, going, command, row));
  } else idle();
  const done = ran.get(key);
  if (!done) return [acts, null];
  const msg = el("p", done.ok ? "pkg-ran ok" : "pkg-error pkg-ran", done.text);
  if (done.ok) msg.prepend(icon("check"));
  const command = cmds.find(([id]) => id === done.id)?.[1] ?? eco;
  if (done.id) msg.append(" ", output(name, key, done.id, command, row));
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
  const up = updatesOf(name);
  const audits = auditsOf(name);
  const audited = p.ecosystems.filter((e) => AUDITED.has(e.id));

  // The tiles: the framework, the updates, the advisories; the two checks under them.
  const tiles = el("div", "pkg-tiles");
  tiles.append(frameworkTile(name, p), updatesTile(up));
  if (audited.length) tiles.append(securityTile(audited.map((e) => audits[e.id])));
  body.append(tiles);
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
  // The update check asks every registry at once, so its failure is the app's, not a section's.
  if (up?.error) body.append(el("p", "pkg-error", up.error));
  body.append(pinsSection(name, p));
  body.append(managersSection(name, p, up, audits));
  if (shown?.name === name) drawFound();
}

const tile = (cls, big, subs: (string | Node)[], tag = "div") => {
  const t = el(tag, `group pkg-tile ${cls}`);
  t.append(big);
  for (const x of subs) {
    const sub = el("div", "pkg-tile-sub");
    sub.append(x);
    t.append(sub);
  }
  return t;
};

/** The app's framework and version (or its first key package), its platform requirements under it. */
function frameworkTile(name, p) {
  const facts: [string, string][] = p.ecosystems.flatMap((e) => e.facts);
  let [label, version] = p.highlights[0] ?? [];
  if (!label) {
    const factKeys = new Set(facts.map(([l]) => l));
    const k = pinsOf(name, p).find((x) => !factKeys.has(x));
    if (k) [label, version] = [new Map<string, string>(p.defaults).get(k) ?? k, versionOf(p, k)];
  }
  if (!label) [label, version] = [managerLabel(p.ecosystems[0]), p.ecosystems[0].manager?.split(" ")[1]];
  const big = el("div", "pkg-tile-big", label);
  if (version) big.append(" ", el("span", "pkg-tile-ver", version));
  return tile(
    "pkg-tile-app",
    big,
    facts.map(([l, v]) => `${l} ${v}`),
  );
}

/** How many updates the last check found, how many major; a click goes to the first. */
function updatesTile(up) {
  if (!up?.at) return tile("pkg-tile-updates", el("div", "pkg-tile-big muted", "Not checked"), []);
  const list = up.list ?? [];
  const n = list.length;
  const major = list.filter((x) => x.level === "major").length;
  const subs = [major ? `${major} major` : "", `checked ${ago(new Date(up.at))}`].filter(Boolean);
  if (!n) return tile("pkg-tile-updates", el("div", "pkg-tile-big", "Up to date"), subs);
  const t = tile("pkg-tile-updates", el("div", "pkg-tile-big", `${n} ${n === 1 ? "update" : "updates"}`), subs, "button");
  t.title = "Show the updates";
  t.onclick = () => showFound({ name: selected!, eco: null, kind: "updates", opener: ".pkg-tile-updates" });
  return t;
}

const SEVERITY = ["critical", "high", "moderate", "medium", "low"];
const rank = (s) => (SEVERITY.indexOf(s) + SEVERITY.length + 1) % (SEVERITY.length + 1); // unknown last

/** The advisories the last security check found, across the managers, and the worst of them. */
function securityTile(checks) {
  const done = checks.filter((a) => a?.at);
  if (!done.length) return tile("pkg-tile-security", el("div", "pkg-tile-big muted", "Not checked"), []);
  const all = done.flatMap((a) => a.list ?? []);
  const when = `checked ${ago(new Date(Math.max(...done.map((a) => a.at))))}`;
  if (!all.length) return tile("pkg-tile-security", el("div", "pkg-tile-big", "No known advisories"), [when]);
  const worst = all.map((v) => v.severity).sort((a, b) => rank(a) - rank(b))[0];
  const big = el("div", "pkg-tile-big bad", `${all.length} ${all.length === 1 ? "advisory" : "advisories"}`);
  const t = tile("pkg-tile-security", big, [el("span", `pkg-sev ${worst}`, worst), when], "button");
  t.title = "Show the advisories";
  t.onclick = () => showFound({ name: selected!, eco: null, kind: "advisories", opener: ".pkg-tile-security" });
  return t;
}

/** The package managers in one box, a row each: its name, what's installed and what the
 * checks found (View opens the list), Install and Update… at the right and, for an npm or
 * yarn app, a line offering pnpm. */
function managersSection(name, p, up, audits) {
  const box = el("section", "pkg-sec pkg-mgrs-sec");
  const head = el("div", "pkg-head");
  head.append(el("h3", "pkg-name", "Package managers"));
  const list = el("div", "group pkg-mgrs");
  for (const e of p.ecosystems) list.append(managerRow(name, p, e, up, AUDITED.has(e.id) ? audits[e.id] : null));
  box.append(head, list);
  return box;
}

function managerRow(name, p, e, up, audit) {
  const row = el("div", "pkg-mgr");
  row.dataset.eco = e.id;
  const info = el("div", "pkg-mgr-info");
  const title = el("div", "pkg-mgr-name", managerLabel(e));
  title.title = [FILE[e.id], e.manager].filter(Boolean).join(" · ");
  info.append(title);
  const sum = summary(name, e, up, audit);
  if (sum) info.append(sum);
  if (audit?.error) info.append(el("p", "pkg-error", audit.error));
  row.append(info);
  const run = RUN[e.id];
  const cmds = run ? p.commands.filter(([id]) => id.startsWith(`pkg-${run}-`)) : [];
  if (cmds.length) {
    const [acts, msg] = runActs(name, run, cmds, `.pkg-mgr[data-eco="${e.id}"]`, e.installed === false);
    row.append(acts);
    if (msg) row.append(msg);
  }
  if (e.id === "npm") {
    const foot = switchFoot(name, e);
    if (foot) row.append(foot);
  }
  return row;
}

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const updatesFor = (up, e) => (up?.list ?? []).filter((x) => x.ecosystem === e.id);
// What Install puts in place, per manager.
const INSTALLS = { composer: "vendor/", npm: "node_modules" };

/** A row's line: installed or not, then what the checks found; View opens the list. */
function summary(name, e, up, audit) {
  const updates = up?.at ? updatesFor(up, e).length : null;
  const advisories = audit?.at && !audit.error ? (audit.list ?? []).length : null;
  const parts: HTMLElement[] = [];
  if (INSTALLS[e.id] && e.installed != null) {
    const state = el("span", e.installed ? "pkg-state" : "pkg-state pkg-missing");
    state.append(el("span", "pkg-dot"), `${INSTALLS[e.id]} ${e.installed ? "installed" : "not installed"}`);
    parts.push(state);
  }
  if (updates != null) parts.push(el("span", updates ? "pkg-sum-n" : null, updates ? plural(updates, "update", "updates") : "up to date"));
  if (advisories != null) {
    const adv = el("span", advisories ? "pkg-sum-adv" : null);
    if (advisories) adv.append(el("b", null, String(advisories)), advisories === 1 ? " security advisory" : " security advisories");
    else adv.append("no known advisories");
    parts.push(adv);
  }
  if (!parts.length) return null;
  const line = el("p", "muted pkg-sum");
  parts.forEach((x, i) => line.append(...(i ? [el("span", "pkg-sep", " · "), x] : [x])));
  if (updates || advisories) {
    const view = el("button", "text-act pkg-view", "View");
    view.onclick = () => showFound({ name, eco: e.id, kind: "all", opener: `.pkg-mgr[data-eco="${e.id}"] .pkg-view` });
    // View goes with the last piece, so a narrow row never wraps it onto a line alone.
    const last = parts.at(-1)!;
    const tie = el("span", "pkg-tie");
    last.replaceWith(tie);
    tie.append(last, view);
  }
  return line;
}

/* What the checks found, in a window: one manager's (View…) or every manager's updates or
 * advisories (the tiles). Drawn again whenever the tab is, so a check that ends while it's
 * open shows at once; closed when nothing is left in it. */
type Found = { name: string; eco: string | null; kind: "all" | "updates" | "advisories"; opener: string };
let shown: Found | null = null;
let sheet: HTMLDialogElement | null = null;

function showFound(f: Found) {
  if (!sheet) {
    sheet = el("dialog", "sheet pkg-sheet") as HTMLDialogElement;
    sheet.setAttribute("aria-labelledby", "pkg-sheet-title");
    const title = el("h2");
    title.id = "pkg-sheet-title";
    const close = el("button", "pkg-sheet-close", "Close");
    close.type = "button";
    close.onclick = () => sheet!.close();
    const actions = el("div", "sheet-actions");
    actions.append(el("span", "spacer"), close);
    // The list takes the focus on open: arrows scroll it, Tab goes on to its links and Close.
    const body = el("div", "pkg-sheet-body");
    body.tabIndex = -1;
    sheet.append(title, body, actions);
    // A click on the backdrop lands on the dialog itself, outside its box.
    sheet.onclick = (ev) => {
      const r = sheet!.getBoundingClientRect();
      if (ev.target === sheet && (ev.clientX < r.left || ev.clientX > r.right || ev.clientY < r.top || ev.clientY > r.bottom))
        sheet!.close();
    };
    // Esc, Close or the backdrop: the focus goes back to what opened it.
    sheet.onclose = () => {
      const opener = shown?.opener;
      shown = null;
      if (opener) ($(`.pkg-body ${opener}`) as HTMLElement | null)?.focus();
    };
    document.body.append(sheet);
  }
  shown = f;
  drawFound();
  if (!shown) return;
  if (!sheet.open) sheet.showModal();
  (sheet.querySelector(".pkg-sheet-body") as HTMLElement).focus();
}

function drawFound() {
  const f = shown!;
  const p = pkgCache.get(f.name);
  const up = updatesOf(f.name);
  const audits = auditsOf(f.name);
  const ecos = (p?.ecosystems ?? []).filter((e) => !f.eco || e.id === f.eco);
  const groups: Node[] = [];
  let [nUp, nAdv] = [0, 0];
  for (const e of ecos) {
    const advisories = f.kind === "updates" ? [] : (audits[e.id]?.list ?? []);
    const newer = new Map<string, any>(f.kind === "advisories" ? [] : updatesFor(up, e).map((x) => [x.name, x]));
    const rows = e.packages.filter((x) => newer.has(x.name)).sort((a, b) => a.name.localeCompare(b.name));
    if (!advisories.length && !rows.length) continue;
    nUp += rows.length;
    nAdv += advisories.length;
    if (!f.eco) groups.push(el("h3", "pkg-name", managerLabel(e)));
    const group = el("div", "group pkg-found");
    if (advisories.length) {
      const dl = el("dl", "pkg-adv-list");
      for (const v of advisories) dl.append(auditRow(v));
      group.append(dl);
    }
    if (rows.length) {
      const dl = el("dl", "pkg-list updates");
      for (const x of rows) dl.append(pkgRow(e.id, x, newer.get(x.name)));
      group.append(dl);
    }
    groups.push(group);
  }
  // A check that found nothing this time leaves nothing to show.
  if (!groups.length) {
    if (sheet?.open) sheet.close();
    else shown = null;
    return;
  }
  const counts = [nUp && plural(nUp, "update", "updates"), nAdv && plural(nAdv, "advisory", "advisories")].filter(Boolean).join(", ");
  $("#pkg-sheet-title").textContent = f.eco ? `${managerLabel(ecos[0])} — ${counts}` : counts;
  sheet!.querySelector(".pkg-sheet-body")!.replaceChildren(...groups);
}

/* An Install or Update's output, in a window: live while it runs, how it ended under it.
 * Closing it leaves the run going; the row's Output opens it again. */
type Log = { name: string; key: string; id: string; command: string; opener: string; t?: Tail };
let logShown: Log | null = null;
let logSheet: HTMLDialogElement | null = null;
let logTerm: ReturnType<typeof termOut>;
let logTimer = 0;

function showLog(l: Log) {
  if (!logSheet) {
    logSheet = el("dialog", "sheet pkg-sheet pkg-log-sheet") as HTMLDialogElement;
    logSheet.setAttribute("aria-labelledby", "pkg-log-title");
    const title = el("h2");
    title.id = "pkg-log-title";
    logTerm = termOut("console-out pkg-log");
    logTerm.out.tabIndex = -1;
    const status = el("span", "pkg-log-status");
    status.setAttribute("role", "status");
    const dev = el("button", "link pkg-log-dev", "Show in Dev");
    dev.type = "button";
    dev.onclick = () => {
      const { name, id } = logShown!;
      logSheet!.close();
      showRun(name, id);
    };
    const close = el("button", "pkg-log-close", "Close");
    close.type = "button";
    close.onclick = () => logSheet!.close();
    const actions = el("div", "sheet-actions");
    actions.append(status, el("span", "spacer"), dev, close);
    logSheet.append(title, logTerm.out, actions);
    logSheet.onclick = (ev) => {
      const r = logSheet!.getBoundingClientRect();
      if (ev.target === logSheet && (ev.clientX < r.left || ev.clientX > r.right || ev.clientY < r.top || ev.clientY > r.bottom))
        logSheet!.close();
    };
    logSheet.onclose = () => {
      clearInterval(logTimer);
      const opener = logShown?.opener;
      logShown = null;
      if (opener) ($(`.pkg-body ${opener}`) as HTMLElement | null)?.focus();
    };
    document.body.append(logSheet);
  }
  logShown = l;
  logTerm.out.replaceChildren();
  logTerm.end();
  $("#pkg-log-title").textContent = l.command;
  drawLog();
  clearInterval(logTimer);
  logTimer = window.setInterval(readLog, 300);
  readLog();
  if (!logSheet.open) logSheet.showModal();
  logTerm.out.focus();
}

/** New output, once the run has started (before, the log is still the last run's). */
async function readLog() {
  const l = logShown;
  if (!l || starting.has(l.key)) return;
  l.t ??= tail(l.name, `once:${l.id}`);
  const text = await readTail(l.t);
  if (logShown !== l || (!text && !l.t.cleared)) return;
  l.t.cleared = false;
  // ponytail: redraws the whole run each time (a \r redraws an earlier line); Dev's line
  // buffer if runs get long enough for that to show.
  logTerm.out.replaceChildren(colorize(clean(l.t.raw, true)));
  logTerm.follow();
}

/** The status under the output: running, finished or how it failed. */
function drawLog() {
  const l = logShown;
  if (!l || !logSheet) return;
  const status = logSheet.querySelector(".pkg-log-status")!;
  const going = running.get(l.key) === l.id;
  const done = going ? null : ran.get(l.key);
  status.className = `pkg-log-status ${done ? (done.ok ? "ok" : "bad") : ""}`.trim();
  if (!going && !done) status.replaceChildren();
  else if (!done) status.replaceChildren(el("span", "spin-dot"), starting.has(l.key) ? "Starting…" : "Running…");
  else if (done.ok) status.replaceChildren(icon("check"), "Finished");
  else
    status.replaceChildren(
      done.text
        .replace(`${l.command} `, "")
        .replace(/^./, (c) => c.toUpperCase())
        .replace(/\.$/, ""),
    );
}

/** An npm or yarn app can move to pnpm, only when asked and confirmed in its row. */
function switchFoot(name, e) {
  const lock = e.lock === "package-lock.json" || e.lock === "yarn.lock" ? e.lock : null;
  const going = busy.has(`${name}:pnpm`);
  const sw = switched.get(name);
  if (!lock && !going && !sw) return null;
  const foot = el("div", "pkg-foot");
  const line = el("div", "pkg-acts");
  const idle = () => {
    line.classList.remove("asking");
    const b = el("button", "text-act pkg-switch", "Switch…");
    b.title = "Switch to pnpm";
    b.onclick = () =>
      ask(line, `Replace ${lock} with pnpm-lock.yaml and install node_modules again? Test the app afterwards.`, "Switch", idle, () =>
        switchToPnpm(name),
      );
    line.replaceChildren(el("span", "muted", "pnpm is faster and saves disk space."), b);
  };
  if (going) line.replaceChildren(el("span", "spin-dot"), el("span", "muted", "Switching to pnpm…"));
  else if (lock) idle();
  if (line.childNodes.length) foot.append(line);
  if (sw && !going) foot.append(el("p", sw.ok ? "muted pkg-ran" : "pkg-error pkg-ran", sw.text));
  return foot;
}

const pinsOf = (name, p): string[] => snap.sites.find((x) => x.name === name)?.packages_pinned ?? p.defaults.map(([k]) => k);

/** Key packages: name and version each, × (on hover) unpins one, + Add pins another, Reset
 * brings back the detected ones. The platform requirements are on the framework tile. */
function pinsSection(name, p) {
  const s = snap.sites.find((x) => x.name === name);
  const defaults = p.defaults.map(([k]) => k);
  const pins = pinsOf(name, p);
  const facts = new Set(p.ecosystems.flatMap((e) => e.facts.map(([l]) => l)));
  const labels = new Map<string, string>(p.defaults);
  const save = (next) => call("set_packages_pinned", { site: name, pins: next });
  const box = el("section", "pkg-sec pkg-pins-sec");
  const head = el("div", "pkg-head");
  const acts = el("div", "pkg-acts");
  head.append(el("h3", "pkg-name", "Key packages"), acts);
  if (pins.join("\n") !== defaults.join("\n")) {
    const reset = el("button", "text-act pkg-pin-reset", "Reset");
    reset.title = "Back to the detected packages";
    // A worktree's unset is its app's, so its reset is the defaults themselves.
    reset.onclick = () => save(s?.worktree_of ? defaults : null);
    acts.append(reset);
  }
  const add = el("button", "text-act pkg-pin-add", "+ Add");
  add.title = "Pin another installed package to this list";
  add.onclick = () => pinPop(add, p, pins, save);
  acts.append(add);
  box.append(head);
  const grid = el("div", "group pkg-pins");
  for (const k of pins.filter((x) => !facts.has(x))) {
    const label = labels.get(k) ?? k;
    const c = el("div", "pkg-pin");
    const n = el("span", "pkg-pin-name", label);
    if (label !== k) n.title = k;
    c.append(n, el("span", "pkg-pin-ver", versionOf(p, k) ?? ""));
    const x = el("button", "pkg-pin-x");
    x.append(icon("x"));
    x.title = "Remove from this list";
    x.setAttribute("aria-label", `Unpin ${label}`);
    x.onclick = () => save(pins.filter((o) => o !== k));
    c.append(x);
    grid.append(c);
  }
  if (!grid.childNodes.length) grid.append(el("p", "muted pkg-none", "None pinned"));
  box.append(grid);
  return box;
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
  for (const e of p.ecosystems)
    for (const x of [...e.packages].sort((a, b) => byName(a.name, b.name))) offer(x.name, x.version ?? x.constraint, managerLabel(e));
  for (const e of p.ecosystems) for (const [n, v] of [...e.locked].sort((a, b) => byName(a[0], b[0]))) offer(n, v, managerLabel(e));

  const pop = el("div", "pkg-pin-pop");
  pop.setAttribute("aria-label", "Pin a package");
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
