import "./platform";
import type { Settings } from "./types";
import { ownError } from "./live";
import { bubbleKeys, confirmInRow, textAct } from "./confirm";
import { NAMES, editor } from "./hooksform";
import { inboxView, type Inbox } from "./inbox";

export const { invoke } = window.__TAURI__.core;

export const $ = (sel: string, root: ParentNode = document): any => root.querySelector(sel);
export const tilde = (p) => p.replace(/^\/Users\/[^/]+/, "~");
export let s: Settings = null as unknown as Settings; // set by the first load

export async function call(cmd: string, args?: Record<string, unknown>) {
  try {
    s = (await invoke(cmd, args)) ?? s;
    showError(null, cmd);
    render();
  } catch (e) {
    showError(String(e), cmd);
  }
}

const errors = new Map<string, string>(); // owner -> its error, see ownError
export function showError(msg, owner = "") {
  const text = ownError(errors, msg, owner);
  $("#error").textContent = text;
  $("#error").hidden = !text;
}

window.refresh = () => {
  call("settings");
};

/* ---------- Languages ---------- */

export let runtimes: any = null;
export const installing = new Set<string>(); // "id@version", "php@8.3" for PHP

const CHECK = `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 8.5 6.5 11.5 12.5 4.75" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

/** The page of each language Anvil installs besides PHP, and which version an
 * app uses when it isn't pinned to one (as apps.rs and runtimes.rs decide). */
const LANGS = [
  [
    "node",
    "Node.js",
    "Apps use the version in .nvmrc or .node-version when it is installed, otherwise the newest. An app can pin one in its settings.",
  ],
  ["go", "Go", "Apps use the newest installed version unless an app pins one in its settings."],
  ["rust", "Rust", "One toolchain, managed by rustup inside Anvil’s folder, serves every app."],
  ["dotnet", ".NET", "The SDKs sit side by side; dotnet picks one per project (global.json), otherwise the newest."],
  ["python", "Python", "uv picks one of these per project (.python-version, requires-python) unless an app pins one in its settings."],
];

// One page per language, the same shape as PHP's.
for (const [id, name, note] of LANGS) {
  const page = document.createElement("section");
  page.id = id;
  page.className = "page";
  page.hidden = true;
  page.innerHTML = `<h1></h1><p class="hint"></p><h2>Versions</h2><div class="group versions"><div class="row"><span class="muted">Loading versions…</span></div></div><p class="hint note"></p>`;
  $("h1", page).textContent = name;
  $(".hint", page).textContent =
    `Installed into Anvil’s folder and used only for the apps Anvil runs; ${name} on your system stays untouched.`;
  $(".note", page).textContent = note;
  if (id === "node") {
    page.insertAdjacentHTML(
      "beforeend",
      `<h2>Package managers</h2><div id="pms" class="group pms"><div class="row"><span class="muted">Looking…</span></div></div>` +
        `<p class="hint note">An app missing node_modules gets them from the manager its lockfile names (npm without one). ` +
        `Apps find yarn and bun installed here first, and your own pnpm before Anvil’s, which is downloaded when an app needs it; your terminal is left alone.</p>`,
    );
  }
  $("#storage").before(page);
}
/** Newest first, numerically: "22" > "9", "1.27.2" > "1.9". */
const newest = (a: string, b: string) => b.localeCompare(a, undefined, { numeric: true });

/** One row of a Versions list: muted with Install when missing, a check and
 * "Default" on the default, Make default and Uninstall on hover otherwise. A version
 * apps are pinned to names them and offers no Uninstall (the core refuses it). */
function versionRow(
  label: string,
  v: { installed: boolean; default?: boolean; pinned_by?: string[]; unsupported?: string },
  busy: boolean,
  blocked: boolean,
  act: { install: () => void; uninstall: () => void; makeDefault?: () => void; note?: string },
  extra?: { slot?: HTMLElement; end?: HTMLElement; error?: HTMLElement; below?: HTMLElement },
) {
  const row = document.createElement("div");
  row.className = `row ver${v.installed ? "" : " missing"}${v.default ? " is-default" : ""}`;
  const name = document.createElement("span");
  name.className = "ver-name";
  name.innerHTML = `<span class="ver-mark">${v.default ? CHECK : ""}</span><span class="ver-label"></span>`;
  $(".ver-label", name).textContent = label;
  if (extra?.slot) name.append(extra.slot);
  if (v.default) name.append(Object.assign(document.createElement("span"), { className: "ver-default", textContent: "Default" }));
  const pinned = v.installed && v.pinned_by?.length ? v.pinned_by.join(", ") : "";
  if (pinned)
    name.append(
      Object.assign(document.createElement("span"), {
        className: "ver-pinned",
        textContent: `Pinned by ${pinned}`,
        title: `Unpin ${pinned} first to uninstall it`,
      }),
    );
  const actions = document.createElement("span");
  actions.className = "actions-inline";
  if (busy) {
    actions.innerHTML = `<span class="spin-dot"></span><span class="muted">Installing…</span>`;
  } else if (!v.installed && v.unsupported) {
    // This Mac is too old for it; the row already names the version.
    const why = v.unsupported.replace(/^.*? needs /, "Needs ");
    actions.append(Object.assign(document.createElement("span"), { className: "muted", textContent: why }));
  } else if (!v.installed) {
    const b = button("Install", act.install);
    b.disabled = blocked;
    actions.append(b);
  } else if (!v.default) {
    actions.classList.add("on-hover");
    const idle = () => {
      const acts: HTMLElement[] = [];
      if (act.makeDefault) acts.push(textAct("Make default", act.makeDefault));
      if (!pinned)
        acts.push(
          textAct("Uninstall", () => confirmInRow(actions, `Uninstall? ${act.note ?? ""}`.trim(), "Uninstall", act.uninstall, idle)),
        );
      actions.replaceChildren(...acts);
    };
    idle();
  }
  // The row's own control (PHP's Extensions) at the far end, after the actions: it stays
  // in one place whether they show or not.
  row.append(name, actions, ...(extra?.end ? [extra.end] : []));
  if (extra?.below) row.append(extra.below);
  if (extra?.error) {
    row.classList.add("has-error");
    row.append(extra.error);
  }
  return row;
}

// "node@22", "php@8.3" -> why its last Install, Uninstall or Make default failed, said in
// its row; the language's next action clears them.
const verErrors = new Map<string, string>();

/** A row's failed action, as versionRow's `error`. */
function verError(key: string) {
  const reason = verErrors.get(key);
  if (!reason) return undefined;
  const error = Object.assign(document.createElement("div"), { className: "ver-error" });
  error.append(Object.assign(document.createElement("span"), { textContent: reason }));
  return error;
}

/** Run a version row's action; a failure lands in that row instead of the page's top. */
async function verAction(key: string, run: () => Promise<unknown>) {
  const lang = key.split("@")[0];
  for (const k of verErrors.keys()) if (k.startsWith(`${lang}@`)) verErrors.delete(k);
  try {
    await run();
  } catch (e) {
    verErrors.set(key, String(e));
  }
}

/** A PHP version row's command (php_install, php_uninstall, set_default_php). */
async function phpAct(version: string, cmd: string) {
  await verAction(`php@${version}`, async () => {
    s = (await invoke(cmd, { version })) ?? s;
  });
  render();
}

async function loadRuntimes() {
  try {
    runtimes = await invoke("runtimes");
    showError(null, "runtimes");
  } catch (e) {
    showError(String(e), "runtimes");
  }
  renderLanguages();
}

function renderLanguages() {
  for (const rt of runtimes ?? []) {
    const list = $(`#${rt.id} .versions`);
    if (!list) continue;
    const all = [...new Set<string>([...rt.available, ...rt.installed])].sort(newest);
    const blocked = [...installing].some((k) => k.startsWith(`${rt.id}@`));
    list.replaceChildren(
      ...all.map((v) =>
        versionRow(
          `${rt.name} ${v}`,
          { installed: rt.installed.includes(v), pinned_by: rt.pinned_by?.[v], unsupported: rt.unsupported?.[v] },
          installing.has(`${rt.id}@${v}`),
          blocked,
          {
            install: () => installRuntime(rt.id, v),
            uninstall: () => uninstallRuntime(rt, v),
            note: "Apps on it switch to another version.",
          },
          { error: verError(`${rt.id}@${v}`) },
        ),
      ),
    );
    if (!all.length) list.innerHTML = `<div class="row"><span class="muted">No versions found. Check the internet connection.</span></div>`;
  }
}

async function installRuntime(id: string, version: string) {
  const k = `${id}@${version}`;
  installing.add(k);
  const done = verAction(k, () => invoke("runtime_install", { id, version })); // clears the old errors first
  renderLanguages();
  await done;
  installing.delete(k);
  loadRuntimes();
}

async function uninstallRuntime(rt, v: string) {
  await verAction(`${rt.id}@${v}`, () => invoke("runtime_uninstall", { id: rt.id, version: v }));
  loadRuntimes();
}

export let pms: any[] | null = null;
const pmBusy = new Map<string, string>(); // name → "Installing…" / "Uninstalling…"
const pmErrors = new Map<string, string>();

/** npm, pnpm, yarn and bun: where apps find them and, when it differs, the terminal. */
async function loadPackageManagers() {
  try {
    pms = await invoke("package_managers");
  } catch (e) {
    return showError(String(e));
  }
  renderPackageManagers();
}

/** "Bundled with Node.js 24 11.6.2", the path in the tooltip. */
function found(label: string, f) {
  const line = document.createElement("span");
  line.append(`${label}: `);
  const src = Object.assign(document.createElement("span"), { className: "pm-src", textContent: f ? f.source : "Not found" });
  if (f) src.title = tilde(f.path);
  line.append(src);
  if (f)
    line.append(
      " ",
      Object.assign(document.createElement("span"), { className: "pm-version", textContent: f.version ?? "unknown version" }),
    );
  return line;
}

function renderPackageManagers() {
  const box = $("#pms");
  if (!box || !pms) return;
  box.replaceChildren(
    ...pms.map((pm) => {
      const row = document.createElement("div");
      row.className = `row pm${pm.apps ? "" : " missing"}`;
      const name = Object.assign(document.createElement("span"), { className: "pm-name", textContent: pm.name });
      const where = Object.assign(document.createElement("span"), { className: "pm-where" });
      where.append(Object.assign(found("Apps", pm.apps), { className: "pm-from" }));
      if (!pm.same && (pm.apps || pm.terminal)) where.append(Object.assign(found("Terminal", pm.terminal), { className: "pm-term" }));
      const error = pmErrors.get(pm.name);
      if (error) where.append(Object.assign(document.createElement("span"), { className: "pm-error", textContent: error }));
      const actions = Object.assign(document.createElement("span"), { className: "actions-inline" });
      const busy = pmBusy.get(pm.name);
      if (busy) {
        actions.innerHTML = `<span class="spin-dot"></span><span class="muted"></span>`;
        $(".muted", actions).textContent = busy;
      } else if (!pm.apps && pm.name !== "npm") {
        const b = button("Install", () => pmAction(pm.name, "package_manager_install", "Installing…"));
        b.disabled = [...pmBusy.values()].includes("Installing…");
        actions.append(b);
      } else if (pm.anvil) {
        actions.classList.add("on-hover");
        const idle = () =>
          actions.replaceChildren(
            textAct("Uninstall", () =>
              confirmInRow(
                actions,
                pm.name === "pnpm"
                  ? "Uninstall? An app that needs pnpm gets it again."
                  : "Uninstall? Apps find the next one on their PATH.",
                "Uninstall",
                () => pmAction(pm.name, "package_manager_uninstall", "Uninstalling…"),
                idle,
              ),
            ),
          );
        idle();
      }
      row.append(name, where, actions);
      return row;
    }),
  );
}

async function pmAction(name: string, cmd: string, label: string) {
  pmBusy.set(name, label);
  pmErrors.delete(name);
  renderPackageManagers();
  try {
    await invoke(cmd, { name });
  } catch (e) {
    pmErrors.set(name, String(e));
  }
  pmBusy.delete(name);
  await loadPackageManagers();
}

/* ---------- Storage ---------- */

export function bytes(n) {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n >= 10 || i === 0 ? Math.round(n) : n.toFixed(1)} ${u[i]}`;
}

/** Size text runs from the normal text colour to red on a log scale:
 * 10 MB and below plain, 20 GB and above full red. Only the number changes. */
function sizeColor(n) {
  const lo = Math.log10(10 * 1024 ** 2);
  const hi = Math.log10(20 * 1024 ** 3);
  const t = Math.min(1, Math.max(0, (Math.log10(Math.max(n, 1)) - lo) / (hi - lo)));
  // A gentler start, so red is for the really big ones.
  return `color-mix(in oklab, var(--danger) ${Math.round(t ** 1.4 * 100)}%, var(--text))`;
}

const SAFETY_LABEL = { safe: "Safe to delete", rebuild: "Rebuildable", keep: "Keep" };

/** The button at a storage row's end, in a slot as wide on every row so the sizes line up. */
function slot(b?: HTMLElement) {
  const box = Object.assign(document.createElement("span"), { className: "act-slot" });
  if (b) box.append(b);
  return box;
}

async function loadAnvilStorage() {
  let st;
  try {
    st = await invoke("storage_anvil");
  } catch (e) {
    return showError(String(e));
  }
  $("#anvil-total").textContent = `PHP, languages, services, their data, caches and logs. Caches and logs can be deleted safely.`;
  $("#anvil-size").textContent = bytes(st.total);
  const box = $("#anvil-items");
  box.replaceChildren();
  // One group per kind of thing, in this order; sizes add up per group.
  const groups = [
    ["PHP", "PHP"],
    ["Languages", "Languages"],
    ["Services", "Service programs"],
    ["Data", "Service data"],
    ["Anvil", "Web server"],
    ["Cache", "Caches"],
    ["Logs", "Logs"],
  ];
  // Anything in a category not listed above still shows, last.
  const known = new Set(groups.map(([cat]) => cat));
  for (const cat of new Set<string>(st.items.map((i) => i.category))) if (!known.has(cat)) groups.push([cat, cat]);
  for (const [cat, title] of groups) {
    const items = st.items.filter((i) => i.category === cat);
    if (!items.length) continue;
    const head = document.createElement("h2");
    head.className = "storage-group";
    head.append(title, Object.assign(document.createElement("span"), { textContent: bytes(items.reduce((a, i) => a + i.bytes, 0)) }));
    const list = document.createElement("div");
    list.className = "group";
    box.append(head);
    // A note every item shares is said once, under the heading.
    const shared = items.every((i) => i.note === items[0].note) && items.length > 1 ? items[0].note : null;
    if (shared) box.append(Object.assign(document.createElement("p"), { className: "hint", textContent: shared }));
    box.append(list);
    for (const i of items) drawAnvilItem(list, i, !shared);
  }
}

function drawAnvilItem(list, i, withNote) {
  const row = document.createElement("div");
  row.className = "row";
  const info = document.createElement("span");
  const name = document.createElement("span");
  name.textContent = i.label;
  const note = document.createElement("small");
  note.textContent = i.note;
  info.append(name);
  if (withNote) info.append(note);
  const actions = document.createElement("span");
  actions.className = "actions-inline";
  if (i.safety !== "keep") {
    const tag = document.createElement("span");
    tag.className = `safety ${i.safety}`;
    tag.textContent = SAFETY_LABEL[i.safety];
    actions.append(tag);
  }
  const size = document.createElement("span");
  size.className = "size";
  size.textContent = bytes(i.bytes);
  size.style.color = sizeColor(i.bytes);
  const tags = [...actions.childNodes];
  const idle = () => actions.replaceChildren(...tags, size, slot(i.safety === "safe" ? button("Delete", ask) : undefined));
  // Asks in the row; what it is stays said beside it.
  const ask = () =>
    confirmInRow(
      actions,
      `Delete ${bytes(i.bytes)}?`,
      "Delete",
      async () => {
        try {
          await invoke("clean_anvil", { path: i.path });
        } catch (e) {
          showError(String(e));
        }
        loadAnvilStorage();
      },
      () => {
        actions.classList.remove("asking");
        idle();
      },
    );
  idle();
  row.append(info, actions);
  list.append(row);
}

/* pnpm's store: shared by every pnpm project on this Mac, so it stays where pnpm keeps
 * it and is never deleted whole; Prune drops what no project uses, confirmed in the row. */
let pnpmState: { busy?: string; note?: string; error?: boolean; asking?: boolean } = {};

async function loadPnpmStore() {
  const box = $("#pnpm-store");
  const st = await invoke("pnpm_store").catch(() => null);
  if (!st) return box.replaceChildren();
  const head = document.createElement("h2");
  head.className = "storage-group";
  head.append("pnpm store", Object.assign(document.createElement("span"), { textContent: bytes(st.bytes) }));
  const row = document.createElement("div");
  row.className = "row";
  const info = document.createElement("span");
  info.append(
    Object.assign(document.createElement("span"), { textContent: tilde(st.path) }),
    Object.assign(document.createElement("small"), {
      textContent: pnpmState.note ?? "Shared by every pnpm project",
      className: pnpmState.error ? "bad" : "",
    }),
  );
  const actions = document.createElement("span");
  actions.className = "actions-inline";
  if (pnpmState.busy) {
    actions.innerHTML = `<span class="spin-dot"></span><span class="muted"></span>`;
    $(".muted", actions).textContent = pnpmState.busy;
  } else if (pnpmState.asking) {
    actions.classList.add("asking");
    actions.append(
      Object.assign(document.createElement("span"), { className: "muted", textContent: "Prune? Packages no project uses go." }),
      textAct("Cancel", () => {
        pnpmState = {};
        loadPnpmStore();
      }),
      Object.assign(textAct("Prune", prunePnpm), { className: "text-act danger" }),
    );
  } else {
    actions.append(
      Object.assign(document.createElement("span"), { className: "size", textContent: bytes(st.bytes) }),
      slot(
        button("Prune…", () => {
          pnpmState = { asking: true };
          loadPnpmStore();
        }),
      ),
    );
  }
  const list = document.createElement("div");
  list.className = "group";
  row.append(info, actions);
  list.append(row);
  box.replaceChildren(head, list);
}

async function prunePnpm() {
  pnpmState = { busy: "Pruning…" };
  loadPnpmStore();
  try {
    const freed = await invoke("pnpm_prune");
    pnpmState = { note: freed ? `Pruned: about ${bytes(freed)} freed` : "Pruned: nothing to free" };
  } catch (e) {
    pnpmState = { note: String(e), error: true };
  }
  loadPnpmStore();
}

/* Application sizes: the last measurement shows at once, and a new one fills
 * in row by row as each app is counted. The toolbar and the list hold their
 * place from the start: while measuring the refresh icon spins, Delete waits
 * disabled, and placeholder rows keep the list's height until numbers arrive. */
export const sizes = new Map<string, any>(); // app name -> { name, kind, total, safe, rebuild }
let measuring = false;
export let loading = true; // the last measurement not read yet
let deleting = false;
let measuredAt = 0; // Unix seconds of the shown numbers
let drawQueued = false;
let fresh: Set<string> | null = null; // apps counted by the running measurement
const PLACEHOLDERS = 6;

window.__TAURI__.event.listen("storage-app", (e) => {
  sizes.set(e.payload.name, e.payload);
  fresh?.add(e.payload.name);
  if (!drawQueued) {
    drawQueued = true;
    requestAnimationFrame(() => {
      drawQueued = false;
      drawSizes();
    });
  }
});

async function loadAppSizes() {
  if (measuring) return;
  drawSizes();
  const last = await invoke("storage_last").catch(() => null);
  loading = false;
  if (last && !sizes.size) {
    measuredAt = last[0];
    for (const a of last[1]) sizes.set(a.name, a);
  }
  // Numbers younger than ten minutes are good enough; the refresh button is there.
  if (!last || Date.now() / 1000 - last[0] > 600) scanApps();
  else drawSizes();
}

async function scanApps() {
  if (measuring) return;
  measuring = true;
  fresh = new Set();
  drawSizes();
  try {
    await invoke("storage_apps");
    // Apps that are gone since the last measurement drop out.
    for (const name of sizes.keys()) if (!fresh.has(name)) sizes.delete(name);
    measuredAt = Date.now() / 1000;
  } catch (e) {
    showError(String(e));
  }
  fresh = null;
  measuring = false;
  drawSizes();
}

// In a narrow window "in All Apps" and the size go (style.css); the hint under it has the size.
const DELETE_ALL = (size = "") => [
  "Delete Logs and Caches",
  Object.assign(document.createElement("span"), { className: "wide-only", textContent: ` in All Apps${size}` }),
];
const cleanAll = $("#clean-all-safe"); // kept while its confirm stands in its place

function drawSizes() {
  const apps = [...sizes.values()].sort((a, b) => b.total - a.total);
  const sum = (k) => apps.reduce((a, x) => a + x[k], 0);
  const busy = measuring || loading;
  $("#apps-size").textContent = apps.length ? bytes(sum("total")) : "";
  // One state at a time: measuring, the numbers, or (after a measurement) nothing to show.
  $("#apps-hint").textContent = busy
    ? "Measuring every application…"
    : !apps.length
      ? ""
      : `${apps.length} applications use ${bytes(sum("total"))}. ${bytes(sum("safe"))} is logs and caches that can be deleted safely; ` +
        `${bytes(sum("rebuild"))} is dependencies and builds that can be recreated.`;

  const scan = $("#scan-apps");
  scan.disabled = busy || deleting;
  scan.classList.toggle("spinning", busy);
  $("#measured").textContent = busy ? "Measuring…" : measuredAt ? `Measured ${agoText(measuredAt)}` : "";

  const all = cleanAll;
  const safe = sum("safe");
  all.disabled = busy || deleting || !safe;
  all.replaceChildren(...DELETE_ALL(busy || deleting || !safe ? "" : ` (${bytes(safe)})`));
  if (deleting) all.prepend(Object.assign(document.createElement("span"), { className: "spin-dot" }));

  const list = $("#app-sizes");
  list.replaceChildren();
  const shown = apps.filter((a) => a.total > 0);
  for (const a of shown) {
    const row = document.createElement("div");
    row.className = "row";
    const info = document.createElement("span");
    const name = document.createElement("span");
    name.textContent = a.name;
    const note = document.createElement("small");
    const parts: any[] = [];
    if (a.rebuild) parts.push(`${bytes(a.rebuild)} rebuildable`);
    if (a.safe) parts.push(`${bytes(a.safe)} safe to delete`);
    note.textContent = parts.join(", ") || "Mostly source and data";
    info.append(name, note);
    const size = document.createElement("span");
    size.className = "size";
    size.textContent = bytes(a.total);
    size.style.color = sizeColor(a.total);
    row.append(info, size);
    list.append(row);
  }
  // Not counted yet: grey bars where rows will be, so the page doesn't jump.
  for (let i = busy ? shown.length : PLACEHOLDERS; i < PLACEHOLDERS; i++) {
    const row = document.createElement("div");
    row.className = "row placeholder";
    // The same lines as a measured row, so it is just as tall.
    row.innerHTML = `<span><span><i class="bar"></i></span><small><i class="bar short"></i></small></span><span class="size"><i class="bar size"></i></span>`;
    list.append(row);
  }
  if (!busy && measuredAt && !shown.length)
    list.innerHTML = `<div class="row"><span class="muted">No applications take any space.</span></div>`;
}

function agoText(secs) {
  const s = Date.now() / 1000 - secs;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}

$("#scan-apps").onclick = scanApps;
// Asks in the toolbar: logs, compiled views and caches in every app; sessions too.
cleanAll.onclick = () => {
  const safe = [...sizes.values()].reduce((a, x) => a + x.safe, 0);
  const box = $("#clean-all-box");
  confirmInRow(box, `Delete ${bytes(safe)}? Users get logged out.`, "Delete", cleanAllSafe, () => {
    box.classList.remove("asking");
    box.replaceChildren(cleanAll);
  });
};
async function cleanAllSafe() {
  $("#clean-all-box").classList.remove("asking");
  $("#clean-all-box").replaceChildren(cleanAll);
  deleting = true;
  drawSizes();
  try {
    const [freed, failed] = await invoke("clean_all_safe");
    showError(failed.length ? `Freed ${bytes(freed)}. Couldn't clean: ${[...new Set(failed)].join(", ")}` : null, "clean_all_safe");
    // Said beside the button, as on an app's Resources; it goes with the next question.
    if (!failed.length && freed)
      $("#clean-all-box").replaceChildren(
        Object.assign(document.createElement("span"), { className: "muted", textContent: `Freed ${bytes(freed)}` }),
        cleanAll,
      );
  } catch (e) {
    showError(String(e), "clean_all_safe");
  }
  deleting = false;
  scanApps();
}

export function options(select, values, current, none) {
  select.replaceChildren(new Option(none, ""));
  for (const v of values) select.add(new Option(v, v));
  // Keep a configured app selectable even if it isn't installed where we look.
  if (current && !values.includes(current)) select.add(new Option(current, current));
  select.value = current ?? "";
}

export function render() {
  // Hidden applications, each with Show.
  const hidden = s.hidden ?? [];
  $("#hidden-box").hidden = !hidden.length;
  $("#hidden-apps").replaceChildren(
    ...hidden.map((name) => {
      const row = document.createElement("div");
      row.className = "row";
      const label = document.createElement("span");
      label.textContent = name;
      const show = button("Show", async () => {
        await invoke("unhide_site", { site: name }).catch((e) => showError(String(e)));
        call("settings");
      });
      show.setAttribute("aria-label", `Show ${name}`);
      row.append(label, show);
      return row;
    }),
  );
  // Site folders
  const paths = $("#paths");
  paths.replaceChildren();
  if (!s.paths.length) {
    paths.innerHTML = `<div class="row"><span class="muted">No folders yet.</span></div>`;
  }
  for (const p of s.paths) {
    const row = document.createElement("div");
    row.className = "row";
    row.innerHTML = `<span class="path"></span><span class="actions-inline"><button>Remove</button></span>`;
    $(".path", row).textContent = tilde(p);
    $("button", row).setAttribute("aria-label", `Remove ${tilde(p)}`);
    // Asks in the row: its apps leave Anvil and stop, nothing on disk changes.
    const box = $(".actions-inline", row);
    const remove = $("button", row);
    remove.onclick = () =>
      confirmInRow(
        box,
        "Its applications leave Anvil and their processes stop. The files stay.",
        "Remove",
        () => call("remove_path", { path: p }),
        () => {
          box.classList.remove("asking");
          box.replaceChildren(remove);
        },
      );
    paths.append(row);
  }

  $("#login").checked = s.login;
  $("#dock").checked = s.dock_icon;
  $("#trim-logs").checked = s.trim_app_logs;
  $("#notifications").checked = s.notifications;
  $("#theme").value = s.theme ?? "";
  options($("#editor"), s.editors, s.editor, "None");
  options(
    $("#terminal"),
    s.terminals.filter((t) => t !== "Terminal"),
    s.terminal,
    "Terminal",
  );
  const git = $("#git-client");
  options(git, s.git_clients, s.git_client, s.git_clients.length ? `Automatic (${s.git_clients[0]})` : "None installed");
  // A picked app is stored by path; show its name.
  for (const o of git.options)
    if (o.value.includes("/"))
      o.text = o.value
        .split("/")
        .pop()
        .replace(/\.app$/, "");
  git.add(new Option("Other app…", "other"));

  // The address a new application gets.
  const [scheme, port, std] = s.https_default ? ["https", s.https_port, 443] : ["http", s.http_port, 80];
  $("#address").textContent = `${scheme}://name.${s.tld}${port === std ? "" : `:${port}`}`;
  $("#https-default").checked = s.https_default;
  $("#worktree-db").checked = s.worktree_db === "copy";
  $("#mcp").checked = s.mcp;
  $("#mcp-line").hidden = !s.mcp;
  $("#tunnel").checked = s.tunnel;
  $("#request-log").checked = s.request_log;
  pluginPages();
  renderMailCatcher();
  renderDbViewer();
  renderPresets();
  // .localhost works without setup; another suffix needs the resolver step once.
  const tld = $("#tld");
  tld.replaceChildren(new Option(".test", "test"), new Option(".localhost (no password needed)", "localhost"));
  if (!["test", "localhost"].includes(s.tld)) tld.add(new Option(`.${s.tld}`, s.tld));
  tld.value = s.tld;
  $("#tld-note").textContent =
    s.tld === "localhost"
      ? "Reaches this Mac by itself, no setup."
      : "Reaches Anvil through a resolver file, set up once with your password.";
  tld.onchange = () => call("set_tld", { tld: tld.value });
  $("#ports").textContent = `HTTP on ${s.http_port}${s.tld === "localhost" ? "" : `, DNS on ${s.dns_port}`}. Change them in config.toml.`;
  $("#config-dir").textContent = tilde(s.config_dir);

  // PHP versions
  const phpBusy = [...installing].some((k) => k.startsWith("php@"));
  $("#versions").replaceChildren(
    ...s.php.map((v) =>
      versionRow(
        `PHP ${v.version}`,
        v,
        installing.has(`php@${v.version}`),
        phpBusy,
        {
          install: async () => {
            installing.add(`php@${v.version}`);
            const done = phpAct(v.version, "php_install"); // clears the old errors first
            render();
            await done;
            installing.delete(`php@${v.version}`);
            render();
          },
          makeDefault: () => phpAct(v.version, "set_default_php"),
          uninstall: () => phpAct(v.version, "php_uninstall"),
          note: "Apps asking for it use the default.",
        },
        { ...phpParts(v), ...(verErrors.has(`php@${v.version}`) && { error: verError(`php@${v.version}`) }) },
      ),
    ),
  );

  for (const input of document.querySelectorAll<HTMLInputElement>("[data-ini]")) {
    if (document.activeElement !== input) input.value = s.ini[input.dataset.ini ?? ""] ?? "";
  }
}

const opcacheErrors = new Map<string, string>();
const extErrors = new Map<string, string>(); // version -> why its last extension change failed

const extsOpen = new Set<string>(); // PHP versions whose extension list is open

/** An installed version's "Extensions 2/6 ›" at the row's far end; it slides the version's
 * list open under the row (OPcache first, then the catalog's). After a failed build, the
 * reason under the row, with the tools when they are missing. */
function phpParts(v: Settings["php"][number]) {
  let end: HTMLElement | undefined;
  let below: HTMLElement | undefined;
  if (v.installed) {
    const on = v.extensions.filter((e) => e.on && e.built).length + (v.opcache === "missing" ? 0 : 1);
    const busy =
      v.opcache_building ||
      installing.has(`opcache@${v.version}`) ||
      v.extensions.some((e) => e.building || installing.has(`ext@${v.version}@${e.name}`));
    const t = Object.assign(document.createElement("button"), { type: "button", className: "ext-toggle" });
    // "Extensions" goes in a narrow row (style.css); the count and the tooltip stay.
    t.innerHTML = `<span><span class="ext-word">Extensions </span></span><span class="chevron" aria-hidden="true">›</span>`;
    t.firstElementChild!.append(`${on}/${v.extensions.length + 1}`);
    t.title = "Extensions";
    if (busy) t.prepend(Object.assign(document.createElement("span"), { className: "spin-dot" }));
    below = Object.assign(document.createElement("div"), { className: "ext-fold" });
    below.append(extensionsList(v));
    const set = (open: boolean) => {
      t.setAttribute("aria-expanded", String(open));
      below!.classList.toggle("open", open);
    };
    set(extsOpen.has(v.version));
    // Opened and closed in place, not redrawn, so it slides.
    t.onclick = () => {
      const open = !extsOpen.has(v.version);
      if (open) extsOpen.add(v.version);
      else extsOpen.delete(v.version);
      set(open);
    };
    end = t;
  }
  const reason = opcacheErrors.get(v.version) ?? extErrors.get(v.version);
  if (!reason) return { end, below };
  const error = Object.assign(document.createElement("div"), { className: "ver-error" });
  error.append(Object.assign(document.createElement("span"), { textContent: reason }));
  if (reason.includes("xcode-select --install")) error.append(button("Install command line tools", () => invoke("xcode_tools_install")));
  return { end, error, below };
}

/** A version's extensions, one line each. OPcache is built once for the version and then
 * always on: unbuilt, "Build" asks first (minutes). The others switch on for every app on the
 * version; one being built says so and holds its switch; one on without a build for this
 * PHP (a failed build, a PHP installed since) shows off, "Not built": on builds it. */
function extensionsList(v: Settings["php"][number]) {
  const list = Object.assign(document.createElement("div"), { className: "ext-list" });
  const op = Object.assign(document.createElement("div"), { className: "ext-line" });
  const opName = Object.assign(document.createElement("span"), { className: "ext-name", textContent: "OPcache" });
  const opEnd = Object.assign(document.createElement("span"), { className: "ext-end" });
  if (installing.has(`opcache@${v.version}`) || v.opcache_building) {
    opEnd.innerHTML = `<span class="spin-dot"></span><span class="muted">Building… a few minutes</span>`;
  } else if (v.opcache === "missing") {
    const idle = () =>
      opEnd.replaceChildren(
        Object.assign(document.createElement("span"), { className: "muted", textContent: "Not built" }),
        textAct("Build", () =>
          confirmInRow(opEnd, `Build OPcache for PHP ${v.version}? It takes a few minutes.`, "Build", () => addOpcache(v.version), idle),
        ),
      );
    idle();
  } else if (v.opcache === "ready") {
    // Built by Anvil: removable, asked first since building it again takes minutes.
    const idle = () =>
      opEnd.replaceChildren(
        Object.assign(document.createElement("span"), { className: "muted", textContent: "On" }),
        textAct("Remove", () =>
          confirmInRow(
            opEnd,
            `Remove OPcache from PHP ${v.version}? Building it again takes minutes.`,
            "Remove",
            () => removeOpcache(v.version),
            idle,
          ),
        ),
      );
    idle();
  } else opEnd.append(Object.assign(document.createElement("span"), { className: "muted", textContent: "Built in" }));
  op.append(opName, opEnd);
  list.append(op);
  for (const e of v.extensions) {
    const building = e.building || installing.has(`ext@${v.version}@${e.name}`);
    const unbuilt = e.on && !e.built && !building;
    const line = Object.assign(document.createElement("label"), { className: "ext-line" });
    const end = Object.assign(document.createElement("span"), { className: "ext-end" });
    if (building) end.append(Object.assign(document.createElement("span"), { className: "muted", textContent: "Building…" }));
    if (unbuilt) end.append(Object.assign(document.createElement("span"), { className: "muted", textContent: "Not built" }));
    const input = Object.assign(document.createElement("input"), { type: "checkbox", checked: e.on && !unbuilt, disabled: building });
    input.setAttribute("role", "switch");
    input.onchange = () => setExtension(v.version, e.name, input.checked);
    end.append(input);
    line.append(Object.assign(document.createElement("span"), { className: "ext-name", textContent: e.name }), end);
    list.append(line);
  }
  return list;
}

/** On builds the extension first unless it is built for this PHP (seconds to a minute),
 * then PHP-FPM restarts; off keeps the build. */
async function setExtension(version: string, name: string, on: boolean) {
  const key = `ext@${version}@${name}`;
  if (installing.has(key)) return;
  if (on) installing.add(key);
  extErrors.delete(version);
  render();
  try {
    s = (await invoke("php_ext", { version, name, on })) ?? s;
  } catch (e) {
    extErrors.set(version, String(e));
  }
  installing.delete(key);
  render();
}

/** The build takes minutes; the row says so meanwhile and holds a second click. */
async function addOpcache(version: string) {
  const key = `opcache@${version}`;
  if (installing.has(key)) return;
  installing.add(key);
  opcacheErrors.delete(version);
  render();
  try {
    s = (await invoke("php_opcache", { version })) ?? s;
  } catch (e) {
    opcacheErrors.set(version, String(e));
  }
  installing.delete(key);
  render();
}

async function removeOpcache(version: string) {
  opcacheErrors.delete(version);
  try {
    s = (await invoke("php_opcache_remove", { version })) ?? s;
  } catch (e) {
    opcacheErrors.set(version, String(e));
  }
  render();
}

/* ---------- Worktrees ---------- */

const editing = new Set<string>(); // presets open for editing

/** One row per preset with Edit; an open one shows its fields, saved as they change, and
 * Reset to default once it differs from Anvil's. */
function renderPresets() {
  const box = $("#presets");
  // A field being typed in stays as it is.
  if (box.contains(document.activeElement) && document.activeElement?.tagName === "INPUT") return;
  box.replaceChildren(
    ...s.worktree_presets.flatMap((p) => {
      const row = document.createElement("div");
      row.className = "row";
      const label = document.createElement("span");
      label.textContent = p.name;
      const small = document.createElement("small");
      small.textContent = [p.kinds.join(", ") || "Any other type", p.edited ? "edited" : ""].filter(Boolean).join(" · ");
      label.append(small);
      const open = editing.has(p.id);
      row.append(
        label,
        button(open ? "Done" : "Edit", () => {
          if (open) editing.delete(p.id);
          else editing.add(p.id);
          renderPresets();
        }),
      );
      if (!open) return [row];
      const save = (field: string | null, value?: unknown) =>
        invoke("set_worktree_preset", { id: p.id, field, value: value ?? null }).then((next) => {
          if (next) s = next;
          render();
        });
      // Not copy_database: the switch above is that layer, a preset leaves it to it.
      const fields: HTMLElement[] = (["copy", "keys", "after_create"] as const).map((field) => {
        const r = document.createElement("div");
        r.className = "row row-top preset-field";
        const t = document.createElement("span");
        t.textContent = NAMES[field];
        r.append(t, editor(field, p.hooks, { save }));
        return r;
      });
      if (p.edited) {
        const reset = document.createElement("div");
        reset.className = "row preset-field";
        const fail = document.createElement("small");
        fail.className = "error-text";
        fail.hidden = true;
        const why = document.createElement("span");
        why.append(fail);
        reset.append(
          why,
          button("Reset to default", () =>
            save(null).catch((e) => {
              fail.textContent = String(e);
              fail.hidden = false;
            }),
          ),
        );
        fields.push(reset);
      }
      return [row, ...fields];
    }),
  );
}

export function button(text, onclick) {
  const b = document.createElement("button");
  b.textContent = text;
  b.onclick = () => onclick(b);
  return b;
}

$("#add-path").onclick = async () => {
  const path = await window.__TAURI__.dialog.open({ directory: true, title: "Choose a folder of projects" });
  if (path) call("add_path", { path });
};
$("#login").onchange = (e) => call("set_login", { on: e.target.checked });
$("#dock").onchange = (e) => call("set_dock_icon", { on: e.target.checked });
$("#trim-logs").onchange = (e) => call("set_trim_app_logs", { on: e.target.checked });
$("#notifications").onchange = (e) => call("set_notifications", { on: e.target.checked });
$("#https-default").onchange = (e) => call("set_https_default", { on: e.target.checked });
$("#worktree-db").onchange = (e) => call("set_worktree_db", { choice: e.target.checked ? "copy" : "empty" });
$("#mcp").onchange = (e) => call("set_mcp", { on: e.target.checked });
$("#tunnel").onchange = (e) => call("set_tunnel", { on: e.target.checked });
$("#request-log").onchange = (e) => call("set_request_log", { on: e.target.checked });
$("#mail-catcher").onchange = (e) => call("set_mail_catcher", { on: e.target.checked });

/** A feature that is on has a page of its own under Features in the sidebar; turned off
 * while it's shown, the list of features takes its place. */
function pluginPages() {
  const on = { "plugin-mcp": s.mcp, "plugin-tunnel": s.tunnel, "plugin-mail": s.mail_catcher, "plugin-db": s.db_viewer || !!dbViewerBusy };
  for (const [page, shown] of Object.entries(on)) {
    const b = $(`.nav [data-page="${page}"]`);
    b.hidden = !shown;
    if (!shown && b.getAttribute("aria-current") === "true") $('.nav [data-page="plugins"]').click();
  }
}

/** Settings › Features › Database viewer: turning it on downloads Adminer first, said in
 * its own line; a failure shows there and leaves it off. */
let dbViewerBusy = "";
let dbViewerFail = "";
$("#db-viewer").onchange = async (e) => {
  const on = e.target.checked;
  dbViewerBusy = on ? "Downloading Adminer…" : "Turning off…";
  dbViewerFail = "";
  renderDbViewer();
  try {
    s = await invoke("set_db_viewer", { on });
  } catch (err) {
    dbViewerFail = String(err);
  }
  dbViewerBusy = "";
  render();
};
function renderDbViewer() {
  const sw = $("#db-viewer");
  sw.checked = dbViewerBusy ? sw.checked : s.db_viewer;
  sw.disabled = !!dbViewerBusy;
  $(".db-tld").textContent = s.tld;
  const line = $("#db-viewer-line");
  line.hidden = !s.db_viewer && !dbViewerBusy;
  $("span", line).textContent = dbViewerBusy || "Opens at";
  $("code", line).textContent = dbViewerBusy ? "" : s.db_viewer_host;
  const why = dbViewerFail || (s.db_viewer && s.db_viewer_problem) || "";
  $("#db-viewer-error").textContent = why;
  $("#db-viewer-error").hidden = !why;
}

/** Settings › Features › Mail catcher: where it listens, why it can't, and the inbox of
 * mail from no app, shown once there is some. */
let other: Inbox | null = null;
function renderMailCatcher() {
  $("#mail-catcher").checked = s.mail_catcher;
  $("#mail-line").hidden = !s.mail_catcher;
  $("#mail-line code").textContent = `127.0.0.1:${s.mail_port}`;
  $("#mail-error").textContent = s.mail_error ?? "";
  $("#mail-error").hidden = !s.mail_error;
  const box = $("#mail-other");
  if (!s.mail_catcher) {
    box.hidden = true;
    return;
  }
  if (!other) {
    other = inboxView("_other", invoke, "No messages from outside your apps.");
    box.append(other.node);
  }
  other.load().then(() => (box.hidden = !other!.node.querySelector(".mail-item")));
}
// Each Copy copies its row's command; "Copied" in the button for a moment, no toast.
for (const b of document.querySelectorAll<HTMLElement>(".mcp-copy"))
  b.onclick = async () => {
    await navigator.clipboard.writeText(b.closest(".row")!.querySelector("code")!.textContent!).catch(() => {});
    b.textContent = "Copied";
    setTimeout(() => (b.textContent = "Copy"), 1500);
  };
$("#theme").onchange = (e) => call("set_theme", { theme: e.target.value || null });
$("#editor").onchange = (e) => call("set_editor", { editor: e.target.value || null });
$("#terminal").onchange = (e) => call("set_terminal", { terminal: e.target.value || null });
$("#git-client").onchange = async (e) => {
  let app = e.target.value || null;
  if (app === "other") {
    app = await window.__TAURI__.dialog.open({
      title: "Choose the app that opens your repositories",
      defaultPath: "/Applications",
      filters: [{ name: "Apps", extensions: ["app"] }],
    });
    if (!app) return render();
  }
  call("set_git_client", { gitClient: app });
};
$("#open-config-about").onclick = () => invoke("open_config_dir").catch((e) => showError(String(e)));

/* ---------- About › Credits ---------- */

/** What Anvil downloads (when an app or a setting first needs it) or ships, hand-kept: name, what it's for, license
 * (SPDX where one exists), homepage, how it gets here. Each license was read from the
 * project's own LICENSE file (October 2026); check again when a source or version changes. */
const windows = document.documentElement.classList.contains("windows");

type Software = [string, string, string, string, string];
const SOFTWARE: Software[] = (<Software[]>[
  ["Caddy", "Web server and HTTPS certificates", "Apache-2.0", "https://caddyserver.com", "Downloaded when used"],
  windows
    ? ["PHP", "PHP apps, in windows.php.net’s builds; PHP 8.1–8.5", "PHP-3.01", "https://www.php.net", "Downloaded when used"]
    : ["PHP", "PHP apps, in static-php-cli’s builds (MIT); PHP 8.1–8.5", "PHP-3.01", "https://www.php.net", "Downloaded when used"],
  ["Composer", "PHP packages", "MIT", "https://getcomposer.org", "Downloaded when used"],
  ["Node.js", "JavaScript apps", "MIT", "https://nodejs.org", "Downloaded when used"],
  ["pnpm", "JavaScript packages", "MIT", "https://pnpm.io", "Downloaded when used"],
  ["Yarn", "JavaScript packages (Yarn 1)", "BSD-2-Clause", "https://classic.yarnpkg.com", "Downloaded when used"],
  ["Bun", "JavaScript packages; links JavaScriptCore, which is LGPL-2.0", "MIT", "https://bun.sh", "Downloaded when used"],
  ["Go", "Go apps", "BSD-3-Clause", "https://go.dev", "Downloaded when used"],
  ["Rust (rustup)", "Rust apps", "MIT OR Apache-2.0", "https://rustup.rs", "Downloaded when used"],
  [".NET SDK", ".NET apps", "MIT", "https://dotnet.microsoft.com", "Downloaded when used"],
  ["Python", "Python apps, in python-build-standalone’s builds (MPL-2.0)", "PSF-2.0", "https://www.python.org", "Downloaded when used"],
  ["uv", "Installs Python and its packages", "MIT OR Apache-2.0", "https://docs.astral.sh/uv", "Downloaded when used"],
  [
    "MySQL",
    "Database (Community Server), with the Universal FOSS Exception",
    "GPL-2.0-only",
    "https://www.mysql.com",
    "Downloaded when used",
  ],
  [
    "PostgreSQL",
    "Database, in theseus-rs’s builds (PostgreSQL license)",
    "PostgreSQL",
    "https://www.postgresql.org",
    "Downloaded when used",
  ],
  ...((windows
    ? [
        [
          "Garnet",
          "Cache, Redis-compatible: Redis has no Windows build",
          "MIT",
          "https://microsoft.github.io/garnet",
          "Downloaded when used",
        ],
        [".NET Runtime", "Runs Garnet", "MIT", "https://dotnet.microsoft.com", "Downloaded when used"],
      ]
    : [
        [
          "Redis",
          "Cache. 8.x: your choice of the three; 7.4: RSALv2 or SSPL-1.0; 7.2: BSD-3-Clause",
          "RSALv2 / SSPL-1.0 / AGPL-3.0",
          "https://redis.io",
          "Built on this Mac",
        ],
      ]) as [string, string, string, string, string][]),
  ["Mailpit", "Catches the mail apps send", "MIT", "https://mailpit.axllent.org", "Downloaded when used"],
  ["Versity S3 Gateway", "S3-compatible file storage", "Apache-2.0", "https://github.com/versity/versitygw", "Downloaded when used"],
  ["SILO", "S3-compatible file storage, a MinIO fork", "AGPL-3.0", "https://silo.pgsty.com", "Downloaded when used"],
  [
    "Tauri",
    windows ? "The app’s windows and tray icon" : "The app’s windows and menu bar",
    "MIT OR Apache-2.0",
    "https://tauri.app",
    "Bundled",
  ],
  // Python and Rust aren't on Windows yet.
]).filter(([name]) => !windows || !["Rust (rustup)", "Python", "uv"].includes(name));

const node = (tag: string, cls: string, ...kids: (Node | string)[]) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  n.append(...kids);
  return n;
};

type Performance = {
  dev_drive_supported: boolean;
  folders: { name: string; path: string; dev_drive: boolean | null }[];
};

/** Settings › Performance (Windows): asked each time it opens, as a drive can change outside Anvil. */
function loadPerformance() {
  const row = (label: string, note: string, value: string) =>
    node("div", "row", node("span", "", label, node("small", "", note)), node("span", "muted", value));
  invoke<Performance>("performance")
    .then((p) => {
      $("#perf-devdrive").textContent = p.dev_drive_supported ? "· Dev Drive available" : "· Dev Drive needs Windows 11";
      $("#perf-guide").hidden = !p.dev_drive_supported;
      $("#perf-folders").replaceChildren(
        ...p.folders.map((f) =>
          row(f.name, f.path, f.dev_drive ? "On a Dev Drive" : f.dev_drive === false ? "Not on a Dev Drive" : "Unknown"),
        ),
      );
    })
    .catch((e) => showError(String(e)));
}

const guide = $("#perf-guide") as HTMLElement;
guide.onclick = () => confirmOpen(guide, "https://learn.microsoft.com/windows/dev-drive/");

/** A bubble under a credit's name: its site opens only on Open, so a stray click
 * while reading the list doesn't throw a browser window up. */
function confirmOpen(anchor: HTMLElement, url: string) {
  document.querySelector(".pop")?.remove();
  const yes = node("button", "primary", "Open") as HTMLButtonElement;
  const no = node("button", "", "Cancel") as HTMLButtonElement;
  const pop = node("div", "pop pop-confirm", node("span", "", new URL(url).host), no, yes);
  pop.setAttribute("role", "dialog");
  pop.setAttribute("aria-label", `Open ${new URL(url).host}`);
  const close = () => {
    const inside = pop.contains(document.activeElement);
    pop.remove();
    document.removeEventListener("mousedown", outside, true);
    document.removeEventListener("scroll", close, true);
    if (inside) anchor.focus();
  };
  const outside = (e: MouseEvent) => !pop.contains(e.target as Node) && close();
  bubbleKeys(pop, close);
  yes.onclick = () => {
    close();
    invoke("open_url", { url }).catch((err) => showError(String(err)));
  };
  no.onclick = close;
  const r = anchor.getBoundingClientRect();
  Object.assign(pop.style, { top: `${r.bottom + window.scrollY + 6}px`, left: `${r.left + window.scrollX}px` });
  document.body.append(pop);
  document.addEventListener("mousedown", outside, true);
  // The list scrolls inside its pane; a bubble left behind would point at nothing.
  document.addEventListener("scroll", close, true);
  yes.focus();
}

$("#credits-software").replaceChildren(
  ...SOFTWARE.map(([name, use, license, url, how]) => {
    const link = node("a", "", name) as HTMLAnchorElement;
    link.href = url;
    link.onclick = (e) => {
      e.preventDefault();
      confirmOpen(link, url);
    };
    return node(
      "div",
      "row credit",
      node("span", "", link, node("small", "", use)),
      node("span", "credit-meta", license, node("small", "", how)),
    );
  }),
);

type Credit = { name: string; version: string; license: string };

// Both files are written at build time by `pnpm credits` (scripts/credits.mjs).
const missing = () => node("p", "credit-missing muted", "Not in this build: `pnpm credits` writes it.");

/** The library list, read when About first opens. */
let creditsLoaded = false;
function loadCredits() {
  if (creditsLoaded) return;
  creditsLoaded = true;
  fetch("credits.json")
    .then((r) => (r.ok ? r.json() : Promise.reject()))
    .then(({ rust, npm }: { rust: Credit[]; npm: Credit[] }) => {
      const rows = (kind: string, list: Credit[]) =>
        list.map((c) =>
          node(
            "div",
            "credit-lib",
            node("span", "", c.name),
            node("span", "muted", c.version),
            node("span", "muted", c.license),
            node("span", "muted", kind),
          ),
        );
      $("#credits-list").replaceChildren(...rows("crate", rust), ...rows("npm", npm));
      $("#credits-count").textContent = String(rust.length + npm.length);
    })
    .catch(() => $("#credits-list").replaceChildren(missing()));
}

// 1 MB of text: read the first time it opens.
const notices = $("#credits-notices") as HTMLDetailsElement;
notices.addEventListener("toggle", () => {
  if (!notices.open || $("#credits-text").textContent) return;
  fetch("THIRD_PARTY_NOTICES.txt")
    .then((r) => (r.ok ? r.text() : Promise.reject()))
    .then((t) => ($("#credits-text").textContent = t))
    .catch(() => $("#credits-text").replaceChildren(missing()));
});
window.__TAURI__.app?.getVersion().then((v) => ($("#version").textContent = `Version ${v}`));

$("#update-check").onclick = () => invoke("show_updates");
$("#open-config").onclick = () => invoke("open_config_dir").catch((e) => showError(String(e)));

for (const input of document.querySelectorAll<HTMLInputElement>("[data-ini]")) {
  input.onchange = async () => {
    const value = input.value.trim() || null;
    await call("set_ini", { site: null, key: input.dataset.ini, value });
    // An upload can't be bigger than the POST body that carries it.
    if (input.dataset.ini === "upload_max_filesize") {
      await call("set_ini", { site: null, key: "post_max_size", value });
    }
  };
}

// Storage: Anvil's own files or the applications', one at a time.
function showStorage(which) {
  localStorage.setItem("storageTab", which);
  for (const b of document.querySelectorAll<HTMLElement>("[data-storage]"))
    b.setAttribute("aria-selected", String(b.dataset.storage === which));
  for (const p of document.querySelectorAll<HTMLElement>("[data-storage-panel]")) p.hidden = p.dataset.storagePanel !== which;
}
for (const b of document.querySelectorAll<HTMLElement>("[data-storage]")) b.onclick = () => showStorage(b.dataset.storage);
showStorage(localStorage.getItem("storageTab") === "apps" ? "apps" : "anvil");

for (const b of document.querySelectorAll<HTMLElement>("[data-page]")) {
  b.onclick = () => {
    for (const other of document.querySelectorAll<HTMLElement>("[data-page]")) other.setAttribute("aria-current", String(other === b));
    for (const page of document.querySelectorAll<HTMLElement>(".page")) page.hidden = page.id !== b.dataset.page;
    if (LANGS.some(([id]) => id === b.dataset.page) && !runtimes) loadRuntimes();
    if (b.dataset.page === "node") loadPackageManagers();
    if (b.dataset.page === "about") loadCredits();
    if (b.dataset.page === "performance") loadPerformance();
    if (b.dataset.page === "storage") {
      loadAnvilStorage();
      loadPnpmStore();
      loadAppSizes();
    }
  };
}

document.addEventListener("contextmenu", (e) => {
  if (!(e.target as HTMLElement).closest("input, textarea")) e.preventDefault();
});

window.refresh();
