import type { Settings } from "./types";

export const { invoke } = window.__TAURI__.core;

export const $ = (sel: string, root: ParentNode = document): any => root.querySelector(sel);
export const tilde = (p) => p.replace(/^\/Users\/[^/]+/, "~");
export let s: Settings = null as unknown as Settings; // set by the first load

export async function call(cmd: string, args?: Record<string, unknown>) {
  try {
    s = (await invoke(cmd, args)) ?? s;
    showError(null);
    render();
  } catch (e) {
    showError(String(e));
  }
}

export function showError(msg) {
  $("#error").textContent = msg ?? "";
  $("#error").hidden = !msg;
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
export const LANGS = [
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
        `<p class="hint note">Anvil starts Node apps with npm from the app’s Node version. A script that calls pnpm, yarn or bun gets the first one found on the app’s PATH, shown here.</p>`,
    );
  }
  $("#storage").before(page);
}

/** Newest first, numerically: "22" > "9", "1.27.2" > "1.9". */
const newest = (a: string, b: string) => b.localeCompare(a, undefined, { numeric: true });

/** One row of a Versions list: muted with Install when missing, a check and
 * "Default" on the default, Make default and Uninstall on hover otherwise. */
export function versionRow(
  label: string,
  v: { installed: boolean; default?: boolean },
  busy: boolean,
  blocked: boolean,
  act: { install: () => void; uninstall: () => void; makeDefault?: () => void },
) {
  const row = document.createElement("div");
  row.className = `row ver${v.installed ? "" : " missing"}${v.default ? " is-default" : ""}`;
  const name = document.createElement("span");
  name.className = "ver-name";
  name.innerHTML = `<span class="ver-mark">${v.default ? CHECK : ""}</span><span class="ver-label"></span>`;
  $(".ver-label", name).textContent = label;
  if (v.default) name.append(Object.assign(document.createElement("span"), { className: "ver-default", textContent: "Default" }));
  const actions = document.createElement("span");
  actions.className = "actions-inline";
  if (busy) {
    actions.innerHTML = `<span class="spin-dot"></span><span class="muted">Installing…</span>`;
  } else if (!v.installed) {
    const b = button("Install", act.install);
    b.disabled = blocked;
    actions.append(b);
  } else if (!v.default) {
    actions.classList.add("on-hover");
    if (act.makeDefault) actions.append(button("Make default", act.makeDefault));
    const u = button("Uninstall", act.uninstall);
    u.title = `Uninstall ${label}`;
    actions.append(u);
  }
  row.append(name, actions);
  return row;
}

export async function loadRuntimes() {
  try {
    runtimes = await invoke("runtimes");
    showError(null);
  } catch (e) {
    showError(String(e));
  }
  renderLanguages();
}

export function renderLanguages() {
  for (const rt of runtimes ?? []) {
    const list = $(`#${rt.id} .versions`);
    if (!list) continue;
    const all = [...new Set<string>([...rt.available, ...rt.installed])].sort(newest);
    const blocked = [...installing].some((k) => k.startsWith(`${rt.id}@`));
    list.replaceChildren(
      ...all.map((v) =>
        versionRow(`${rt.name} ${v}`, { installed: rt.installed.includes(v) }, installing.has(`${rt.id}@${v}`), blocked, {
          install: () => installRuntime(rt.id, v),
          uninstall: () => uninstallRuntime(rt, v),
        }),
      ),
    );
    if (!all.length) list.innerHTML = `<div class="row"><span class="muted">No versions found. Check the internet connection.</span></div>`;
  }
}

export async function installRuntime(id: string, version: string) {
  const k = `${id}@${version}`;
  installing.add(k);
  renderLanguages();
  try {
    await invoke("runtime_install", { id, version });
    showError(null);
  } catch (e) {
    showError(String(e));
  }
  installing.delete(k);
  loadRuntimes();
}

export async function uninstallRuntime(rt, v: string) {
  const ok = await window.__TAURI__.dialog.ask(
    `Applications using it switch to another installed version, or stop starting until one is installed.`,
    { title: `Uninstall ${rt.name} ${v}?`, kind: "warning", okLabel: "Uninstall", cancelLabel: "Cancel" },
  );
  if (!ok) return;
  try {
    await invoke("runtime_uninstall", { id: rt.id, version: v });
  } catch (e) {
    showError(String(e));
  }
  loadRuntimes();
}

/** npm, pnpm, yarn and bun: found or not, where from and which version. Read-only. */
export async function loadPackageManagers() {
  let pms;
  try {
    pms = await invoke("package_managers");
  } catch (e) {
    return showError(String(e));
  }
  $("#pms").replaceChildren(
    ...pms.map((pm) => {
      const row = document.createElement("div");
      row.className = "row pm";
      row.innerHTML = `<span class="pm-name"></span><span class="pm-from"></span><span class="pm-version"></span>`;
      $(".pm-name", row).textContent = pm.name;
      const from = $(".pm-from", row);
      if (pm.path) {
        from.textContent = pm.source?.startsWith("Node.js") ? `Bundled with ${pm.source}` : pm.source;
        from.title = tilde(pm.path);
        if (pm.source === "System") from.append(Object.assign(document.createElement("small"), { textContent: ` ${tilde(pm.path)}` }));
      } else {
        from.textContent = "Not found";
        row.classList.add("missing");
      }
      $(".pm-version", row).textContent = pm.version ?? (pm.path ? "unknown" : "");
      return row;
    }),
  );
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
export function sizeColor(n) {
  const lo = Math.log10(10 * 1024 ** 2);
  const hi = Math.log10(20 * 1024 ** 3);
  const t = Math.min(1, Math.max(0, (Math.log10(Math.max(n, 1)) - lo) / (hi - lo)));
  // A gentler start, so red is for the really big ones.
  return `color-mix(in oklab, var(--danger) ${Math.round(t ** 1.4 * 100)}%, var(--text))`;
}

export const SAFETY_LABEL = { safe: "Safe to delete", rebuild: "Rebuildable", keep: "Keep" };

export async function confirmDelete(title, message) {
  return window.__TAURI__.dialog.ask(message, { title, kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" });
}

export async function loadAnvilStorage() {
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

export function drawAnvilItem(list, i, withNote) {
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
  actions.append(size);
  if (i.safety === "safe") {
    actions.append(
      button("Delete", async () => {
        if (!(await confirmDelete(`Delete ${i.label}?`, `${i.note}. This frees about ${bytes(i.bytes)}.`))) return;
        try {
          await invoke("clean_anvil", { path: i.path });
        } catch (e) {
          showError(String(e));
        }
        loadAnvilStorage();
      }),
    );
  }
  row.append(info, actions);
  list.append(row);
}

/* Application sizes: the last measurement shows at once, and a new one fills
 * in row by row as each app is counted. The toolbar and the list hold their
 * place from the start: while measuring the refresh icon spins, Delete waits
 * disabled, and placeholder rows keep the list's height until numbers arrive. */
export const sizes = new Map<string, any>(); // app name -> { name, kind, total, safe, rebuild }
export let measuring = false;
export let loading = true; // the last measurement not read yet
export let deleting = false;
export let measuredAt = 0; // Unix seconds of the shown numbers
export let drawQueued = false;
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

export async function loadAppSizes() {
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

export async function scanApps() {
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

export const DELETE_ALL = "Delete Logs and Caches in All Apps";

export function drawSizes() {
  const apps = [...sizes.values()].sort((a, b) => b.total - a.total);
  const sum = (k) => apps.reduce((a, x) => a + x[k], 0);
  const busy = measuring || loading;
  $("#apps-size").textContent = apps.length ? bytes(sum("total")) : "";
  $("#apps-hint").textContent = !apps.length
    ? "Measuring every application…"
    : `${apps.length} applications use ${bytes(sum("total"))}. ${bytes(sum("safe"))} is logs and caches that can be deleted safely; ` +
      `${bytes(sum("rebuild"))} is dependencies and builds that can be recreated.`;

  const scan = $("#scan-apps");
  scan.disabled = busy || deleting;
  scan.classList.toggle("spinning", busy);
  $("#measured").textContent = busy ? "Measuring…" : measuredAt ? `Measured ${agoText(measuredAt)}` : "";

  const all = $("#clean-all-safe");
  const safe = sum("safe");
  all.disabled = busy || deleting || !safe;
  all.replaceChildren(busy || deleting || !safe ? DELETE_ALL : `${DELETE_ALL} (${bytes(safe)})`);
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
  if (!busy && !shown.length) list.innerHTML = `<div class="row"><span class="muted">No applications take any space.</span></div>`;
}

export function agoText(secs) {
  const s = Date.now() / 1000 - secs;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}

$("#scan-apps").onclick = scanApps;
$("#clean-all-safe").onclick = async () => {
  const safe = [...sizes.values()].reduce((a, x) => a + x.safe, 0);
  const ok = await confirmDelete(
    "Delete logs and caches in all applications?",
    `Removes application logs, compiled views and caches (sessions too, which logs users out) in every app. Folders and .gitignore files stay. About ${bytes(safe)}.`,
  );
  if (!ok) return;
  deleting = true;
  drawSizes();
  try {
    const [freed, failed] = await invoke("clean_all_safe");
    showError(failed.length ? `Freed ${bytes(freed)}. Couldn't clean: ${[...new Set(failed)].join(", ")}` : null);
  } catch (e) {
    showError(String(e));
  }
  deleting = false;
  scanApps();
};

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
      row.append(
        label,
        button("Show", async () => {
          await invoke("unhide_site", { site: name }).catch((e) => showError(String(e)));
          call("settings");
        }),
      );
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
    row.innerHTML = `<span class="path"></span><button>Remove</button>`;
    $(".path", row).textContent = tilde(p);
    $("button", row).onclick = () => call("remove_path", { path: p });
    paths.append(row);
  }

  $("#login").checked = s.login;
  $("#dock").checked = s.dock_icon;
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
      versionRow(`PHP ${v.version}`, v, installing.has(`php@${v.version}`), phpBusy, {
        install: async () => {
          installing.add(`php@${v.version}`);
          render();
          await call("php_install", { version: v.version });
          installing.delete(`php@${v.version}`);
          render();
        },
        makeDefault: () => call("set_default_php", { version: v.version }),
        uninstall: async () => {
          const ok = await window.__TAURI__.dialog.ask("Applications pinned to it fall back to the default PHP version.", {
            title: `Uninstall PHP ${v.version}?`,
            kind: "warning",
            okLabel: "Uninstall",
            cancelLabel: "Cancel",
          });
          if (ok) call("php_uninstall", { version: v.version });
        },
      }),
    ),
  );

  for (const input of document.querySelectorAll<HTMLInputElement>("[data-ini]")) {
    if (document.activeElement !== input) input.value = s.ini[input.dataset.ini ?? ""] ?? "";
  }
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
$("#https-default").onchange = (e) => call("set_https_default", { on: e.target.checked });
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
window.__TAURI__.app?.getVersion().then((v) => ($("#version").textContent = `Version ${v}`));
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
export function showStorage(which) {
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
    if (b.dataset.page === "storage") {
      loadAnvilStorage();
      loadAppSizes();
    }
  };
}

document.addEventListener("contextmenu", (e) => {
  if (!(e.target as HTMLElement).closest("input, textarea")) e.preventDefault();
});

window.refresh();
