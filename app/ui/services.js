/* Services in the main window: list in the sidebar, details on the right, and
 * the Add Service sheet. Shares $, snap, invoke, el, toast and friends with app.js. */

let services = [];
let svcSelected = localStorage.getItem("service");
const ABBR = { mysql: "My", postgres: "Pg", redis: "Rd", mailpit: "Mp" };

async function serviceCall(cmd, args) {
  try {
    services = await invoke(cmd, args);
    showError(null);
  } catch (e) {
    showError(String(e));
  }
  if (mode === "services") renderMain();
  else if (selected) window.renderSiteServices(snap?.sites.find((s) => s.name === selected), $("#site"));
  renderModes();
}

/** Services attached to an app, as small chips under its address. */
window.renderSiteServices = (s, root) => {
  const box = root && $(".site-services", root);
  if (!box || !s) return;
  box.replaceChildren();
  for (const svc of services.filter((x) => x.apps.includes(s.name))) {
    const chip = el("button", "svc-chip");
    chip.title = `${svc.name} ${svc.version} on port ${svc.port}, ${svc.running ? "running" : "stopped"}`;
    chip.append(el("span", `dot ${svc.running ? "on" : ""}`), el("span", null, svc.id));
    chip.onclick = () => {
      setMode("services");
      selectService(svc.id);
    };
    box.append(chip);
  }
};

window.loadServices = () => serviceCall("services");

function setMode(m) {
  mode = m;
  localStorage.setItem("mode", m);
  filter = "";
  $("#search").value = "";
  renderModes();
  renderMain();
}

function renderModes() {
  for (const b of document.querySelectorAll(".modes [role=tab]")) {
    b.setAttribute("aria-selected", b.dataset.mode === mode);
  }
  const running = services.filter((x) => x.running).length;
  $("#svc-running").textContent = running ? running : "";
  $("#svc-running").title = running ? `${running} running` : "";
  const apps = mode === "apps";
  $("#search").placeholder = apps ? "Search applications" : "Search services";
  $("#add").title = apps ? "Add applications" : "Add a service (⌘N)";
  $("#add").setAttribute("aria-label", $("#add").title);
  $("#list").setAttribute("aria-label", apps ? "Applications" : "Services");
}

for (const b of document.querySelectorAll(".modes [role=tab]")) b.onclick = () => setMode(b.dataset.mode);

function serviceMark(svc, cls = "mark") {
  const m = el("span", `${cls} svc-mark svc-${svc.kind}`, ABBR[svc.kind] ?? "?");
  m.setAttribute("aria-hidden", "true");
  return m;
}

function serviceRow(svc) {
  const b = el("button", "item");
  b.dataset.name = svc.id;
  b.setAttribute("aria-current", svc.id === svcSelected);
  const text = el("span", "text");
  text.append(el("span", "name", svc.id), el("span", "sub", `${svc.name} ${svc.version} on port ${svc.port}`));
  const end = el("span", "end");
  for (const t of svc.tags.slice(0, 2)) end.append(el("span", "tag", t));
  const dot = el("span", `dot ${svc.running ? "on" : ""}`);
  dot.title = svc.running ? "Running" : svc.error ? "Failed to start" : "Stopped";
  end.append(dot);
  b.append(serviceMark(svc), text, end);
  b.onclick = () => selectService(svc.id);
  return b;
}

function renderServiceList() {
  const list = $("#list");
  list.replaceChildren();
  if (!services.length) {
    list.append(el("p", "empty", "No services yet."));
    return;
  }
  const note = filterNote("services", (k) => services.find((x) => x.kind === k)?.name ?? k);
  if (note) list.append(note);
  const q = filter.toLowerCase();
  const f = filterOf("services");
  const hit = (x) =>
    (!f.kind || x.kind === f.kind) &&
    (!f.running || x.running) &&
    (!q || [x.id, x.name, x.version, x.port, ...x.tags, ...x.apps].join(" ").toLowerCase().includes(q));
  let count = 0;
  for (const cat of [...new Set(services.map((x) => x.category))]) {
    const rows = services.filter((x) => x.category === cat && hit(x)).map(serviceRow);
    if (!rows.length) continue;
    count += rows.length;
    list.append(...listSection(`services:${cat}`, cat, rows, rows.length));
  }
  if (!count) list.append(el("p", "empty", filter ? `No services match “${filter}”.` : "No services match the filter."));
}

function selectService(id) {
  svcSelected = id;
  localStorage.setItem("service", id);
  renderServiceList();
  renderService();
  $(`.item[data-name="${CSS.escape(id)}"]`)?.focus();
}

function serviceKeys(e) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const items = [...document.querySelectorAll("#list .item")].filter((b) => !b.closest(".closed"));
  const i = items.findIndex((b) => b.dataset.name === svcSelected);
  const next = items[i + (e.key === "ArrowDown" ? 1 : -1)];
  if (next) selectService(next.dataset.name);
}

/** Editable list of chips: `values` shown, `add` control appended, `save` gets the new list. */
function chips(root, values, label, save) {
  root.replaceChildren();
  for (const v of values) {
    const c = el("span", "chip", v.text ?? v);
    if (v.onclick) {
      c.classList.add("link");
      c.onclick = v.onclick;
    }
    const x = el("button", null, "×");
    x.setAttribute("aria-label", `Remove ${v.text ?? v}`);
    x.onclick = (e) => {
      e.stopPropagation();
      save(values.map((w) => w.value ?? w).filter((w) => w !== (v.value ?? v)));
    };
    c.append(x);
    root.append(c);
  }
  return root;
}

function renderService() {
  const root = $("#site");
  if (!services.some((x) => x.id === svcSelected)) svcSelected = services[0]?.id;
  const svc = services.find((x) => x.id === svcSelected);
  if (!svc) {
    const box = el("div", "welcome");
    const add = el("button", "primary", "Add Service…");
    add.onclick = openAddService;
    box.append(
      el("h1", null, "No services yet"),
      el("p", "muted", "Add MySQL, PostgreSQL, Redis or Mailpit. Each one gets its own name, port and data, and you can add as many as you need."),
      add,
    );
    root.replaceChildren(box);
    delete root.dataset.service;
    return;
  }
  // Keep focus and typing in a field across refreshes of the same service.
  if (root.dataset.service === svc.id && root.contains(document.activeElement) && document.activeElement.matches("input")) return;
  root.dataset.service = svc.id;
  root.replaceChildren($("#service-tpl").content.cloneNode(true));

  $(".site-name", root).textContent = svc.id;
  $(".site-title .mark", root).replaceWith(serviceMark(svc, "mark big"));
  $(".svc-what", root).textContent = svc.installed ? `${svc.name} ${svc.version}` : `${svc.name}, not installed`;

  $(".run-box .dot", root).className = `dot ${svc.running ? "on" : ""}`;
  $(".state-text", root).textContent = svc.running ? "Running" : "Stopped";
  $('[data-svc="start"]', root).hidden = svc.running;
  $('[data-svc="restart"]', root).hidden = !svc.running;
  $('[data-svc="stop"]', root).hidden = !svc.running;
  for (const b of root.querySelectorAll("[data-svc]")) {
    b.onclick = () => serviceCall("service_run", { id: svc.id, action: b.dataset.svc });
  }
  const err = $(".run-error", root);
  err.textContent = svc.error ?? "";
  err.hidden = !svc.error;

  const inbox = $('[data-svc-open="inbox"]', root);
  inbox.hidden = !svc.web;
  inbox.disabled = !svc.running;
  inbox.onclick = () => invoke("open_url", { url: svc.web }).catch((e) => showError(String(e)));
  for (const what of ["data", "log"]) {
    $(`[data-svc-open="${what}"]`, root).onclick = () =>
      invoke("open_service", { id: svc.id, what }).catch((e) => showError(String(e)));
  }

  const portInput = (sel, value, web) => {
    const input = $(sel, root);
    input.value = value;
    input.onchange = () => {
      const n = parseInt(input.value, 10);
      serviceCall("set_service_port", { id: svc.id, port: Number.isNaN(n) ? null : n, web });
    };
  };
  portInput("#svc-port-edit", svc.port, false);
  $(".svc-web", root).hidden = !svc.web_port;
  if (svc.web_port) portInput("#svc-web-edit", svc.web_port, true);
  const fix = svc.suggested_port ?? svc.suggested_web_port;
  if (fix && !svc.running) {
    const web = !svc.suggested_port;
    err.textContent = `Port ${web ? svc.web_port : svc.port} is taken by another program.`;
    const use = el("button", "link", `Use ${fix}`);
    use.onclick = () => serviceCall("set_service_port", { id: svc.id, port: fix, web });
    err.append(" ", use);
    err.hidden = false;
  }

  // Applications: chips that jump to the app, plus a picker for the rest.
  const known = new Set(snap.sites.map((s) => s.name));
  const appChips = svc.apps.map((a) => ({
    text: known.has(a) ? `${a}.${snap.tld}` : `${a} (gone)`,
    value: a,
    onclick: known.has(a) ? () => { setMode("apps"); select(a); } : null,
  }));
  const saveApps = (apps) => serviceCall("set_service_apps", { id: svc.id, apps });
  const appsBox = chips($(".svc-apps", root), appChips, "application", saveApps);
  const pick = el("select");
  pick.setAttribute("aria-label", "Add an application");
  pick.add(new Option(svc.apps.length ? "Add…" : "Choose an application…", ""));
  for (const s of snap.sites.filter((s) => !s.linked && !svc.apps.includes(s.name))) {
    pick.add(new Option(`${s.name}.${snap.tld}`, s.name));
  }
  pick.onchange = () => pick.value && saveApps([...svc.apps, pick.value]);
  appsBox.append(pick);

  const saveTags = (tags) => serviceCall("set_service_tags", { id: svc.id, tags });
  const tagsBox = chips($(".svc-tags", root), svc.tags, "tag", saveTags);
  const tagInput = el("input");
  tagInput.placeholder = "Add tag";
  tagInput.setAttribute("aria-label", "Add a tag");
  tagInput.spellcheck = false;
  tagInput.onkeydown = (e) => {
    if (e.key !== "Enter" || !tagInput.value.trim()) return;
    e.preventDefault();
    const add = tagInput.value.split(",").map((t) => t.trim()).filter(Boolean);
    tagInput.blur();
    saveTags([...svc.tags, ...add]);
  };
  tagsBox.append(tagInput);

  $('[data-svc-open="data"]', root).title = `Show ~/.anvil/data/${svc.id} in Finder`;
  const env = envFor(svc);
  $(".svc-env", root).textContent = env;
  $(".svc-copy", root).onclick = () => copy(env, `Copied ${svc.id}’s settings`);

  // Database apps installed on this Mac open the service directly.
  for (const client of svc.clients) {
    const b = el("button");
    b.innerHTML = `<svg viewBox="0 0 16 16" aria-hidden="true"><ellipse cx="8" cy="3.75" rx="5.25" ry="2"/><path d="M2.75 3.75v8.5c0 1.1 2.35 2 5.25 2s5.25-.9 5.25-2v-8.5M2.75 8c0 1.1 2.35 2 5.25 2s5.25-.9 5.25-2"/></svg>`;
    b.append(client);
    b.title = `Open ${svc.id} in ${client}`;
    b.onclick = () => openClient(svc, client, chosenDb[svc.id]);
    $(".quick", root).append(b);
  }
  renderContents(svc, root);
  $(".svc-remove", root).onclick = () => removeService(svc);
}

async function copy(text, done) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // Older WebKit: copy through a hidden field.
    const t = el("textarea");
    t.value = text;
    document.body.append(t);
    t.select();
    document.execCommand("copy");
    t.remove();
  }
  toast(done);
}

/** Remove a service; a second question decides whether its data goes too. */
async function removeService(svc) {
  // One question; the default button (Return) keeps the data, Escape cancels.
  const what = svc.kind === "mailpit" ? "caught mail" : svc.kind === "redis" ? "saved keys" : "databases";
  const answer = await window.__TAURI__.dialog.message(
    `${svc.id} stops and is removed from Anvil. Keep its ${what} to find them again if you add ${svc.id} later.`,
    {
      title: `Remove ${svc.id}?`,
      kind: "warning",
      buttons: { yes: "Remove, Keep Data", no: "Remove and Delete Data", cancel: "Cancel" },
    },
  );
  if (answer !== "Remove, Keep Data" && answer !== "Remove and Delete Data" && answer !== "Yes" && answer !== "No") return;
  const wipe = answer === "Remove and Delete Data" || answer === "No";
  await serviceCall("remove_service", { id: svc.id, deleteData: wipe });
  toast(wipe ? `Removed ${svc.id} and its data` : `Removed ${svc.id}; its data is kept`);
}

/* ---------- Add Service sheet: kind, name, port, app and tags, like creating a container ---------- */

let svcKinds = [];
let svcKind = localStorage.getItem("serviceKind") ?? "mysql";

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
    const label = el("label", "kind-card");
    const radio = el("input");
    radio.type = "radio";
    radio.name = "svc-kind";
    radio.value = k.kind;
    radio.checked = k.kind === svcKind;
    radio.onchange = () => {
      svcKind = k.kind;
      localStorage.setItem("serviceKind", k.kind);
      fillService();
    };
    label.append(radio, el("strong", null, k.name), el("small", null, k.category));
    grid.append(label);
  }
  const app = $("#svc-app");
  app.replaceChildren(new Option("None", ""));
  for (const s of snap.sites.filter((s) => !s.linked)) app.add(new Option(`${s.name}.${snap.tld}`, s.name));
  // Adding from an application's page? Start with that application.
  app.value = mode === "apps" && selected ? selected : "";
  fillService();
  setServiceBusy(false);
  $("#new-service").showModal();
  $("#svc-name").select();
}

function fillService() {
  const k = svcKinds.find((x) => x.kind === svcKind) ?? svcKinds[0];
  svcKind = k.kind;
  $("#svc-name").value = k.suggested_name;
  const ver = $("#svc-version");
  ver.replaceChildren();
  for (const v of k.versions) {
    const tags = [v === k.default_version && "recommended", k.installed_versions.includes(v) && "installed"].filter(Boolean);
    ver.add(new Option(tags.length ? `${v} (${tags.join(", ")})` : v, v));
  }
  ver.value = k.default_version;
  ver.disabled = k.versions.length < 2;
  noteService();
  $("#svc-error").hidden = true;
}

/** One quiet line under the name: what gets downloaded, the port and where data goes. */
function noteService() {
  const k = svcKinds.find((x) => x.kind === svcKind);
  const v = $("#svc-version").value;
  const name = $("#svc-name").value || "…";
  const fetch = k.installed_versions.includes(v)
    ? ""
    : k.kind === "redis"
      ? `Redis ${v} is built from source first (about a minute). `
      : `${k.name} ${v} is downloaded first. `;
  $("#svc-note").textContent = `${fetch}Port ${k.suggested_port}, data in ~/.anvil/data/${name}`;
}

function setServiceBusy(on) {
  $("#svc-create").disabled = on;
  $("#svc-create").textContent = on ? "Adding…" : "Add";
  for (const i of $("#svc-form").querySelectorAll("input, select")) i.disabled = on;
}

$("#svc-version").onchange = noteService;
$("#svc-cancel").onclick = () => $("#new-service").close();
$("#svc-name").oninput = (e) => {
  e.target.value = e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, "-");
  noteService();
};
$("#svc-form").onsubmit = async (e) => {
  e.preventDefault();
  const name = $("#svc-name").value.trim();
  const app = $("#svc-app").value;
  setServiceBusy(true);
  try {
    services = await invoke("add_service", {
      kind: svcKind,
      name: name || null,
      port: null,
      version: $("#svc-version").value || null,
      tags: [],
      apps: app ? [app] : [],
    });
    $("#new-service").close();
    svcSelected = name;
    localStorage.setItem("service", name);
    if (mode !== "services") setMode("services");
    else renderMain();
    renderModes();
    toast(`Added ${name}`);
  } catch (err) {
    $("#svc-error").textContent = String(err);
    $("#svc-error").hidden = false;
    setServiceBusy(false);
  }
};

renderModes();
window.loadServices();

/* ---------- What's inside: databases, keys, messages ---------- */

const contents = new Map(); // service id -> last contents
const chosenDb = {}; // service id -> database used in the .env lines

function openClient(svc, client, database) {
  invoke("open_in_client", { id: svc.id, client, database: database ?? null }).catch((e) => showError(String(e)));
}

/** The .env lines, pointed at the chosen database. */
function envFor(svc) {
  const db = chosenDb[svc.id];
  if (!db) return svc.env;
  return svc.env
    .split("\n")
    .map((l) => (l.startsWith("DATABASE_URL=") ? l.replace(/\/[^/]*$/, `/${db}`) : l))
    .concat(`DB_DATABASE=${db}`)
    .join("\n");
}

const CONTENTS_TITLE = { mysql: "Databases", postgres: "Databases", redis: "Keys", mailpit: "Messages" };
const SYSTEM_DBS = new Set(["information_schema", "mysql", "performance_schema", "sys", "postgres", "template1"]);

async function renderContents(svc, root, action) {
  $(".svc-contents-title", root).textContent = CONTENTS_TITLE[svc.kind];
  const box = $(".svc-contents", root);
  if (!svc.running) {
    box.replaceChildren(el("p", "muted", `Start ${svc.id} to see what’s inside.`));
    contents.delete(svc.id);
    return;
  }
  const cached = contents.get(svc.id);
  if (cached && !action) drawContents(svc, root, cached);
  else if (!cached) box.replaceChildren(el("p", "muted", "Looking inside…"));
  try {
    const c = action
      ? await invoke("service_data", action)
      : await invoke("service_contents", { id: svc.id });
    contents.set(svc.id, c);
    if (root.dataset.service === svc.id) drawContents(svc, root, c);
  } catch (e) {
    if (action) showError(String(e));
    else box.replaceChildren(el("p", "muted", String(e)));
  }
}

function drawContents(svc, root, c) {
  const box = $(".svc-contents", root);
  const list = el("dl", "group res-list");
  const line = (name, value, ...buttons) => {
    const row = el("div", "row");
    const dt = el("dt");
    dt.append(el("span", null, name));
    const dd = el("dd", "res-cells");
    if (value != null) dd.append(el("span", "size", value));
    dd.append(...buttons);
    row.append(dt, dd);
    list.append(row);
    return dt;
  };
  const act = (label, fn, cls) => {
    const b = el("button", cls ?? "link", label);
    b.onclick = fn;
    return b;
  };
  const confirm = (title, message, ok) =>
    window.__TAURI__.dialog.ask(message, { title, kind: "warning", okLabel: ok, cancelLabel: "Cancel" });

  if (svc.kind === "mysql" || svc.kind === "postgres") {
    // The Connect lines can point at one of the databases.
    const pick = $(".svc-db", root);
    const own = c.databases.map(([n]) => n).filter((n) => !SYSTEM_DBS.has(n));
    pick.replaceChildren(new Option("No database", ""), ...own.map((n) => new Option(n, n)));
    pick.value = own.includes(chosenDb[svc.id]) ? chosenDb[svc.id] : "";
    pick.hidden = !own.length;
    pick.onchange = () => {
      chosenDb[svc.id] = pick.value || undefined;
      const env = envFor(svc);
      $(".svc-env", root).textContent = env;
      $(".svc-copy", root).onclick = () => copy(env, `Copied ${svc.id}’s settings`);
    };
    const system = c.databases.filter(([n]) => SYSTEM_DBS.has(n));
    for (const [name, size] of c.databases.filter(([n]) => !SYSTEM_DBS.has(n))) {
      const buttons = [];
      {
        if (svc.clients[0]) buttons.push(act("Open", () => openClient(svc, svc.clients[0], name)));
        buttons.push(act("Drop…", async () => {
          if (await confirm(`Drop ${name}?`, `Every table in ${name} is deleted. This can’t be undone.`, "Drop"))
            renderContents(svc, root, { id: svc.id, action: "drop", name });
        }, "link danger hover-only"));
      }
      line(name, bytes(size), ...buttons);
    }
    // The server's own databases: one quiet line instead of a row each.
    if (system.length) {
      const dt = line("Server databases", bytes(system.reduce((a, [, b]) => a + b, 0)));
      dt.classList.add("muted");
      dt.append(el("small", "path", system.map(([n]) => n).join(", ")));
    }
    // New database: name field plus Create.
    const row = el("div", "row");
    const input = el("input");
    input.placeholder = "New database name";
    input.spellcheck = false;
    input.setAttribute("aria-label", "New database name");
    const create = () => {
      const name = input.value.trim();
      if (name) renderContents(svc, root, { id: svc.id, action: "create", name });
    };
    input.onkeydown = (e) => e.key === "Enter" && create();
    row.append(input, act("Create", create, "plain"));
    list.append(row);
  } else if (svc.kind === "redis") {
    if (!c.keys.length) line("No keys yet", null);
    for (const [db, keys] of c.keys) line(db, `${keys} ${keys === 1 ? "key" : "keys"}`);
    line("Memory", c.memory ?? "–");
    if (c.keys.length) {
      const row = el("div", "row");
      row.append(act("Flush All…", async () => {
        if (await confirm(`Flush ${svc.id}?`, "Every key in every database is deleted.", "Flush"))
          renderContents(svc, root, { id: svc.id, action: "empty" });
      }, "plain danger"));
      list.append(row);
    }
  } else {
    const n = c.messages ?? 0;
    const buttons = [act("Open inbox", () => invoke("open_url", { url: svc.web }))];
    if (n) buttons.push(act("Delete All…", async () => {
      if (await confirm(`Delete ${n} messages?`, `Every message caught by ${svc.id} is deleted.`, "Delete"))
        renderContents(svc, root, { id: svc.id, action: "empty" });
    }, "link danger"));
    line(n === 1 ? "1 message" : `${n} messages`, null, ...buttons);
  }
  box.replaceChildren(list);
}
