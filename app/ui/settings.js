const { invoke } = window.__TAURI__.core;

const $ = (sel, root = document) => root.querySelector(sel);
const tilde = (p) => p.replace(/^\/Users\/[^/]+/, "~");
let s = null;

async function call(cmd, args) {
  try {
    s = (await invoke(cmd, args)) ?? s;
    showError(null);
    render();
  } catch (e) {
    showError(String(e));
  }
}

function showError(msg) {
  $("#error").textContent = msg ?? "";
  $("#error").hidden = !msg;
}

window.refresh = () => {
  call("settings");
  loadServices();
};

/* ---------- Languages ---------- */

let runtimes = null;
const installing = new Set(); // "id@version"

async function loadRuntimes() {
  try {
    runtimes = await invoke("runtimes");
    showError(null);
  } catch (e) {
    showError(String(e));
  }
  renderRuntimes();
}

function renderRuntimes() {
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
      x.setAttribute("aria-label", x.title);
      x.onclick = async () => {
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
    pick.setAttribute("aria-label", `${rt.name} version to install`);
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

function bytes(n) {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n >= 10 || i === 0 ? Math.round(n) : n.toFixed(1)} ${u[i]}`;
}

const SAFETY_LABEL = { safe: "Safe to delete", rebuild: "Rebuildable", keep: "Keep" };

async function confirmDelete(title, message) {
  return window.__TAURI__.dialog.ask(message, { title, kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" });
}

async function loadAnvilStorage() {
  let st;
  try {
    st = await invoke("storage_anvil");
  } catch (e) {
    return showError(String(e));
  }
  $("#anvil-total").textContent = `Anvil uses ${bytes(st.total)} for PHP, languages, services, data, caches and logs.`;
  const list = $("#anvil-items");
  list.replaceChildren();
  for (const i of st.items) {
    const row = document.createElement("div");
    row.className = "row";
    const info = document.createElement("span");
    const name = document.createElement("span");
    name.textContent = i.label;
    const note = document.createElement("small");
    note.textContent = i.note;
    info.append(name, note);
    const actions = document.createElement("span");
    actions.className = "actions-inline";
    const tag = document.createElement("span");
    tag.className = `safety ${i.safety}`;
    tag.textContent = SAFETY_LABEL[i.safety];
    const size = document.createElement("span");
    size.className = "size";
    size.textContent = bytes(i.bytes);
    actions.append(tag, size);
    if (i.safety === "safe") {
      actions.append(button("Delete", async () => {
        if (!(await confirmDelete(`Delete ${i.label}?`, `${i.note}. This frees about ${bytes(i.bytes)}.`))) return;
        try {
          await invoke("clean_anvil", { path: i.path });
        } catch (e) {
          showError(String(e));
        }
        loadAnvilStorage();
      }));
    }
    row.append(info, actions);
    list.append(row);
  }
}

async function scanApps() {
  const btn = $("#scan-apps");
  btn.disabled = true;
  btn.textContent = "Measuring…";
  let apps;
  try {
    apps = await invoke("storage_apps");
  } catch (e) {
    showError(String(e));
  }
  btn.disabled = false;
  btn.textContent = "Measure again";
  if (!apps) return;

  const sum = (k) => apps.reduce((a, x) => a + x[k], 0);
  $("#apps-hint").textContent =
    `${apps.length} applications use ${bytes(sum("total"))}. ${bytes(sum("safe"))} is logs and caches that can be deleted safely; ` +
    `${bytes(sum("rebuild"))} is dependencies and builds that can be recreated. Open an application to clean it item by item.`;

  const all = $("#clean-all-safe");
  all.hidden = !sum("safe");
  all.textContent = `Delete logs and caches in all apps (${bytes(sum("safe"))})`;
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
  list.hidden = false;
  list.replaceChildren();
  for (const a of apps.filter((a) => a.total > 0)) {
    const row = document.createElement("div");
    row.className = "row";
    const info = document.createElement("span");
    const name = document.createElement("span");
    name.textContent = a.name;
    const note = document.createElement("small");
    const parts = [];
    if (a.rebuild) parts.push(`${bytes(a.rebuild)} rebuildable`);
    if (a.safe) parts.push(`${bytes(a.safe)} safe to delete`);
    note.textContent = parts.join(", ") || "Mostly source and data";
    info.append(name, note);
    const size = document.createElement("span");
    size.className = "size";
    size.textContent = bytes(a.total);
    row.append(info, size);
    list.append(row);
  }
}

$("#scan-apps").onclick = scanApps;

/* ---------- Services ---------- */

let services = [];
const busy = new Set(); // service ids with an install in progress

async function serviceCall(cmd, args) {
  try {
    services = await invoke(cmd, args);
    showError(null);
  } catch (e) {
    showError(String(e));
  }
  renderServices();
}

const loadServices = () => serviceCall("services");

function renderServices() {
  const root = $("#service-list");
  root.replaceChildren();
  if (!services.length) {
    const p = document.createElement("p");
    p.className = "empty";
    p.textContent = "No services yet. Add MySQL, PostgreSQL, Redis or Mailpit; each one gets its own port and data.";
    root.append(p);
  }
  const categories = [...new Set(services.map((x) => x.category))];
  for (const cat of categories) {
    const h = document.createElement("h2");
    h.textContent = cat;
    const group = document.createElement("div");
    group.className = "group";
    for (const svc of services.filter((x) => x.category === cat)) group.append(serviceRow(svc));
    root.append(h, group);
  }
}

function serviceRow(svc) {
  const row = document.createElement("div");
  row.className = "row service";

  const info = document.createElement("span");
  const title = document.createElement("span");
  title.className = "service-title";
  title.innerHTML = `<span class="dot" aria-hidden="true"></span><strong></strong><span class="muted"></span>`;
  $(".dot", title).classList.toggle("on", svc.running);
  $("strong", title).textContent = svc.id;
  $(".muted", title).textContent = `${svc.name} ${svc.installed ? svc.version : ""}`;
  const status = document.createElement("small");
  if (busy.has(svc.id)) status.textContent = svc.kind === "redis" ? "Building from source, about a minute…" : "Downloading…";
  else if (svc.error) status.textContent = svc.error;
  else if (svc.running) status.textContent = `Running on 127.0.0.1:${svc.port}`;
  else if (svc.installed) status.textContent = "Stopped";
  else status.textContent = "Not installed";
  if (svc.error) status.className = "bad";
  info.append(title, status);
  if (svc.suggested_port && !svc.running) {
    const fix = button(`Use port ${svc.suggested_port}`, () =>
      serviceCall("set_service_port", { id: svc.id, port: svc.suggested_port }));
    fix.className = "link";
    info.append(fix);
  }
  if (svc.suggested_web_port && !svc.running) {
    const fix = button(`Use inbox port ${svc.suggested_web_port}`, () =>
      serviceCall("set_service_port", { id: svc.id, port: svc.suggested_web_port, web: true }));
    fix.className = "link";
    info.append(fix);
  }

  const actions = document.createElement("span");
  actions.className = "actions-inline";
  if (!svc.installed) {
    const b = button(busy.has(svc.id) ? "Installing…" : "Install", async () => {
      busy.add(svc.id);
      renderServices();
      await serviceCall("install_service", { id: svc.id });
      busy.delete(svc.id);
      renderServices();
    });
    b.disabled = busy.has(svc.id);
    actions.append(b);
  } else {
    if (svc.web && svc.running) actions.append(button("Open inbox", () => invoke("open_url", { url: svc.web })));
    const port = document.createElement("input");
    port.value = svc.port;
    port.inputMode = "numeric";
    port.setAttribute("aria-label", `${svc.id} port`);
    port.onchange = () => {
      const n = parseInt(port.value, 10);
      serviceCall("set_service_port", { id: svc.id, port: Number.isNaN(n) ? null : n });
    };
    actions.append(port);
    if (svc.web_port) {
      const web = document.createElement("input");
      web.value = svc.web_port;
      web.placeholder = svc.web_port;
      web.title = "Inbox port";
      web.inputMode = "numeric";
      web.setAttribute("aria-label", `${svc.name} inbox port`);
      web.onchange = () => {
        const n = parseInt(web.value, 10);
        serviceCall("set_service_port", { id: svc.id, port: Number.isNaN(n) ? null : n, web: true });
      };
      actions.append(web);
    }
    const toggle = document.createElement("input");
    toggle.type = "checkbox";
    toggle.setAttribute("role", "switch");
    toggle.setAttribute("aria-label", `Run ${svc.id}`);
    toggle.checked = svc.running || (svc.enabled && !svc.error);
    toggle.onchange = () => serviceCall("set_service", { id: svc.id, on: toggle.checked });
    actions.append(toggle);
  }
  actions.append(button("Remove", () => removeService(svc)));
  row.append(info, actions);
  return row;
}

/** Remove a service; a second question decides whether its data goes too. */
async function removeService(svc) {
  const { ask } = window.__TAURI__.dialog;
  const sure = await ask(`${svc.id} stops and is removed from Anvil.`, {
    title: `Remove ${svc.id}?`,
    kind: "warning",
    okLabel: "Remove",
    cancelLabel: "Cancel",
  });
  if (!sure) return;
  const what = svc.kind === "mailpit" ? "caught mail" : svc.kind === "redis" ? "saved keys" : "databases";
  const wipe = await ask(`Keep its ${what} to find them again if you add ${svc.id} later, or delete them for good.`, {
    title: `Keep ${svc.id}’s data?`,
    kind: "warning",
    okLabel: "Delete Data",
    cancelLabel: "Keep Data",
  });
  serviceCall("remove_service", { id: svc.id, deleteData: wipe });
}

/* Add Service sheet: pick a kind, name and port, like creating a container. */
let svcKinds = [];
let svcKind = "mysql";

async function openAddService() {
  try {
    svcKinds = await invoke("service_kinds");
  } catch (e) {
    showError(String(e));
    return;
  }
  const grid = $("#svc-kinds");
  grid.replaceChildren();
  for (const k of svcKinds) {
    const label = document.createElement("label");
    label.className = "kind-card";
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "svc-kind";
    radio.value = k.kind;
    radio.checked = k.kind === svcKind;
    radio.onchange = () => {
      svcKind = k.kind;
      fillService();
    };
    const strong = document.createElement("strong");
    strong.textContent = k.name;
    const small = document.createElement("small");
    small.textContent = k.category;
    label.append(radio, strong, small);
    grid.append(label);
  }
  fillService();
  setServiceBusy(false);
  $("#new-service").showModal();
  $("#svc-name").focus();
}

function fillService() {
  const k = svcKinds.find((x) => x.kind === svcKind) ?? svcKinds[0];
  $("#svc-name").value = k.suggested_name;
  $("#svc-port").value = k.suggested_port;
  noteService();
  $("#svc-error").hidden = true;
}

function noteService() {
  const k = svcKinds.find((x) => x.kind === svcKind) ?? svcKinds[0];
  $("#svc-note").textContent = !k.installed
    ? k.kind === "redis"
      ? "Redis is built from source the first time, about a minute."
      : `${k.name} is downloaded the first time.`
    : `Its data goes in Anvil’s data/${$("#svc-name").value || "…"} folder.`;
}

function setServiceBusy(on) {
  $("#svc-create").disabled = on;
  $("#svc-create").textContent = on ? "Adding…" : "Add";
  for (const i of $("#svc-form").querySelectorAll("input")) i.disabled = on;
}

$("#add-service").onclick = openAddService;
$("#svc-cancel").onclick = () => $("#new-service").close();
$("#svc-name").oninput = (e) => {
  e.target.value = e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, "-");
  noteService();
};
$("#svc-form").onsubmit = async (e) => {
  e.preventDefault();
  const port = parseInt($("#svc-port").value, 10);
  setServiceBusy(true);
  try {
    services = await invoke("add_service", {
      kind: svcKind,
      name: $("#svc-name").value.trim() || null,
      port: Number.isNaN(port) ? null : port,
    });
    $("#new-service").close();
    renderServices();
  } catch (err) {
    $("#svc-error").textContent = String(err);
    $("#svc-error").hidden = false;
    setServiceBusy(false);
  }
};


function options(select, values, current, none) {
  select.replaceChildren(new Option(none, ""));
  for (const v of values) select.add(new Option(v, v));
  // Keep a configured app selectable even if it isn't installed where we look.
  if (current && !values.includes(current)) select.add(new Option(current, current));
  select.value = current ?? "";
}

function render() {
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
  options($("#terminal"), s.terminals.filter((t) => t !== "Terminal"), s.terminal, "Terminal");

  $("#tld").textContent = `.${s.tld}`;
  $("#http").textContent = s.http_port;
  $("#https").textContent = s.https_port;
  $("#dns").textContent = s.dns_port;
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
        button("Uninstall", () => call("php_uninstall", { version: v.version })),
      );
    } else {
      actions.append(button("Install", async (b) => {
        b.disabled = true;
        b.textContent = "Installing…";
        await call("php_install", { version: v.version });
      }));
    }
    row.append(label, actions);
    versions.append(row);
  }

  for (const input of document.querySelectorAll("[data-ini]")) {
    if (document.activeElement !== input) input.value = s.ini[input.dataset.ini] ?? "";
  }
}

function button(text, onclick) {
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
$("#theme").onchange = (e) => call("set_theme", { theme: e.target.value || null });
$("#editor").onchange = (e) => call("set_editor", { editor: e.target.value || null });
$("#terminal").onchange = (e) => call("set_terminal", { terminal: e.target.value || null });
$("#open-config-about").onclick = () => invoke("open_config_dir").catch((e) => showError(String(e)));
window.__TAURI__.app?.getVersion().then((v) => ($("#version").textContent = `Version ${v}`));
$("#open-config").onclick = () => invoke("open_config_dir").catch((e) => showError(String(e)));

for (const input of document.querySelectorAll("[data-ini]")) {
  input.onchange = async () => {
    const value = input.value.trim() || null;
    await call("set_ini", { site: null, key: input.dataset.ini, value });
    // An upload can't be bigger than the POST body that carries it.
    if (input.dataset.ini === "upload_max_filesize") {
      await call("set_ini", { site: null, key: "post_max_size", value });
    }
  };
}

for (const b of document.querySelectorAll("[data-page]")) {
  b.onclick = () => {
    for (const other of document.querySelectorAll("[data-page]")) other.setAttribute("aria-current", other === b);
    for (const page of document.querySelectorAll(".page")) page.hidden = page.id !== b.dataset.page;
    if (b.dataset.page === "languages" && !runtimes) loadRuntimes();
    if (b.dataset.page === "storage") loadAnvilStorage();
  };
}

document.addEventListener("contextmenu", (e) => {
  if (!e.target.closest("input, textarea")) e.preventDefault();
});

window.refresh();
