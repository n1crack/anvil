import { $, call, el, invoke, selected, snap, tab } from "./app";
import { inboxView, type Inbox } from "./inbox";
import { shown } from "./live";

/* Mail tab: the mail catcher's inbox for the app (Settings › Plugins › Mail catcher), live
 * while the tab is shown, under a line saying how Anvil catches it and a switch to leave
 * the app to its own mail setup. app.ts calls mailTab() from showTab. */

let view: Inbox | null = null;
let timer: ReturnType<typeof setInterval> | null = null;

window.mailTab = (root, visible) => {
  const panel = $('[data-panel="mail"]', root);
  const s = snap?.sites.find((x) => x.name === selected);
  const c = snap?.catcher;
  if (!visible || !panel || !s || !c) return poll(false);
  if (view?.inbox !== s.name) {
    view = inboxView(s.name, invoke, "No mail yet. What the app sends shows up here.");
    view.load();
  }
  const caught = c.caught.includes(s.name);
  const head = el("div", "mail-head");
  const how = el(
    "p",
    "mail-how muted",
    caught
      ? `Anvil catches ${s.name}’s mail: its processes and PHP get MAIL_HOST=127.0.0.1, MAIL_PORT=${c.port} and MAIL_USERNAME=${s.name}, over its .env. A running process gets them when it restarts.`
      : `${s.name} sends mail with its own settings; Anvil doesn’t catch it.`,
  );
  const sw = el("label", "mail-catch");
  const box = el("input");
  box.type = "checkbox";
  box.setAttribute("role", "switch");
  box.checked = caught;
  box.onchange = () => call("set_mail_caught", { site: s.name, on: box.checked });
  sw.append(box, " Catch mail");
  head.append(how, sw);
  const parts = [head];
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
