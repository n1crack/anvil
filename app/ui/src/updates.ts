import "./platform";
import { restart, type Release } from "./update";

/* The Software Update window: a check on opening (and when opened again), what's new,
 * and Restart to update. */
const { invoke } = window.__TAURI__.core;
const $ = (sel: string): any => document.querySelector(sel);

const el = (tag: string, cls: string | null = null) => Object.assign(document.createElement(tag), { className: cls ?? "" });

/** `code` spans of a line as <code>, the rest as text. */
function inline(text: string) {
  const out: Node[] = [];
  text.split("`").forEach((part, i) => {
    if (!part) return;
    out.push(i % 2 ? Object.assign(el("code"), { textContent: part }) : document.createTextNode(part));
  });
  return out;
}

/** Release notes, markdown or the plain text latest.json carries: headings ("### Added",
 * or a short line without a full stop), bullets ("- ", "• ") and paragraphs. */
export function renderNotes(notes: string) {
  const out: HTMLElement[] = [];
  let list: HTMLElement | null = null;
  for (const raw of notes.split("\n")) {
    const line = raw.trim();
    if (!line) {
      list = null;
      continue;
    }
    const bullet = line.match(/^[-•]\s+(.*)$/);
    if (bullet) {
      if (!list) out.push((list = el("ul")));
      const li = el("li");
      li.append(...inline(bullet[1]));
      list.append(li);
      continue;
    }
    list = null;
    const heading = line.match(/^#+\s+(.*)$/)?.[1] ?? (line.length <= 40 && !/[.:;!?]$/.test(line) ? line : null);
    const node = el(heading ? "h2" : "p");
    node.append(...inline(heading ?? line));
    out.push(node);
  }
  return out;
}

let busy = false;

async function check() {
  if (busy) return;
  const title = $("#upd-title");
  const sub = $("#upd-sub");
  const status = $("#upd-status");
  const go = $("#upd-go");
  const notes = $("#upd-notes");
  title.textContent = "Checking…";
  sub.textContent = "";
  sub.classList.remove("error-text");
  status.textContent = "";
  status.classList.remove("error-text");
  go.hidden = $("#upd-retry").hidden = true;
  notes.hidden = true;
  const current = (await window.__TAURI__.app?.getVersion()) ?? "";
  let found: Release | null;
  try {
    found = await invoke("update_check");
  } catch (e) {
    // Under the title, wrapped: the reason, often a long address.
    title.textContent = "Couldn't check for updates";
    sub.textContent = String(e);
    sub.classList.add("error-text");
    $("#upd-retry").hidden = false;
    return;
  }
  if (!found) {
    title.textContent = "Anvil is up to date";
    sub.textContent = `Version ${current}`;
    $("#upd-close").textContent = "Close";
    return;
  }
  title.textContent = `Anvil ${found.version} is available`;
  sub.textContent = `You have ${current}. Anvil restarts; the apps running now start again.`;
  $("#upd-close").textContent = "Later";
  go.hidden = false;
  if (found.notes) {
    notes.replaceChildren(...renderNotes(found.notes));
    notes.hidden = false;
  }
}

$("#upd-close").onclick = () => window.__TAURI__.window.getCurrentWindow().close();
$("#upd-go").onclick = async () => {
  busy = true;
  $("#upd-close").disabled = true;
  await restart($("#upd-go"), $("#upd-status"), false);
  busy = false;
  $("#upd-close").disabled = false;
};
$("#upd-retry").onclick = check;
window.__TAURI__.event.listen("update-recheck", check);
check();
