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
export const installing = new Set<string>(); // "id@version"

export async function loadRuntimes() {
  try {
    runtimes = await invoke("runtimes");
    showError(null);
  } catch (e) {
    showError(String(e));
  }
  renderRuntimes();
}

export function renderRuntimes() {
  if (!runtimes) return;
  const root = $("#runtime-list");
  root.replaceChildren();
  for (const rt of runtimes) {
    const row = document.createElement("div");
    row.className = "row runtime";
    const info = document.createElement("span");
    const title = document.createElement("strong");
    title.textContent = rt.name;
    const versions = document.createElement("span");
    versions.className = "versions";
    if (!rt.installed.length) {
      const none = document.createElement("small");
      none.textContent = "Not installed";
      versions.append(none);
    }
    for (const v of rt.installed) {
      const chip = document.createElement("span");
      chip.className = "chip";
      chip.textContent = v;
      const x = document.createElement("button");
      x.textContent = "×";
      x.title = `Uninstall ${rt.name} ${v}`;
      x.setAttribute("aria-label", String(x.title));
      x.onclick = async () => {
        const ok = await window.__TAURI__.dialog.ask(
          `Applications using it switch to another installed version, or stop starting until one is installed.`,
          {
            title: `Uninstall ${rt.name} ${v}?`,
            kind: "warning",
            okLabel: "Uninstall",
            cancelLabel: "Cancel",
          },
        );
        if (!ok) return;
        try {
          await invoke("runtime_uninstall", { id: rt.id, version: v });
        } catch (e) {
          showError(String(e));
        }
        loadRuntimes();
      };
      chip.append(x);
      versions.append(chip);
    }
    info.append(title, versions);

    const actions = document.createElement("span");
    actions.className = "actions-inline";
    const pick = document.createElement("select");
    pick.setAttribute("aria-label", String(`${rt.name} version to install`));
    for (const v of rt.available.filter((v) => !rt.installed.includes(v))) pick.add(new Option(v, v));
    const key = () => `${rt.id}@${pick.value}`;
    const install = button("Install", async () => {
      const k = key();
      installing.add(k);
      renderRuntimes();
      try {
        await invoke("runtime_install", { id: rt.id, version: k.split("@")[1] });
        showError(null);
      } catch (e) {
        showError(String(e));
      }
      installing.delete(k);
      loadRuntimes();
    });
    const busyHere = [...installing].some((k) => k.startsWith(`${rt.id}@`));
    if (busyHere) {
      install.textContent = "Installing…";
      install.disabled = true;
      pick.disabled = true;
    }
    if (pick.options.length) actions.append(pick, install);
    row.append(info, actions);
    root.append(row);
  }
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
 * in row by row as each app is counted. */
export const sizes = new Map<string, any>(); // app name -> { name, kind, total, safe, rebuild }
export let measuring = false;
export let measuredAt = 0; // Unix seconds of the shown numbers
export let drawQueued = false;

window.__TAURI__.event.listen("storage-app", (e) => {
  sizes.set(e.payload.name, e.payload);
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
  const last = await invoke("storage_last").catch(() => null);
  if (last && !sizes.size) {
    measuredAt = last[0];
    for (const a of last[1]) sizes.set(a.name, a);
  }
  drawSizes();
  // Numbers younger than ten minutes are good enough; Measure Again is there.
  if (!last || Date.now() / 1000 - last[0] > 600) scanApps();
}

export async function scanApps() {
  measuring = true;
  const fresh = new Set();
  const stop = window.__TAURI__.event.listen("storage-app", (e) => fresh.add(e.payload.name));
  drawSizes();
  try {
    await invoke("storage_apps");
    // Apps that are gone since the last measurement drop out.
    for (const name of sizes.keys()) if (!fresh.has(name)) sizes.delete(name);
    measuredAt = Date.now() / 1000;
  } catch (e) {
    showError(String(e));
  }
  (await stop)();
  measuring = false;
  drawSizes();
}

export function drawSizes() {
  const apps = [...sizes.values()].sort((a, b) => b.total - a.total);
  const sum = (k) => apps.reduce((a, x) => a + x[k], 0);
  const when = measuredAt ? agoText(measuredAt) : "";
  $("#apps-size").textContent = apps.length ? bytes(sum("total")) : "";
  $("#apps-hint").textContent = !apps.length
    ? "Measuring every application…"
    : `${apps.length} applications use ${bytes(sum("total"))}. ${bytes(sum("safe"))} is logs and caches that can be deleted safely; ` +
      `${bytes(sum("rebuild"))} is dependencies and builds that can be recreated. ` +
      (measuring ? "Measuring again…" : `Measured ${when}.`);
  $("#scan-apps").hidden = measuring;

  const all = $("#clean-all-safe");
  all.hidden = measuring || !sum("safe");
  all.textContent = `Delete Logs and Caches in All Apps (${bytes(sum("safe"))})`;
  all.onclick = async () => {
    const ok = await confirmDelete(
      "Delete logs and caches in all applications?",
      `Removes application logs, compiled views and caches (sessions too, which logs users out) in every app. Folders and .gitignore files stay. About ${bytes(sum("safe"))}.`,
    );
    if (!ok) return;
    try {
      const [freed, failed] = await invoke("clean_all_safe");
      showError(failed.length ? `Freed ${bytes(freed)}. Couldn't clean: ${[...new Set(failed)].join(", ")}` : null);
    } catch (e) {
      showError(String(e));
    }
    scanApps();
  };

  const list = $("#app-sizes");
  list.hidden = !apps.length;
  list.replaceChildren();
  for (const a of apps.filter((a) => a.total > 0)) {
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
}

export function agoText(secs) {
  const s = Date.now() / 1000 - secs;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}

$("#scan-apps").onclick = scanApps;

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
  $("#trim-logs").checked = s.trim_app_logs;
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
  const versions = $("#versions");
  versions.replaceChildren();
  for (const v of s.php) {
    const row = document.createElement("div");
    row.className = "row";
    const label = document.createElement("span");
    label.textContent = `PHP ${v.version}`;
    const actions = document.createElement("span");
    actions.className = "actions-inline";
    if (v.default) {
      const tag = document.createElement("span");
      tag.className = "tag";
      tag.textContent = "Default";
      actions.append(tag);
    } else if (v.installed) {
      actions.append(
        button("Make default", () => call("set_default_php", { version: v.version })),
        button("Uninstall", async () => {
          const ok = await window.__TAURI__.dialog.ask("Applications pinned to it fall back to the default PHP version.", {
            title: `Uninstall PHP ${v.version}?`,
            kind: "warning",
            okLabel: "Uninstall",
            cancelLabel: "Cancel",
          });
          if (ok) call("php_uninstall", { version: v.version });
        }),
      );
    } else {
      actions.append(
        button("Install", async (b) => {
          b.disabled = true;
          b.textContent = "Installing…";
          await call("php_install", { version: v.version });
        }),
      );
    }
    row.append(label, actions);
    versions.append(row);
  }

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
$("#trim-logs").onchange = (e) => call("set_trim_app_logs", { on: e.target.checked });
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
    if (b.dataset.page === "languages" && !runtimes) loadRuntimes();
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
