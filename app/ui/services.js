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
  renderModes();
}

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
    const p = el("p", "empty", "No services yet. Add MySQL, PostgreSQL, Redis or Mailpit; each one gets its own port and data.");
    const add = el("button", "below", "Add Service…");
    add.onclick = openAddService;
    p.append(el("br"), add);
    list.append(p);
    return;
  }
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
    const box = el("div", "rows");
    box.append(...rows);
    list.append(el("h2", null, cat), box);
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
  const items = [...document.querySelectorAll("#list .item")];
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
    root.innerHTML = `<div class="placeholder">Add a service to get started</div>`;
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
  $(".state-text", root).textContent = svc.running ? `Running on port ${svc.port}` : "Stopped";
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
  tagInput.placeholder = svc.tags.length ? "Add tag" : "Add a tag, e.g. queue";
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

  $(".svc-data", root).textContent = `~/.anvil/data/${svc.id}`;
  $(".svc-env", root).textContent = svc.env;
  $(".svc-copy", root).onclick = () => copy(svc.env, `Copied ${svc.id}’s settings`);
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
  $("#svc-tags").value = "";
  fillService();
  setServiceBusy(false);
  $("#new-service").showModal();
  $("#svc-name").select();
}

function fillService() {
  const k = svcKinds.find((x) => x.kind === svcKind) ?? svcKinds[0];
  svcKind = k.kind;
  $("#svc-name").value = k.suggested_name;
  $("#svc-port").value = k.suggested_port;
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

function noteService() {
  const k = svcKinds.find((x) => x.kind === svcKind);
  const v = $("#svc-version").value;
  $("#svc-note").textContent = !k.installed_versions.includes(v)
    ? k.kind === "redis"
      ? `Redis ${v} is built from source the first time, about a minute.`
      : `${k.name} ${v} is downloaded the first time.`
    : `Its data goes in ~/.anvil/data/${$("#svc-name").value || "…"}`;
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
  const port = parseInt($("#svc-port").value, 10);
  const name = $("#svc-name").value.trim();
  const app = $("#svc-app").value;
  setServiceBusy(true);
  try {
    services = await invoke("add_service", {
      kind: svcKind,
      name: name || null,
      port: Number.isNaN(port) ? null : port,
      version: $("#svc-version").value || null,
      tags: $("#svc-tags").value.split(",").map((t) => t.trim()).filter(Boolean),
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
