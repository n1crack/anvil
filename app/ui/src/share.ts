import { $, callOrThrow, el, invoke, render, showError, snap } from "./app";
import { confirmPop } from "./processes";
import { copy } from "./services";

/* The Overview's Public address line (Settings › Plugins › Tunnel): Share opens a
 * Cloudflare quick tunnel to the app; its address, Copy and Stop show on the line. */

const LIMITS = "The address is temporary and changes every time; Cloudflare gives no uptime guarantee.";
const opening = new Set<string>(); // apps whose share_start hasn't answered (the first one downloads cloudflared)
const failed = new Map<string, string>(); // app -> why share_start refused

async function share(site: string) {
  opening.add(site);
  failed.delete(site);
  render();
  try {
    await callOrThrow("share_start", { site });
  } catch (e) {
    failed.set(site, String(e));
  }
  opening.delete(site);
  render();
}

window.renderShare = (s, root) => {
  const row = $(".share-row", root);
  const on = snap.tunnel && s.kind !== "compose" && s.kind !== "folder";
  row.hidden = !on;
  if (!on) return;
  const dd = $("dd", row);
  const t = snap.shares?.[s.name];
  const busy = opening.has(s.name) || t?.state === "starting";
  if (busy) {
    const b = el("span", "securing", "Opening…");
    b.prepend(el("span", "spin-dot"));
    return dd.replaceChildren(b);
  }
  if (t?.state === "ready") {
    const link = el("a", "share-url", t.url);
    link.href = "#";
    link.onclick = (e) => {
      e.preventDefault();
      invoke("open_url", { url: t.url }).catch((err) => showError(String(err)));
    };
    const c = el("button", "inline-action", "Copy");
    c.onclick = () => copy(t.url, c);
    const stop = el("button", "inline-action", "Stop");
    stop.onclick = () => callOrThrow("share_stop", { site: s.name }).catch((err) => failed.set(s.name, String(err)));
    const parts = [link, " ", c, stop];
    if (t.note) parts.push(el("small", "share-note muted", t.note));
    return dd.replaceChildren(...parts);
  }
  const b = el("button", "inline-action", "Share…");
  b.onclick = () => confirmPop(b, `Anyone with the address can open ${s.name} while it is shared. ${LIMITS}`, "Share", () => share(s.name));
  const why = t?.state === "failed" ? t.error : failed.get(s.name);
  dd.replaceChildren(b, ...(why ? [el("small", "share-error", why)] : []));
};
