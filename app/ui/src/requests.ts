import { $, el, inlineError, invoke, seg, selected, snap, tab } from "./app";
import { follower } from "./console";
import { shown } from "./live";

/* Requests tab: what Caddy served for the app (src/requests.rs), newest first, live while
 * the tab is shown. app.ts calls requestsTab() from showTab. */

const STATUSES: [string, string][] = [
  ["all", "All"],
  ["2xx", "2xx"],
  ["3xx", "3xx"],
  ["4xx", "4xx"],
  ["5xx", "5xx"],
];
const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const MAX_ROWS = 2000; // kept while new ones come in
let rv: any = null; // the view: { site, host, node, ... }
let timer: ReturnType<typeof setTimeout> | null = null;

window.requestsTab = (root, visible) => {
  const panel = $('[data-panel="requests"]', root);
  if (!visible || !panel) return poll(false);
  const s = snap.sites.find((x) => x.name === selected);
  if (!s) return;
  if (rv?.site !== s.name) rv = build(s);
  panel.replaceChildren(rv.node);
  rv.list.restore();
  poll(true);
};

const filter = (v) => ({ status: v.status === "all" ? null : v.status, method: v.method || null, query: v.query, assets: v.assets });
const filtered = (v) => v.status !== "all" || v.method || v.query || v.assets;

function build(s) {
  const node = el("div", "logs");
  node.innerHTML = `
    <div class="logs-bar req-bar">
      <input class="logs-search" type="search" placeholder="Filter by path" aria-label="Filter by path" spellcheck="false">
      <select class="logs-source req-method-pick" aria-label="Method"><option value="">All methods</option>${METHODS.map((m) => `<option>${m}</option>`).join("")}</select>
      <label class="req-assets"><input type="checkbox"> Show assets</label>
      <button class="req-clear" hidden>Clear Filters</button>
    </div>
    <p class="logs-status muted"></p>
    <ol class="logs-list req-list"></ol>
    <button class="logs-more req-more" hidden>Load older</button>`;
  const v: any = { site: s.name, host: s.host, node, status: "all", method: "", query: "", assets: false, token: 0, from: 0, page: null };
  const search = $(".logs-search", node) as HTMLInputElement;
  const method = $(".req-method-pick", node) as HTMLSelectElement;
  const assets = $(".req-assets input", node) as HTMLInputElement;
  const statuses = seg("Status", STATUSES, v.status, (id) => {
    v.status = id;
    load(v);
  });
  search.after(statuses);
  let debounce;
  search.oninput = () => {
    clearTimeout(debounce);
    debounce = setTimeout(() => {
      v.query = search.value.trim();
      load(v);
    }, 300);
  };
  method.onchange = () => {
    v.method = method.value;
    load(v);
  };
  assets.onchange = () => {
    v.assets = assets.checked;
    load(v);
  };
  $(".req-clear", node).onclick = () => {
    Object.assign(v, { status: "all", method: "", query: "", assets: false });
    search.value = method.value = "";
    assets.checked = false;
    for (const b of statuses.children) b.setAttribute("aria-pressed", String(b.dataset.id === "all"));
    load(v);
  };
  $(".req-more", node).onclick = () => more(v);
  const list = $(".req-list", node);
  v.list = follower(list, true);
  list.onclick = (e) => {
    const head = (e.target as HTMLElement).closest("button.log-head");
    if (head) toggle(v, head.parentElement);
  };
  load(v);
  return v;
}

async function load(v) {
  const token = ++v.token;
  $(".req-clear", v.node).hidden = !filtered(v);
  let page;
  try {
    page = await invoke("requests_read", { site: v.site, filter: filter(v), before: null });
  } catch (e) {
    if (token === v.token) empty(v, String(e), true);
    return;
  }
  if (token !== v.token) return;
  v.page = page;
  v.from = page.end_offset;
  v.trimmed = false;
  const list = $(".req-list", v.node);
  list.replaceChildren(...page.requests.map((r) => row(v, r)));
  v.list.end();
  if (!page.requests.length)
    empty(v, filtered(v) ? "Nothing matches." : "No requests yet. Open the app in the browser and they show up here.");
  $(".req-more", v.node).hidden = !page.truncated;
}

function empty(v, text, error = false) {
  $(".req-list", v.node).replaceChildren(el("li", error ? "empty error-text" : "empty", text));
  $(".req-more", v.node).hidden = true;
}

async function more(v) {
  if (v.trimmed) return load(v);
  const btn = $(".req-more", v.node);
  btn.disabled = true;
  const token = v.token;
  try {
    const page = await invoke("requests_read", { site: v.site, filter: filter(v), before: v.page.start_offset });
    if (token !== v.token) return;
    v.page = { ...page, end_offset: v.page.end_offset };
    $(".req-list", v.node).append(...page.requests.map((r) => row(v, r)));
    btn.hidden = !page.truncated;
    inlineError(btn, null);
  } catch (e) {
    inlineError(btn, String(e));
  } finally {
    btn.disabled = false;
  }
}

/** "0.4 ms", "12 ms", "1.25 s". */
export function took(ms: number) {
  if (ms < 1) return `${ms.toFixed(1)} ms`;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

/** "0 B", "812 B", "5.2 KB", "1.4 MB". */
function size(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1048576) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1048576).toFixed(1)} MB`;
}

const CLOCK = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", second: "2-digit" });

/** One row: time, method, status (its colour only as text), host when not the app's own,
 * path with query, duration, size. */
function row(v, r) {
  const li = el("li", "log req");
  li.req = r;
  const head = el("button", "log-head");
  head.setAttribute("aria-expanded", "false");
  const d = new Date(r.ts * 1000);
  const time = el("span", "log-time", CLOCK.format(d));
  time.title = d.toLocaleString();
  const path = el("span", "log-msg");
  const host = r.host.replace(/:\d+$/, "");
  if (host !== v.host) path.append(el("span", "req-host", host));
  path.append(r.uri);
  path.title = r.uri;
  head.append(
    time,
    el("span", "req-method", r.method),
    el("span", `req-status st-${Math.floor(r.status / 100)}`, String(r.status)),
    path,
    el("span", "req-dur", took(r.duration_ms)),
    el("span", "req-size", size(r.size)),
  );
  li.append(head);
  return li;
}

/** Headers as name and value lines; Caddy's REDACTED says "redacted", muted. */
function headers(list: [string, string[]][]) {
  const dl = el("dl", "req-headers");
  for (const [name, values] of list)
    for (const value of values) {
      const line = el("div");
      line.append(el("dt", null, name), value === "REDACTED" ? el("dd", "redacted", "redacted") : el("dd", null, value));
      dl.append(line);
    }
  return dl;
}

function toggle(v, li) {
  const head = $(".log-head", li);
  const open = head.getAttribute("aria-expanded") !== "true";
  head.setAttribute("aria-expanded", String(open));
  if (!open) return $(".log-detail", li)?.remove();
  const r = li.req;
  const box = el("div", "log-detail");
  const when = new Date(r.ts * 1000).toLocaleString();
  box.append(el("div", "log-meta", `${r.proto} · ${r.tls ? "TLS" : "no TLS"} · from ${r.remote} · ${when}`));
  // Caddy keeps the Host header apart from the others.
  box.append(el("h4", null, "Request headers"), headers([["Host", [r.host]], ...(Object.entries(r.req_headers) as [string, string[]][])]));
  box.append(el("h4", null, "Response headers"), headers(Object.entries(r.resp_headers) as [string, string[]][]));
  li.append(box);
}

let quiet = 0; // looks in a row that found nothing new

/** The wait before the next look: 250 ms after a new request, doubling while nothing new
 * comes, up to 1 s. */
export const nextIn = (quiet: number) => Math.min(1000, 250 * 2 ** quiet);

/** New requests, only while the tab is shown in a window that can be seen: with nothing
 * new that's a stat of the log and an empty read. */
export function poll(on: boolean) {
  clearTimeout(timer ?? undefined);
  timer = null;
  quiet = 0;
  if (on) next();
}

function next() {
  const t = setTimeout(async () => {
    await tick();
    if (timer === t) next(); // not stopped or restarted meanwhile
  }, nextIn(quiet));
  timer = t;
}

export async function tick() {
  const v = rv;
  if (!v?.page || !v.node.isConnected || v.node.closest("[hidden]")) return poll(false);
  if (!shown() || v.tailing) return;
  quiet++;
  const token = v.token;
  v.tailing = true;
  let page;
  try {
    page = await invoke("requests_tail", { site: v.site, filter: filter(v), from: v.from });
  } catch {
    return;
  } finally {
    v.tailing = false;
  }
  if (token !== v.token || rv !== v) return;
  v.from = page.end_offset;
  if (!page.requests.length) return;
  quiet = 0;
  const list = $(".req-list", v.node);
  list.querySelector(".empty")?.remove();
  v.list.keep(() => {
    list.prepend(...page.requests.map((r) => row(v, r)));
    const rows = list.querySelectorAll(".req");
    for (let i = MAX_ROWS; i < rows.length; i++) rows[i].remove();
    // What was dropped has no offset to read on from: Load older starts again.
    if (rows.length > MAX_ROWS) v.trimmed = true;
  });
  if (v.trimmed) $(".req-more", v.node).hidden = false;
  v.list.follow();
}

// app.ts may have drawn the tab before this file loaded.
if (tab === "requests") window.requestsTab($("#site"), true);
