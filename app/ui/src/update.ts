// Updates: a newer release, or for a local build a newer binary on disk (see
// `update_state` in app/src/main.rs). Nothing installs until the user clicks.
const { invoke } = window.__TAURI__.core;

export type Release = { version: string; notes: string | null };

/** Restart into the update (or the rebuilt binary); progress and errors go into `status`. */
export async function restart(button: HTMLButtonElement, status: HTMLElement, relaunch: boolean) {
  button.disabled = true;
  status.classList.remove("error-text");
  status.textContent = relaunch ? "Restarting…" : "Downloading…";
  const off = await window.__TAURI__.event.listen<number | null>("update-progress", (e) => {
    status.textContent = e.payload === null ? "Downloading…" : e.payload >= 100 ? "Installing…" : `Downloading ${e.payload}%`;
  });
  try {
    // On success Anvil restarts and this never returns.
    await invoke(relaunch ? "relaunch" : "update_install");
  } catch (e) {
    status.textContent = String(e);
    status.classList.add("error-text");
    button.disabled = false;
  }
  off();
}

const el = (tag: string, cls: string | null, text: string) =>
  Object.assign(document.createElement(tag), { className: cls ?? "", textContent: text });

/** The main window's "Update available" / "Relaunch" at the right end of the titlebar; it
 * opens a bubble with what is new and Restart. `popAt` is the app's bubble (passed in so
 * Settings, which uses `restart`, doesn't load the main window's code). */
export function updateBar(popAt: (anchor: Element, pop: HTMLElement, focus?: HTMLElement | null) => () => void) {
  const bar = document.querySelector("#update") as HTMLElement;
  let close: (() => void) | null = null;
  let busy = false;
  // The bubble closes on the mousedown before the toggle's click: that click must not
  // open it again. Window capture runs before the bubble's own document listener.
  let wasOpen = false;
  window.addEventListener("mousedown", (e) => (wasOpen = bar.contains(e.target as Node) && !!document.querySelector(".update-pop")), true);
  const draw = async () => {
    if (busy) return;
    const st = await invoke("update_state").catch(() => null);
    const update: Release | null = st?.update ?? null;
    const relaunch = !update && !!st?.relaunch;
    bar.hidden = !update && !relaunch;
    const toggle = el("button", "update-toggle", update ? "Update" : "Relaunch") as HTMLButtonElement;
    // "available" goes in a narrow window, next to the setup buttons (style.css).
    if (update) toggle.append(el("span", "update-more", " available"));
    toggle.title = update ? "A new version of Anvil" : "A newer build of Anvil is on disk";
    toggle.onclick = () => {
      if (wasOpen) return (wasOpen = false);
      const pop = el("div", "pop-confirm wide update-pop", "");
      const note = el("p", "update-note", update ? `Anvil ${update.version} is ready.` : "A newer build of Anvil is on disk.");
      const what = el("p", "muted", "Anvil restarts; the apps running now start again.");
      const later = el("button", null, "Later") as HTMLButtonElement;
      const go = el("button", "primary", update ? "Restart to update" : "Restart") as HTMLButtonElement;
      if (update) {
        const news = el("button", "link", "What's new") as HTMLButtonElement;
        news.onclick = () => {
          close?.();
          invoke("show_updates");
        };
        note.append(" ", news);
      }
      pop.append(note, what, later, go);
      toggle.setAttribute("aria-expanded", "true");
      const shut = popAt(toggle, pop, go);
      close = () => {
        if (busy) return;
        shut();
        toggle.setAttribute("aria-expanded", "false");
      };
      // A click elsewhere removes the bubble on its own; the toggle follows.
      new MutationObserver((_, o) => {
        if (!pop.isConnected) {
          toggle.setAttribute("aria-expanded", "false");
          o.disconnect();
        }
      }).observe(document.body, { childList: true });
      later.onclick = close;
      go.onclick = async () => {
        busy = true;
        later.disabled = true;
        await restart(go, what, relaunch);
        busy = false;
        later.disabled = false;
      };
    };
    bar.replaceChildren(toggle);
  };
  window.__TAURI__.event.listen("update", draw);
  draw();
}
