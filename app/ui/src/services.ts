import type { ServiceRow } from "./types";
import {
  $,
  listSection,
  resCache,
  bytes,
  el,
  filter,
  filterNote,
  filterOf,
  focusRow,
  invoke,
  markBox,
  metaParts,
  mode,
  renderMain,
  select,
  selected,
  setMode,
  showError,
  showTab,
  snap,
} from "./app";
import { late } from "./late";
import { picker } from "./picker";

/* Services in the main window: list in the sidebar, details on the right, and
 * the Add Service sheet. Shares $, snap, invoke, el and friends with app.js. */

export let services: ServiceRow[] = [];
let svcSelected = localStorage.getItem("service");
// After a removal that kept the data: a muted line over the list says where it is.
let keptNote = "";
const ABBR = { mysql: "My", postgres: "Pg", redis: "Rd", mailpit: "Mp", versitygw: "S3", silo: "Si" };

async function serviceCall(cmd: string, args?: Record<string, unknown>) {
  try {
    services = await invoke(cmd, args);
    showError(null, cmd);
  } catch (e) {
    showError(String(e), cmd);
  }
  if (mode === "services") renderMain();
  else if (selected)
    window.renderSiteServices(
      snap?.sites.find((s) => s.name === selected),
      $("#site"),
    );
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
  // The Overview's Database line, with the Database viewer on: what Browse opens. A
  // worktree's own Database line has its Browse (app.ts).
  const dbRow = $(".db-row", root);
  const db = snap?.db_viewer?.apps[s.name];
  dbRow.hidden = !db || !!s.worktree_of || !browsable();
  if (!dbRow.hidden) $("dd", dbRow).replaceChildren(el("span", null, db), " ", browseButton(s.name));
  // The Overview's Mail line: the mail catcher's for an app it catches, else only for an
  // app whose .env sends to an Anvil Mailpit.
  const row = $(".mail-row", root);
  const c = snap?.catcher;
  if (c?.caught.includes(s.name)) {
    row.hidden = false;
    const b = el("button", "inline-action", "Show mail");
    b.onclick = () => showTab(root, "mail");
    $("dd", row).replaceChildren(c.error ? el("span", "error", c.error) : el("span", null, "Caught by Anvil"), " ", b);
    return;
  }
  const svc = services.find((x) => x.id === snap?.mail?.[s.name]);
  row.hidden = !svc?.web;
  if (!svc?.web) return;
  const dd = $("dd", row);
  const b = el("button", "inline-action", svc.running ? "Open inbox" : "Start");
  b.onclick = () =>
    svc.running
      ? invoke("open_url", { url: svc.web }).catch((e) => showError(String(e)))
      : serviceCall("service_run", { id: svc.id, action: "start" });
  dd.replaceChildren(el("span", svc.running ? null : "muted", svc.running ? svc.id : `${svc.id} is stopped`), " ", b);
};

window.loadServices = () => serviceCall("services");

/** Browse: open a service (at `database`) or an app's database in the Database viewer,
 * logged in. A failure says why beside the button. */
export function browseButton(name: string, database?: string, cls = "inline-action") {
  const b = el("button", cls || null, "Browse");
  b.title = `Open in the Database viewer at db.${snap.tld}`;
  b.onclick = async () => {
    if (b.nextElementSibling?.classList.contains("browse-error")) b.nextElementSibling.remove();
    b.disabled = true;
    try {
      await invoke("db_open", { name, database: database ?? null });
    } catch (e) {
      b.after(el("span", "error browse-error", ` ${String(e)}`));
    } finally {
      b.disabled = false;
    }
  };
  return b;
}

/** Browse is offered while the viewer is on and served. */
const browsable = () => !!snap?.db_viewer && !snap.db_viewer.problem;

export function renderModes() {
  for (const b of document.querySelectorAll<HTMLElement>(".modes [role=tab]")) {
    b.setAttribute("aria-selected", String(b.dataset.mode === mode));
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

for (const b of document.querySelectorAll<HTMLElement>(".modes [role=tab]")) b.onclick = () => setMode(b.dataset.mode);

function serviceMark(svc, cls = "mark") {
  const m = el("span", `${cls} svc-mark svc-${svc.kind}`, ABBR[svc.kind] ?? "?");
  m.setAttribute("aria-hidden", "true");
  return m;
}

function serviceRow(svc) {
  const b = el("button", "item");
  b.dataset.name = svc.id;
  b.setAttribute("aria-current", String(svc.id === svcSelected));
  const text = el("span", "text");
  text.append(el("span", "name", svc.id), el("span", "sub", `${svc.name} ${svc.version} on port ${svc.port}`));
  // As an app's row: the run state is a badge on the mark, and the tags end where the
  // apps' kind labels do.
  const end = el("span", "end");
  for (const t of svc.tags.slice(0, 2)) end.append(el("span", "tag", t));
  b.append(markBox(serviceMark(svc), svc.running ? "on" : svc.error ? "bad" : null), text, end);
  b.onclick = () => selectService(svc.id);
  return b;
}

export function renderServiceList() {
  const list = $("#list");
  list.replaceChildren();
  if (keptNote) list.append(el("p", "muted svc-kept", keptNote));
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
  for (const cat of new Set(services.map((x) => x.category))) {
    const rows = services.filter((x) => x.category === cat && hit(x)).map(serviceRow);
    if (!rows.length) continue;
    count += rows.length;
    list.append(...listSection(`services:${cat}`, cat, rows, rows.length));
  }
  if (!count) list.append(el("p", "empty", filter ? `No services match “${filter}”.` : "No services match the filter."));
}

export function selectService(id) {
  svcSelected = id;
  localStorage.setItem("service", id);
  renderServiceList();
  renderService();
  // Scrolled only as far as needed (keys moving past the edge): focus alone centres it.
  focusRow($(`.item[data-name="${CSS.escape(id)}"]`));
}

export function serviceKeys(e) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const items = [...document.querySelectorAll<HTMLElement>("#list .item")].filter((b) => !b.closest(".closed"));
  const i = items.findIndex((b) => b.dataset.name === svcSelected);
  const next = items[i + (e.key === "ArrowDown" ? 1 : -1)];
  if (next) selectService(next.dataset.name);
}

/** Editable list of chips: `values` shown, `add` control appended, `save` gets the new list. */
export function chips(root, values, label, save) {
  root.replaceChildren();
  for (const v of values) {
    const c = el("span", "chip", v.text ?? v);
    if (v.onclick) {
      c.classList.add("link");
      c.onclick = v.onclick;
    }
    const x = el("button", null, "×");
    x.setAttribute("aria-label", String(`Remove ${v.text ?? v}`));
    x.onclick = (e) => {
      e.stopPropagation();
      save(values.map((w) => w.value ?? w).filter((w) => w !== (v.value ?? v)));
    };
    c.append(x);
    root.append(c);
  }
  return root;
}

export function renderService() {
  const root = $("#site");
  if (!services.some((x) => x.id === svcSelected)) svcSelected = services[0]?.id;
  const svc = services.find((x) => x.id === svcSelected);
  if (!svc) {
    const box = el("div", "welcome");
    const add = el("button", "primary", "Add Service…");
    add.onclick = () => openAddService();
    box.append(
      el("h1", null, "No services yet"),
      el(
        "p",
        "muted",
        "Add MySQL, PostgreSQL, Redis, Mailpit or S3 storage. Each one gets its own name, port and data, and you can add as many as you need.",
      ),
      add,
    );
    root.replaceChildren(box);
    delete root.dataset.service;
    return;
  }
  // Keep focus and typing in a field across refreshes of the same service.
  if (root.dataset.service === svc.id && root.contains(document.activeElement) && document.activeElement?.matches("input")) return;
  root.dataset.service = svc.id;
  root.replaceChildren($("#service-tpl").content.cloneNode(true));

  $(".site-name", root).textContent = svc.id;
  $(".site-title .mark", root).replaceWith(serviceMark(svc, "mark big"));
  drawServiceMeta(svc, root);

  $(".run-box .dot", root).className = `dot ${svc.running ? "on" : svc.error ? "bad" : ""}`;
  $(".state-text", root).textContent = svc.running ? "Running" : "Stopped";
  $('[data-svc="start"]', root).hidden = svc.running;
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
  if (svc.kind !== "mailpit") {
    inbox.title = "Open the web console; sign in with the keys in Connect";
    inbox.setAttribute("aria-label", "Open the web console");
  }
  // The data folder opens in Finder: the outline folder icon, as on an app's page.
  const data = $('[data-svc-open="data"]', root);
  data.title = `Show ~/.anvil/data/${svc.id} in Finder`;
  data.setAttribute("aria-label", String(data.title));
  inbox.onclick = () => invoke("open_url", { url: svc.web }).catch((e) => showError(String(e)));
  data.onclick = () => invoke("open_service", { id: svc.id, what: "data" }).catch((e) => showError(String(e)));
  // The log is in the Processes window, on its Services tab, while the service runs.
  const log = $('[data-svc-open="log"]', root);
  log.title = svc.running ? "Show the log in Processes" : "Not running";
  log.setAttribute("aria-label", svc.running ? `Show the log of ${svc.id} in Processes` : "Log: not running");
  log.setAttribute("aria-disabled", String(!svc.running));
  log.onclick = () => svc.running && invoke("show_processes", { select: `service:${svc.id}` }).catch((e) => showError(String(e)));
  // One database app: an icon beside the folder; several: a row of them under the contents.
  const one = $('[data-svc-open="client"]', root);
  one.hidden = svc.clients.length !== 1;
  if (svc.clients.length === 1) {
    one.title = `Open in ${svc.clients[0]}`;
    one.setAttribute("aria-label", `Open ${svc.id} in ${svc.clients[0]}`);
    one.onclick = () => openClient(svc, svc.clients[0], chosenDb[svc.id]);
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
  if (svc.kind !== "mailpit") $('label[for="svc-web-edit"]', root).textContent = "Console port";
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
    onclick: known.has(a)
      ? () => {
          setMode("apps");
          select(a);
        }
      : null,
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
    const add = tagInput.value
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);
    tagInput.blur();
    saveTags([...svc.tags, ...add]);
  };
  tagsBox.append(tagInput);

  const env = envFor(svc);
  $(".svc-env", root).textContent = env;
  $(".svc-copy", root).onclick = (e) => copy(env, e.currentTarget);

  const clients = $(".svc-clients", root);
  clients.hidden = svc.clients.length < 2;
  if (svc.clients.length > 1) {
    clients.append(el("span", "muted", "Open with"));
    for (const client of svc.clients) {
      const open = el("button", null, client);
      open.setAttribute("aria-label", String(`Open ${svc.id} in ${client}`));
      open.onclick = () => openClient(svc, client, chosenDb[svc.id]);
      clients.append(open);
    }
  }
  renderContents(svc, root);
  if (browsable() && (svc.kind === "mysql" || svc.kind === "postgres")) $(".svc-contents-head", root).append(browseButton(svc.id));
  $(".svc-remove", root).onclick = () => removeService(svc);
}

/** The line under the name, as an app's: kind · version · port · data size (once
 * an app's Resources measured it) · what is notable about its state. */
function drawServiceMeta(svc, root) {
  const parts = [svc.name, svc.version, `port ${svc.port}`];
  const use = [...resCache.values()].flatMap((r) => r?.services ?? []).find((u) => u.anvil === svc.id && u.data_bytes != null);
  if (use) parts.push(`data ${bytes(use.data_bytes)}`);
  if (!svc.installed) parts.push("not installed");
  else if (svc.enabled) parts.push("starts with Anvil");
  const spans = parts.map((t) => el("span", null, t));
  $(".meta", root).replaceChildren(...metaParts(spans));
}

/** A row of a service's lists, in the Processes look: the name, a muted detail, then buttons. */
function svcLine(name, detail, ...buttons) {
  const row = el("div", "proc svc-row");
  row.append(el("span", "proc-name", name));
  const last = el("span", "proc-last", detail ?? "");
  last.title = detail ?? "";
  const actions = el("span", "proc-actions");
  actions.append(...buttons);
  row.append(last, actions);
  return row;
}

/** Copy `text`; the button says "Copied" for a moment instead of a toast. */
export async function copy(text, btn) {
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
  // An icon button too: "Copied" in its place, then the icon again.
  const label = btn.dataset.label ?? btn.innerHTML;
  btn.dataset.label = label;
  btn.textContent = "Copied";
  setTimeout(() => (btn.innerHTML = label), 1500);
}

/** Remove a service; a second question decides whether its data goes too. */
async function removeService(svc) {
  // One question; the default button (Return) keeps the data, Escape cancels.
  const what = { mailpit: "caught mail", redis: "saved keys", versitygw: "buckets", silo: "buckets" }[svc.kind] ?? "databases";
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
  keptNote = wipe ? "" : `Kept ${svc.id}’s ${what} for when you add ${svc.id} again.`;
  await serviceCall("remove_service", { id: svc.id, deleteData: wipe });
}

/* ---------- Add Service sheet: kind, name, port, app and tags, like creating a container ---------- */

let svcKinds: any[] = [];
let svcKind = localStorage.getItem("serviceKind") ?? "mysql";

// Monochrome glyphs: a database for MySQL and PostgreSQL, layers for Redis, a letter for
// Mailpit, a bucket for S3.
const SVC_ICONS: Record<string, string> = {
  db: "M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3Zm0 0v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3",
  redis: "M12 3 3 7.5l9 4.5 9-4.5L12 3Zm-9 9 9 4.5 9-4.5M3 16.5 12 21l9-4.5",
  mailpit: "M3.5 6.5h17v11h-17zM3.5 7l8.5 6.5L20.5 7",
  versitygw: "M4 6c0-1.4 3.6-2.5 8-2.5s8 1.1 8 2.5-3.6 2.5-8 2.5S4 7.4 4 6Zm0 0 2 13c.2 1 2.8 1.5 6 1.5s5.8-.5 6-1.5l2-13",
};
SVC_ICONS.silo = SVC_ICONS.versitygw;
const svcIcon = (kind: string) =>
  `<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="${SVC_ICONS[kind] ?? SVC_ICONS.db}"/></svg>`;
const SVC_ABOUT: Record<string, string> = {
  mysql: "Relational database",
  postgres: "Relational database",
  redis: "Cache, queues and sessions",
  mailpit: "Catches the mail apps send",
  versitygw: "S3-compatible file storage",
  silo: "S3-compatible file storage",
};

const svcPicker = picker({
  list: $("#svc-kinds"),
  search: $("#svc-search"),
  sheet: $("#new-service"),
  form: $("#svc-form"),
  groups: [],
  choices: () =>
    svcKinds.map((k) => ({ id: k.kind, name: k.name, about: SVC_ABOUT[k.kind] ?? k.category, group: k.category, icon: svcIcon(k.kind) })),
  onPick: (kind) => {
    svcKind = kind;
    localStorage.setItem("serviceKind", kind);
    fillService();
  },
  none: "No services match",
});

/** The add-service sheet; `want` starts it on a kind and version (a project's .anvil.toml). */
export async function openAddService(want?: { kind: string; version: string }) {
  try {
    svcKinds = await invoke("service_kinds");
  } catch (e) {
    showError(String(e));
    return;
  }
  const app = $("#svc-app");
  app.replaceChildren(new Option("None", ""));
  for (const s of snap.sites.filter((s) => !s.linked)) app.add(new Option(`${s.name}.${snap.tld}`, s.name));
  // Adding from an application's page? Start with that application.
  app.value = mode === "apps" && selected ? selected : "";
  svcPicker.groups.splice(0, Infinity, ...new Set(svcKinds.map((k) => k.category)));
  svcPicker.draw(want?.kind ?? svcKind);
  if (want?.version) {
    $("#svc-version").value = want.version;
    noteService();
    drawLicense();
  }
  setServiceBusy(false);
  $("#new-service").showModal();
  $("#svc-search").focus();
}

/** The chosen version's license: accepted before (a quiet note), or to accept now. */
function drawLicense() {
  const k = svcKinds.find((x) => x.kind === svcKind);
  const v = $("#svc-version").value;
  const entry = k?.licenses?.find((l) => l[0] === v);
  const text = $("#svc-license-text");
  const check = $("#svc-license-check") as HTMLInputElement;
  if (!entry) {
    $(".svc-license").hidden = true;
    return;
  }
  $(".svc-license").hidden = false;
  const [, lic, accepted] = entry;
  const link = el("a", null, "Read the license");
  link.href = lic.url;
  link.onclick = (e) => {
    e.preventDefault();
    invoke("open_url", { url: lic.url }).catch((err) => showError(String(err)));
  };
  text.replaceChildren(`${k.name} ${v} comes under ${lic.name}. `, link);
  $("#svc-license-accept").hidden = !!accepted;
  $("#svc-license-done").hidden = !accepted;
  if (accepted)
    $("#svc-license-done").textContent =
      `✓ Accepted on ${new Date(accepted).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })}`;
  check.checked = false;
  syncAdd();
}

/** Add waits for the license, unless it was accepted before. */
function syncAdd() {
  const k = svcKinds.find((x) => x.kind === svcKind);
  const accepted = k?.licenses?.find((l) => l[0] === $("#svc-version").value)?.[2];
  ($("#svc-create") as HTMLButtonElement).disabled = !accepted && !($("#svc-license-check") as HTMLInputElement).checked;
}
$("#svc-license-check").onchange = syncAdd;

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
  const port = $("#svc-port");
  port.value = k.suggested_port;
  // Say why it isn't the usual port, as the next free one was picked.
  $("#svc-port-note").textContent = k.suggested_port === k.default_port ? "" : `Suggested: the usual port ${k.default_port} is in use`;
  noteService();
  drawLicense();
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
  $("#svc-note").textContent = `${fetch}Data in ~/.anvil/data/${name}`;
}

function setServiceBusy(on) {
  $("#svc-create").disabled = on;
  if (!on) syncAdd();
  $("#svc-create").textContent = on ? "Adding…" : "Add";
  for (const i of $("#svc-form").querySelectorAll("input, select")) i.disabled = on;
}

$("#svc-version").onchange = () => {
  noteService();
  drawLicense();
};
$("#svc-port").oninput = (e) => (e.target.value = e.target.value.replace(/\D/g, "").slice(0, 5));
$("#svc-cancel").onclick = () => $("#new-service").close();
$("#svc-name").oninput = (e) => {
  e.target.value = e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, "-");
  noteService();
};
$("#svc-form").onsubmit = async (e) => {
  e.preventDefault();
  const name = $("#svc-name").value.trim();
  const app = $("#svc-app").value;
  // Ports below 1024 need root; above 65535 don't exist. Say so before anything is installed.
  const port = Number.parseInt($("#svc-port").value, 10);
  if (!(port >= 1024 && port <= 65535)) {
    $("#svc-error").textContent = "Use a port between 1024 and 65535.";
    $("#svc-error").hidden = false;
    return;
  }
  setServiceBusy(true);
  try {
    services = await invoke("add_service", {
      kind: svcKind,
      name: name || null,
      port,
      version: $("#svc-version").value || null,
      tags: [],
      apps: app ? [app] : [],
      acceptLicense: ($("#svc-license-check") as HTMLInputElement).checked,
    });
    $("#new-service").close();
    keptNote = "";
    svcSelected = name;
    localStorage.setItem("service", name);
    if (mode !== "services") setMode("services");
    else renderMain();
    renderModes();
  } catch (err) {
    $("#svc-error").textContent = String(err);
    $("#svc-error").hidden = false;
    setServiceBusy(false);
  }
};

renderModes();
window.loadServices();

/* ---------- What's inside: databases, keys, messages ---------- */

export const contents = new Map<string, any>(); // service id -> last contents
const chosenDb: Record<string, string> = {}; // service id -> database used in the .env lines

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

const SYSTEM_DBS = new Set(["information_schema", "mysql", "performance_schema", "sys", "postgres", "template1"]);
const keysOpen = new Set<string>(); // Redis services whose keys are shown

/** Redis's used_memory_human ("911.12K", "1.20M", "856B") as "911 KB", "1.2 MB". */
export function memText(m: string | null) {
  const x = /^([\d.]+)([KMGT]?)B?$/i.exec(m ?? "");
  if (!x) return m ?? "–";
  const n = Number(x[1]);
  const unit = `${x[2].toUpperCase()}B`;
  return `${n < 10 && x[2] ? String(Math.round(n * 10) / 10) : Math.round(n)} ${unit}`;
}

async function renderContents(svc, root, action?) {
  // Only a database list needs its heading; keys and messages say what they are.
  $(".svc-contents-head", root).hidden = !svc.running || !["mysql", "postgres"].includes(svc.kind);
  const box = $(".svc-contents", root);
  if (!svc.running) {
    box.replaceChildren(el("p", "procs-note", `Start ${svc.id} to see what’s inside.`));
    contents.delete(svc.id);
    return;
  }
  const cached = contents.get(svc.id);
  if (cached && !action) drawContents(svc, root, cached);
  else if (!cached) box.replaceChildren(el("p", "procs-note", "Looking inside…"));
  try {
    const c = action ? await invoke("service_data", action) : await invoke("service_contents", { id: svc.id });
    contents.set(svc.id, c);
    if (root.dataset.service === svc.id) drawContents(svc, root, c);
  } catch (e) {
    if (action) showError(String(e));
    else box.replaceChildren(el("p", "procs-note", String(e)));
  }
}

function drawContents(svc, root, c) {
  const box = $(".svc-contents", root);
  const list = el("div", "group procs-list svc-list");
  const line = (name, value, ...buttons) => {
    const row = svcLine(name, value, ...buttons);
    list.append(row);
    return row;
  };
  const act = (label: string, fn: () => unknown, cls?: string) => {
    const b = el("button", cls, label);
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
      $(".svc-copy", root).onclick = (e) => copy(env, e.currentTarget);
    };
    const system = c.databases.filter(([n]) => SYSTEM_DBS.has(n));
    for (const [name, size] of c.databases.filter(([n]) => !SYSTEM_DBS.has(n))) {
      // Drop… (shown on hover) first, so Open lines up with Create at the right edge.
      const buttons: any[] = [
        act(
          "Drop…",
          async () => {
            if (await confirm(`Drop ${name}?`, `Every table in ${name} is deleted. This can’t be undone.`, "Drop"))
              renderContents(svc, root, { id: svc.id, action: "drop", name });
          },
          "danger hover-only",
        ),
      ];
      if (browsable()) buttons.push(browseButton(svc.id, name, ""));
      if (svc.clients[0]) buttons.push(act("Open", () => openClient(svc, svc.clients[0], name)));
      line(name, bytes(size), ...buttons);
    }
    // The server's own databases: one quiet line instead of a row each.
    if (system.length) {
      const row = line("Server databases", `${bytes(system.reduce((a, [, b]) => a + b, 0))} · ${system.map(([n]) => n).join(", ")}`);
      row.classList.add("muted");
    }
    // New database: name field plus Create.
    const row = el("div", "proc svc-row svc-new");
    const input = el("input");
    input.placeholder = "New database name";
    input.spellcheck = false;
    input.setAttribute("aria-label", "New database name");
    const create = () => {
      const name = input.value.trim();
      if (name) renderContents(svc, root, { id: svc.id, action: "create", name });
    };
    input.onkeydown = (e) => {
      if (e.key === "Enter") create();
    };
    const actions = el("span", "proc-actions");
    actions.append(act("Create", create));
    row.append(input, actions);
    list.append(row);
  } else if (svc.kind === "redis") {
    // One line, "12 keys · 911 KB memory"; with keys, a click shows them per database.
    const total = c.keys.reduce((a, [, n]) => a + n, 0);
    const open = keysOpen.has(svc.id) && c.keys.length > 0;
    const sum = el(c.keys.length ? "button" : "div", "proc svc-row svc-summary");
    if (c.keys.length) {
      sum.setAttribute("aria-expanded", String(open));
      sum.innerHTML = `<svg class="chevron" viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 2.5 8 6l-3.5 3.5"/></svg>`;
      sum.title = open ? "Hide the keys" : "Show the keys";
      sum.onclick = () => {
        keysOpen[open ? "delete" : "add"](svc.id);
        drawContents(svc, root, c);
      };
    }
    sum.append(el("span", "proc-name", `${total} ${total === 1 ? "key" : "keys"} · ${memText(c.memory)} memory`));
    list.append(sum);
    if (open) {
      const rows = c.keys.map(([db, keys]) => line(db, `${keys} ${keys === 1 ? "key" : "keys"}`));
      for (const r of rows) r.classList.add("svc-sub");
      // Flush All… on the last database's row.
      rows.at(-1).lastChild.append(
        act(
          "Flush All…",
          async () => {
            if (await confirm(`Flush ${svc.id}?`, "Every key in every database is deleted.", "Flush"))
              renderContents(svc, root, { id: svc.id, action: "empty" });
          },
          "danger",
        ),
      );
    }
  } else if (svc.kind === "versitygw" || svc.kind === "silo") {
    // Buckets are folders in the data folder; make more from an app or the web console.
    for (const [name, size] of c.buckets) line(name, bytes(size));
    if (!c.buckets.length) line("No buckets", null).classList.add("muted");
  } else {
    // The inbox opens from the header's envelope.
    const n = c.messages ?? 0;
    const buttons: HTMLElement[] = [];
    if (n)
      buttons.push(
        act(
          "Delete All…",
          async () => {
            if (await confirm(`Delete ${n} messages?`, `Every message caught by ${svc.id} is deleted.`, "Delete"))
              renderContents(svc, root, { id: svc.id, action: "empty" });
          },
          "danger",
        ),
      );
    line(n === 1 ? "1 message" : `${n} messages`, null, ...buttons);
  }
  box.replaceChildren(list);
}

late.services = () => services;
late.browseButton = browseButton;
late.browsable = browsable;
late.renderServiceList = renderServiceList;
late.selectService = selectService;
late.serviceKeys = serviceKeys;
late.renderService = renderService;
late.copy = copy;
late.openAddService = openAddService;
late.renderModes = renderModes;
