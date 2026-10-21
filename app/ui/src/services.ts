import type { ServiceRow } from "./types";
import {
  $,
  M,
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
import { icon } from "./icons";
import { late } from "./late";
import { picker } from "./picker";
import { fileManager, keys } from "./platform";
import { confirmPop } from "./processes";

/* Services in the main window: list in the sidebar, details on the right, and
 * the Add Service sheet. Shares $, snap, invoke, el and friends with app.js. */

export let services: ServiceRow[] = [];
let svcSelected = localStorage.getItem("service");
// After a removal that kept the data: a muted line over the list says where it is.
let keptNote = "";
const ABBR = { mysql: "My", postgres: "Pg", redis: "Rd", mailpit: "Mp", versitygw: "S3", silo: "Si", docker: "Dk", other: "Ex" };
/** Not a program of Anvil's: a Docker container, a server elsewhere. */
const outside = (svc) => svc.kind === "docker" || svc.kind === "external";
/** A server elsewhere: no Start or Stop, its dot says whether it answers. */
const external = (svc) => svc.kind === "external";

// A service's last failed action, by id: said on its page, under its name.
const svcErrors = new Map<string, string>();

async function serviceCall(cmd: string, args?: Record<string, unknown>) {
  const id = typeof args?.id === "string" ? args.id : null;
  try {
    services = await invoke(cmd, args);
    showError(null, cmd);
    if (id) svcErrors.delete(id);
  } catch (e) {
    // On the service's page when it is shown; else in the banner.
    if (id && id === svcSelected && mode === "services") svcErrors.set(id, String(e));
    else showError(String(e), cmd);
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

/** Apps a service can be listed for: parked or linked, as New application offers; not a
 * worktree (it shares its app's) nor an alias (another name for an app's folder). */
const ownApp = (s) => late.mayHaveWorktrees(s);

export function renderModes() {
  for (const b of document.querySelectorAll<HTMLElement>(".modes [role=tab]")) {
    b.setAttribute("aria-selected", String(b.dataset.mode === mode));
  }
  const running = services.filter((x) => x.running).length;
  $("#svc-running").textContent = running ? running : "";
  $("#svc-running").title = running ? `${running} running` : "";
  const apps = mode === "apps";
  $("#search").placeholder = apps ? "Search applications" : "Search services";
  $("#add").title = apps ? "Add applications" : keys("Add a service (⌘N)");
  $("#add").setAttribute("aria-label", $("#add").title);
  $("#list").setAttribute("aria-label", apps ? "Applications" : "Services");
}

for (const b of document.querySelectorAll<HTMLElement>(".modes [role=tab]")) b.onclick = () => setMode(b.dataset.mode);

function serviceMark(svc, cls = "mark") {
  // An external one by what it speaks: My, Pg, Rd or Ex.
  const kind = external(svc) ? svc.speaks : svc.kind;
  const m = el("span", `${cls} svc-mark svc-${kind}`, ABBR[kind] ?? "?");
  m.setAttribute("aria-hidden", "true");
  return m;
}

function serviceRow(svc) {
  const b = el("button", "item");
  b.dataset.name = svc.id;
  b.setAttribute("aria-current", String(svc.id === svcSelected));
  const text = el("span", "text");
  text.append(el("span", "name", svc.id), el("span", "sub", subLine(svc)));
  // As an app's row: the run state is a badge on the mark, and the tags end where the
  // apps' kind labels do.
  const end = el("span", "end");
  for (const t of svc.tags.slice(0, 2)) end.append(el("span", "tag", t));
  const badge = svc.running ? "on" : svc.error ? "bad" : null;
  b.append(markBox(serviceMark(svc), badge, external(svc) && svc.running ? "Answers" : undefined), text, end);
  b.onclick = () => selectService(svc.id);
  b.oncontextmenu = (e) => {
    e.preventDefault();
    selectService(svc.id);
    serviceMenu(svc);
  };
  return b;
}

/** The line under a row's name: kind, version and port; a Docker one's image; where an
 * external one is. */
function subLine(svc) {
  if (external(svc)) return `${svc.name} at ${svc.host}:${svc.port}`;
  if (svc.kind === "docker") return svc.port ? `${svc.version} on port ${svc.port}` : svc.version;
  return `${svc.name} ${svc.version} on port ${svc.port}`;
}

/** A service row's menu: what its page offers, Rename and Remove last. An external one
 * has nothing to start, a Docker one no data folder or log of Anvil's. */
async function serviceMenu(svc) {
  const item = (text, action) => M.MenuItem.new({ text, action });
  const fail = (e) => showError(String(e));
  const run = (action) => () => serviceCall("service_run", { id: svc.id, action });
  const state: any[] = [];
  if (!external(svc)) {
    state.push(item(svc.running ? "Stop" : "Start", run(svc.running ? "stop" : "start")));
    if (svc.running) state.push(item("Restart", run("restart")));
  }
  const open = svc.clients.map((c) => item(`Open in ${c}`, () => openClient(svc, c, chosenDb[svc.id])));
  if (svc.web && svc.running)
    open.push(item(svc.kind === "mailpit" ? "Open Inbox" : "Open Web Console", () => invoke("open_url", { url: svc.web }).catch(fail)));
  if (!outside(svc)) {
    open.push(item(`Show Data in ${fileManager}`, () => invoke("open_service", { id: svc.id, what: "data" }).catch(fail)));
    if (svc.running) open.push(item("Show Log in Processes", () => invoke("show_processes", { select: `service:${svc.id}` }).catch(fail)));
  }
  const row = () => $(`#list .item[data-name="${CSS.escape(svc.id)}"]`);
  const last = [item("Rename…", () => renameService(svc)), item("Remove Service…", () => removeService(svc, row()))];
  const items = [state, open, last]
    .filter((g) => g.length)
    .flatMap((g, i) => (i ? [M.PredefinedMenuItem.new({ item: "Separator" }), ...g] : g));
  (await M.Menu.new({ items: await Promise.all(items) })).popup();
}

/** Rename…: the page's name becomes a field; Return or leaving it saves, Esc keeps the name. */
function renameService(svc) {
  const head = $("#site .site-name");
  if (!head || $("#site").dataset.service !== svc.id) return;
  const input = el("input", "site-name-edit") as HTMLInputElement;
  input.value = svc.id;
  input.spellcheck = false;
  input.setAttribute("aria-label", `New name for ${svc.id}`);
  input.oninput = () => (input.value = input.value.toLowerCase().replace(/[^a-z0-9-]/g, "-"));
  let done = false;
  const finish = async (save: boolean) => {
    if (done) return;
    done = true;
    const name = input.value.trim();
    // The heading back first: a focused field would hold off the redraw.
    input.replaceWith(head);
    if (!save || !name || name === svc.id) return;
    try {
      services = await invoke("rename_service", { id: svc.id, name });
      svcErrors.delete(svc.id);
      svcSelected = name;
      localStorage.setItem("service", name);
    } catch (e) {
      // Said under the name, where it was typed.
      svcErrors.set(svc.id, String(e));
    }
    renderServiceList();
    delete $("#site").dataset.service;
    renderService();
    renderModes();
  };
  input.onkeydown = (e) => {
    if (e.key === "Enter" || e.key === "Escape") {
      e.preventDefault();
      finish(e.key === "Enter");
    }
  };
  input.onblur = () => finish(true);
  head.replaceWith(input);
  input.focus();
  input.select();
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
    const x = el("button");
    x.append(icon("x"));
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
        "Add MySQL, PostgreSQL, Redis, Mailpit or S3 storage. Each one gets its own name, port and data, and you can add as many as you need. A Docker container or a server elsewhere can be added too.",
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
  // An external one isn't run: whether it answers.
  const ext = external(svc);
  $(".state-text", root).textContent = ext ? (svc.running ? "Answers" : "Not answering") : svc.running ? "Running" : "Stopped";
  $('[data-svc="start"]', root).hidden = svc.running || ext;
  $('[data-svc="stop"]', root).hidden = !svc.running || ext;
  for (const b of root.querySelectorAll("[data-svc]")) {
    b.onclick = () => serviceCall("service_run", { id: svc.id, action: b.dataset.svc });
  }
  const err = $(".run-error", root);
  const failed = svcErrors.get(svc.id) ?? svc.error ?? svc.problem;
  err.textContent = failed ?? "";
  err.hidden = !failed;

  const inbox = $('[data-svc-open="inbox"]', root);
  inbox.hidden = !svc.web;
  inbox.disabled = !svc.running;
  if (svc.kind !== "mailpit") {
    inbox.title = "Open the web console; sign in with the keys in Connect";
    inbox.setAttribute("aria-label", "Open the web console");
  }
  // The data folder opens in Finder: the outline folder icon, as on an app's page.
  const data = $('[data-svc-open="data"]', root);
  data.title = `Show ~/.anvil/data/${svc.id} in ${fileManager}`;
  data.setAttribute("aria-label", String(data.title));
  inbox.onclick = () => invoke("open_url", { url: svc.web }).catch((e) => showError(String(e)));
  data.onclick = () => invoke("open_service", { id: svc.id, what: "data" }).catch((e) => showError(String(e)));
  // The log is in the Processes window, on its Services tab, while the service runs.
  const log = $('[data-svc-open="log"]', root);
  log.title = svc.running ? "Show the log in Processes" : "Not running";
  log.setAttribute("aria-label", svc.running ? `Show the log of ${svc.id} in Processes` : "Log: not running");
  log.setAttribute("aria-disabled", String(!svc.running));
  log.onclick = () => svc.running && invoke("show_processes", { select: `service:${svc.id}` }).catch((e) => showError(String(e)));
  // Docker keeps a container's data and log; an external one's are where it runs.
  data.hidden = log.hidden = outside(svc);
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
  // Set where it runs: the line under the name says it.
  $("#svc-port-edit", root).closest(".row").hidden = outside(svc);
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
  for (const s of snap.sites.filter((s) => ownApp(s) && !svc.apps.includes(s.name))) {
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
  // Nothing to connect with (a Docker container, an "other" server): no Connect.
  $(".connect-head", root).hidden = $(".env", root).hidden = $(".svc-apps-hint", root).hidden = !env;
  drawWrite(svc, root);

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
  $(".svc-remove", root).onclick = (e) => removeService(svc, e.currentTarget as HTMLElement);
}

const written = new Map<string, string>(); // "service/app" -> what the last write said

/** Under Connect: write its lines into each listed app's .env, asked first with the keys. */
function drawWrite(svc, root) {
  const box = $(".svc-write", root);
  const apps = svc.env ? svc.apps.filter((a) => snap.sites.some((s) => s.name === a)) : [];
  box.hidden = !apps.length;
  box.replaceChildren();
  const keys = envFor(svc)
    .split("\n")
    .map((l) => l.split("=")[0])
    .filter(Boolean);
  for (const a of apps) {
    const b = el("button", "inline-action", `Write to ${a}’s .env…`);
    b.onclick = () =>
      confirmPop(
        b,
        `Set ${keys.join(", ")} in ${a}’s .env? Its other lines stay.`,
        "Write",
        async () => {
          try {
            await invoke("service_write_env", { id: svc.id, site: a, database: chosenDb[svc.id] ?? null });
            written.set(`${svc.id}/${a}`, "Written. Running processes get it when they restart.");
          } catch (e) {
            written.set(`${svc.id}/${a}`, `!${String(e)}`);
          }
          if (root.dataset.service === svc.id) drawWrite(svc, root);
        },
        [],
        "left",
      );
    const said = written.get(`${svc.id}/${a}`);
    const line = el("div", "svc-write-line");
    line.append(b);
    if (said) line.append(el("span", said.startsWith("!") ? "error" : "muted", said.replace(/^!/, "")));
    box.append(line);
  }
}

/** The line under the name, as an app's: kind · version · port · data size (once
 * an app's Resources measured it) · what is notable about its state. */
function drawServiceMeta(svc, root) {
  if (outside(svc)) {
    const parts = external(svc)
      ? [`External ${svc.name === "External" ? "server" : svc.name}`, `${svc.host}:${svc.port}`]
      : [svc.adopted ? "Docker container" : "Docker image", svc.version, svc.port ? `port ${svc.port}` : "no published port"];
    if (svc.kind === "docker") parts.push(svc.adopted ? "Anvil never deletes it" : svc.enabled ? "starts with Anvil" : "");
    $(".meta", root).replaceChildren(...metaParts(parts.filter(Boolean).map((t) => el("span", null, t))));
    return;
  }
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

/** Remove a service: one bubble, two labelled answers. The main one (Return) keeps the data. */
function removeService(svc, at: HTMLElement) {
  const what =
    { mailpit: "caught mail", redis: "saved keys", versitygw: "buckets", silo: "buckets", docker: "data" }[svc.kind] ?? "databases";
  const used = svc.apps.length
    ? ` ${svc.apps.join(", ")} ${svc.apps.length === 1 ? "uses" : "use"} it; ${svc.apps.length === 1 ? "its" : "their"} .env stays as it is.`
    : "";
  // Not Anvil's to delete: it only leaves the list.
  if (svc.adopted || external(svc)) {
    const stays = svc.adopted ? `Its container stays in Docker as it is.` : `The server isn’t touched.`;
    confirmPop(
      at,
      `Remove ${svc.id} from Anvil? ${stays}${used}`,
      "Remove",
      () => serviceCall("remove_service", { id: svc.id, deleteData: false }),
      [],
      "left",
    );
    return;
  }
  const remove = (wipe: boolean) => {
    keptNote = wipe ? "" : `Kept ${svc.id}’s ${what} for when you add ${svc.id} again.`;
    serviceCall("remove_service", { id: svc.id, deleteData: wipe });
  };
  confirmPop(
    at,
    `Remove ${svc.id}? It stops and leaves Anvil.${used} Keep its ${what} to find them again if you add ${svc.id} later.`,
    "Remove, Keep Data",
    () => remove(false),
    [["Remove and Delete Data", () => remove(true)]],
    "left",
  );
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
  // A box for a container, a globe for a server elsewhere.
  docker: "M12 3 4 7v10l8 4 8-4V7l-8-4Zm-8 4 8 4 8-4M12 11v10",
  external: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Zm-9 9h18M12 3c2.4 2.5 3.6 5.5 3.6 9s-1.2 6.5-3.6 9c-2.4-2.5-3.6-5.5-3.6-9S9.6 5.5 12 3Z",
};
SVC_ICONS.silo = SVC_ICONS.versitygw;
SVC_ICONS.adopt = SVC_ICONS.docker;

/** What the Add sheet offers besides Anvil's own programs. */
const OUTSIDE_KINDS = [
  { kind: "docker", name: "Docker image", category: "Docker" },
  { kind: "adopt", name: "Docker container", category: "Docker" },
  { kind: "external", name: "External server", category: "External" },
].map((k) => ({ ...k, versions: [], installed_versions: [], licenses: [], unsupported: {} }));
const isOutside = (kind: string) => OUTSIDE_KINDS.some((k) => k.kind === kind);
/** The usual port of what an external server speaks. */
const EXT_PORT: Record<string, number> = { mysql: 3306, postgres: 5432, redis: 6379 };
const OUTSIDE_NOTE: Record<string, string> = {
  docker: "Pulled the first time; its data stays in Docker volumes.",
  adopt: "Anvil starts and stops it, and never deletes it.",
  external: "Anvil doesn’t run it; its dot is green while it answers.",
};
const svcIcon = (kind: string) =>
  `<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="${SVC_ICONS[kind] ?? SVC_ICONS.db}"/></svg>`;
const SVC_ABOUT: Record<string, string> = {
  mysql: "Relational database",
  postgres: "Relational database",
  redis: "Cache, queues and sessions",
  mailpit: "Catches the mail apps send",
  versitygw: "S3-compatible file storage",
  silo: "S3-compatible file storage",
  docker: "Runs an image, like mongo:7",
  adopt: "One that is there already",
  external: "A server Anvil doesn’t run",
};

const svcPicker = picker({
  list: $("#svc-kinds"),
  search: $("#svc-search"),
  sheet: $("#new-service"),
  form: $("#svc-form"),
  groups: [],
  choices: () => svcKinds.map((k) => ({ id: k.kind, name: k.name, about: kindAbout(k), group: k.category, icon: svcIcon(k.kind) })),
  onPick: (kind) => {
    svcKind = kind;
    localStorage.setItem("serviceKind", kind);
    fillService();
  },
  none: "No services match",
});

/** What a kind is, or, when this Mac is too old for every version, what it needs. */
function kindAbout(k) {
  const why = Object.values(k.unsupported ?? {});
  const need = /needs (macOS [\d.]+ or later)/.exec(String(why[0]))?.[1];
  return need && why.length === k.versions.length ? `Needs ${need}` : (SVC_ABOUT[k.kind] ?? k.category);
}

/** The add-service sheet; `want` starts it on a kind and version (a project's .anvil.toml). */
export async function openAddService(want?: { kind: string; version: string }) {
  try {
    svcKinds = [...(await invoke("service_kinds")), ...OUTSIDE_KINDS];
  } catch (e) {
    showError(String(e));
    return;
  }
  const app = $("#svc-app");
  app.replaceChildren(new Option("None", ""));
  for (const s of snap.sites.filter(ownApp)) app.add(new Option(`${s.name}.${snap.tld}`, s.name));
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
  // No license to accept for what can't be added on this Mac.
  if (!entry || k.unsupported?.[v]) {
    $(".svc-license").hidden = true;
    syncAdd();
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
    $("#svc-license-done").replaceChildren(
      icon("check"),
      ` Accepted on ${new Date(accepted).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })}`,
    );
  check.checked = false;
  syncAdd();
}

/** Add waits for the license, unless it was accepted before. */
function syncAdd() {
  // Docker's: once Docker answers (and for a container, one is picked).
  if (isOutside(svcKind)) {
    ($("#svc-create") as HTMLButtonElement).disabled =
      (svcKind !== "external" && dockerProblem !== "") || (svcKind === "adopt" && !$("#svc-container").value);
    return;
  }
  const k = svcKinds.find((x) => x.kind === svcKind);
  const accepted = k?.licenses?.find((l) => l[0] === $("#svc-version").value)?.[2];
  ($("#svc-create") as HTMLButtonElement).disabled =
    !!k?.unsupported?.[$("#svc-version").value] || (!accepted && !($("#svc-license-check") as HTMLInputElement).checked);
}
$("#svc-license-check").onchange = syncAdd;

function fillService() {
  const k = svcKinds.find((x) => x.kind === svcKind) ?? svcKinds[0];
  svcKind = k.kind;
  // Each kind's own fields: Anvil's programs have a version, Docker an image, a server a host.
  const group = isOutside(k.kind) ? k.kind : "anvil";
  for (const f of document.querySelectorAll<HTMLElement>("#svc-form [data-svc-for]"))
    f.hidden = !f.dataset.svcFor!.split(" ").includes(group);
  $("#svc-name").required = group === "anvil";
  $("#svc-name").placeholder = group === "anvil" ? "" : "Optional";
  $("#svc-port-note").textContent = "";
  if (group !== "anvil") {
    $("#svc-name").value = "";
    $("#svc-port").value = k.kind === "external" ? String(EXT_PORT[$("#svc-ext-kind").value] ?? "") : "";
    $("#svc-error").hidden = true;
    noteService();
    drawLicense();
    if (k.kind !== "external") checkDocker(k.kind);
    return;
  }
  $("#svc-name").value = k.suggested_name;
  const ver = $("#svc-version");
  ver.replaceChildren();
  for (const v of k.versions) {
    const tags = [
      k.unsupported?.[v] ? "not on this Mac" : v === k.default_version && "recommended",
      k.installed_versions.includes(v) && "installed",
    ].filter(Boolean);
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
}

/** One quiet line under the name: what gets downloaded, the port and where data goes. */
function noteService() {
  if (isOutside(svcKind)) {
    $("#svc-note").textContent = OUTSIDE_NOTE[svcKind];
    return;
  }
  const k = svcKinds.find((x) => x.kind === svcKind);
  const v = $("#svc-version").value;
  const name = $("#svc-name").value || "…";
  // This Mac is too old for it: why, before anything is downloaded.
  const why = k.unsupported?.[v];
  const fetch =
    why || k.installed_versions.includes(v)
      ? ""
      : k.kind === "redis"
        ? `Redis ${v} is built from source first (about a minute). `
        : `${k.name} ${v} is downloaded first. `;
  $("#svc-note").textContent = `${fetch}Data in ~/.anvil/data/${name}`;
  $("#svc-error").textContent = why ?? "";
  $("#svc-error").hidden = !why;
}

// Why Docker can't be used, said in the sheet ("" once it answered, null while asked).
let dockerProblem: string | null = null;
// The containers Docker listed, for the Container choice.
let dockerSeen: any[] = [];

/** Ask Docker for its containers: the Container choice, or inline why Docker can't be
 * used. Anvil never installs it. */
async function checkDocker(kind: string) {
  dockerProblem = null;
  syncAdd();
  const err = $("#svc-error");
  const pick = $("#svc-container") as HTMLSelectElement;
  try {
    dockerSeen = await invoke("docker_containers");
    dockerProblem = "";
  } catch (e) {
    const why = String(e);
    dockerSeen = [];
    dockerProblem = /installed/.test(why)
      ? "Docker isn’t installed. Anvil uses the Docker on this computer and doesn’t install it."
      : /running/.test(why)
        ? "Docker isn’t running. Start it, then pick this again."
        : why;
  }
  if (svcKind !== kind) return;
  pick.replaceChildren(...dockerSeen.map((c) => new Option(`${c.name} (${c.image}, ${c.state})`, c.name)));
  const none = kind === "adopt" && !dockerProblem && !dockerSeen.length;
  err.textContent = dockerProblem || (none ? "Docker has no container that isn’t a service already." : "");
  err.hidden = !err.textContent;
  syncAdd();
}
$("#svc-ext-kind").onchange = () => ($("#svc-port").value = String(EXT_PORT[$("#svc-ext-kind").value] ?? ""));

/** What the sheet sends for the chosen kind, or why it can't yet. */
function addArgs(name: string | null, port: number, apps: string[]): [string, Record<string, unknown>] | string {
  const val = (sel: string) => ($(sel) as HTMLInputElement).value.trim();
  const portOk = (lowest: number) => port >= lowest && port <= 65535;
  if (svcKind === "docker") {
    if (!val("#svc-image")) return "Give the image to run, like mongo:7.";
    if (!portOk(1024)) return "Use a port between 1024 and 65535.";
    const inside = Number.parseInt(val("#svc-cport"), 10);
    const env = val("#svc-env")
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const image = val("#svc-image");
    return ["add_docker_service", { name, image, container: null, port, containerPort: Number.isNaN(inside) ? null : inside, env, apps }];
  }
  if (svcKind === "adopt") {
    const c = dockerSeen.find((x) => x.name === val("#svc-container"));
    if (!c) return "Pick a container.";
    return ["add_docker_service", { name, image: null, container: c.name, port: c.port, containerPort: c.container_port, env: [], apps }];
  }
  if (svcKind === "external") {
    if (!val("#svc-host")) return "Give the host, like db.example.com.";
    if (!portOk(1)) return "Use a port between 1 and 65535.";
    const opt = (sel: string) => val(sel) || null;
    const password = ($("#svc-password") as HTMLInputElement).value || null;
    const kind = val("#svc-ext-kind");
    return [
      "add_external_service",
      { name, kind, host: val("#svc-host"), port, user: opt("#svc-user"), password, database: opt("#svc-database"), apps },
    ];
  }
  if (!portOk(1024)) return "Use a port between 1024 and 65535.";
  const version = $("#svc-version").value || null;
  const acceptLicense = ($("#svc-license-check") as HTMLInputElement).checked;
  return ["add_service", { kind: svcKind, name, port, version, tags: [], apps, acceptLicense }];
}

let progressTimer: ReturnType<typeof setInterval> | undefined;

function setServiceBusy(on) {
  $("#svc-create").disabled = on;
  if (!on) syncAdd();
  $("#svc-create").textContent = on ? "Adding…" : "Add";
  for (const i of $("#svc-form").querySelectorAll("input, select, textarea")) i.disabled = on;
  // A first add downloads (Redis builds) for a while: what it is at, beside the buttons.
  clearInterval(progressTimer);
  $("#svc-progress").textContent = "";
  if (on) progressTimer = setInterval(showProgress, 500);
}

async function showProgress() {
  const p = await invoke("service_installing").catch(() => null);
  if (!$("#svc-create").disabled) return;
  $("#svc-progress").textContent = !p
    ? ""
    : p.downloaded != null
      ? `Downloading ${p.what}… ${bytes(p.downloaded)}`
      : p.build
        ? `Building ${p.what} from source…`
        : `Downloading ${p.what}…`;
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
  // Ports below 1024 need root (an external server's may be anything); above 65535 don't
  // exist. Said before anything is installed.
  const port = Number.parseInt($("#svc-port").value, 10);
  const ask = addArgs(name || null, port, app ? [app] : []);
  if (typeof ask === "string") {
    $("#svc-error").textContent = ask;
    $("#svc-error").hidden = false;
    return;
  }
  setServiceBusy(true);
  // A name left empty is picked by Anvil: the new one is the one that wasn't there.
  const before = new Set(services.map((s) => s.id));
  try {
    services = await invoke(...ask);
    $("#new-service").close();
    keptNote = "";
    svcSelected = services.find((s) => !before.has(s.id))?.id ?? name;
    localStorage.setItem("service", svcSelected ?? "");
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

/** Docker and external services change outside Anvil (a container stopped in a terminal, a
 * server going down): while services show, asked again every 5 s, redrawn only when
 * something changed. Anvil answers from what it last saw, so this stays cheap. */
export async function pollOutside() {
  if (document.hidden || mode !== "services" || !services.some(outside)) return;
  const next = await invoke("services").catch(() => null);
  if (!next || JSON.stringify(next) === JSON.stringify(services)) return;
  services = next;
  renderMain();
  renderModes();
}
setInterval(pollOutside, 5000);

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
  // Anvil doesn't look inside what it doesn't run.
  if (outside(svc)) {
    $(".svc-contents-head", root).hidden = true;
    box.replaceChildren();
    return;
  }
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
    // A failed Create, Drop or Flush: said above the contents it was asked from.
    if (action && cached) box.prepend(el("p", "procs-note error", String(e)));
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
  const act = (label: string, fn: (b: HTMLElement) => unknown, cls?: string) => {
    const b = el("button", cls, label);
    b.onclick = () => fn(b);
    return b;
  };

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
      drawWrite(svc, root);
    };
    const system = c.databases.filter(([n]) => SYSTEM_DBS.has(n));
    for (const [name, size] of c.databases.filter(([n]) => !SYSTEM_DBS.has(n))) {
      // Drop… (shown on hover) first, so Open lines up with Create at the right edge.
      const buttons: any[] = [
        act(
          "Drop…",
          (b) =>
            confirmPop(b, `Drop ${name}? Every table in it is deleted. This can’t be undone.`, "Drop", () =>
              renderContents(svc, root, { id: svc.id, action: "drop", name }),
            ),
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
      rows
        .at(-1)
        .lastChild.append(
          act(
            "Flush All…",
            (b) =>
              confirmPop(b, `Flush ${svc.id}? Every key in every database is deleted.`, "Flush", () =>
                renderContents(svc, root, { id: svc.id, action: "empty" }),
              ),
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
          (b) =>
            confirmPop(b, `Delete ${n === 1 ? "1 message" : `all ${n} messages`}? This can’t be undone.`, "Delete", () =>
              renderContents(svc, root, { id: svc.id, action: "empty" }),
            ),
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
