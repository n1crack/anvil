import { $, call, el, invoke, selected, snap, tab } from "./app";
import { confirmPop, loadProcs, procCache } from "./processes";
import { inboxView, type Inbox } from "./inbox";
import { shown } from "./live";

/* Mail tab: the mail catcher's inbox for the app (Settings › Features › Mail catcher), live
 * while the tab is shown, under a line saying how Anvil catches it and a switch to leave
 * the app to its own mail setup. app.ts calls mailTab() from showTab. */

let view: Inbox | null = null;
let timer: ReturnType<typeof setInterval> | null = null;
// Apps whose catching changed while processes ran: they still send the old way until restarted.
const stale = new Set<string>();

window.mailTab = (root, visible) => {
  const panel = $('[data-panel="mail"]', root);
  const s = snap?.sites.find((x) => x.name === selected);
  const c = snap?.catcher;
  if (!visible || !panel || !s || !c) return poll(false);
  if (view?.inbox !== s.name) {
    view = inboxView(s.name, invoke, "No messages yet. What the app sends shows up here.");
    view.load();
  }
  const caught = c.caught.includes(s.name);
  const head = el("div", "mail-head");
  const how = el(
    "p",
    "mail-how muted",
    caught
      ? `Mail ${s.name} sends lands here, not in real inboxes. A running process picks this up when it restarts.`
      : `${s.name} sends mail with its own settings; Anvil doesn’t catch it.`,
  );
  // The how, for those who want it: what overrides the .env.
  if (caught) how.title = `MAIL_HOST=127.0.0.1, MAIL_PORT=${c.port} and MAIL_USERNAME=${s.name}, over its .env`;
  const sw = el("label", "mail-catch");
  const box = el("input");
  box.type = "checkbox";
  box.setAttribute("role", "switch");
  box.checked = caught;
  const running = () => (procCache.get(s.name)?.rows ?? []).filter((r) => r.running && r.kind !== "site");
  box.onchange = () => {
    if (running().length) stale.add(s.name);
    call("set_mail_caught", { site: s.name, on: box.checked });
  };
  sw.append(box, " Catch mail");
  head.append(how, sw);
  const parts = [head];
  // The processes that ran through the change: Restart, asked first since they stop a moment.
  const procs = running();
  if (!procs.length) stale.delete(s.name);
  if (stale.has(s.name)) {
    const names = procs.map((r) => r.name).join(", ");
    const line = el(
      "p",
      "mail-restart muted",
      `${names} still ${procs.length === 1 ? "sends" : "send"} mail the old way until restarted. `,
    );
    const b = el("button", "inline-action", procs.length === 1 ? "Restart…" : "Restart Them…");
    b.onclick = () =>
      confirmPop(b, `Restart ${names}? ${procs.length === 1 ? "It stops" : "They stop"} for a moment.`, "Restart", async () => {
        stale.delete(s.name);
        await Promise.all(procs.map((r) => loadProcs(s.name, "process_run", { name: r.id, action: "restart" })));
        window.mailTab($("#site"), true);
      });
    line.append(b);
    parts.push(line);
  }
  if (c.error) parts.push(el("p", "error mail-down", c.error));
  panel.replaceChildren(...parts, view.node);
  poll(true);
};

/** New mail every 2 s, only while the tab is shown in a window that can be seen. */
export function poll(on: boolean) {
  clearInterval(timer ?? undefined);
  timer = on ? setInterval(tick, 2000) : null;
}

export async function tick() {
  if (!view?.node.isConnected || view.node.closest("[hidden]")) return poll(false);
  if (shown()) await view.tick();
}

// app.ts may have drawn the tab before this file loaded.
if (tab === "mail") window.mailTab($("#site"), true);
