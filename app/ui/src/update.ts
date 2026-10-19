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

/** The main window's "Update available" / "Relaunch", at the right end of the titlebar. */
export function updateBar() {
  const bar = document.querySelector("#update") as HTMLElement;
  let open = false;
  let busy = false;
  const draw = async () => {
    if (busy) return;
    const st = await invoke("update_state").catch(() => null);
    const update: Release | null = st?.update ?? null;
    const relaunch = !update && !!st?.relaunch;
    bar.hidden = !update && !relaunch;
    const toggle = el("button", "update-toggle", update ? "Update available" : "Relaunch");
    toggle.title = update ? "A new version of Anvil" : "A newer build of Anvil is on disk";
    toggle.setAttribute("aria-expanded", String(open));
    toggle.onclick = () => {
      open = !open;
      draw();
    };
    if (!open) return bar.replaceChildren(toggle);
    const note = el("span", "update-note", update ? `Version ${update.version}` : "A newer build is on disk");
    const go = el("button", null, update ? "Restart to update" : "Restart") as HTMLButtonElement;
    go.onclick = async () => {
      busy = true;
      await restart(go, note, relaunch);
      busy = false;
    };
    bar.replaceChildren(note, go, toggle);
  };
  window.__TAURI__.event.listen("update", draw);
  draw();
}
