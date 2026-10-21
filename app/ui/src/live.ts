/* What every window shares about being live: whether it can be seen, so polls skip their
 * turn when not, and whose error its banner shows. */

const win = (window as any).__TAURI__?.window?.getCurrentWindow?.();
let focused = document.hasFocus();
let visible = true;
let checked = 0;

function check() {
  checked = Date.now();
  win?.isVisible().then(
    (v: boolean) => (visible = v),
    () => {},
  );
}
win
  ?.onFocusChanged(({ payload }: { payload: boolean }) => {
    focused = payload;
    if (focused) visible = true;
    else check();
  })
  ?.catch?.(() => {});

/** False while the window can't be seen. `document.hidden` alone isn't trusted: ⌘Q hides
 * the main window through Tauri, and WebKit may not report that. Without the focus, Tauri
 * is asked again at most every 2 s (another Anvil window had it when this one was hidden). */
export function shown() {
  if (document.hidden) return false;
  if (!focused && Date.now() - checked > 2000) check();
  return visible;
}

/** An agent action's local time: "14:02" today, "Oct 20, 14:02" on an earlier day. */
export function clock(at: number, now = new Date()) {
  const d = new Date(at * 1000);
  const time = d.toTimeString().slice(0, 5);
  if (d.toDateString() === now.toDateString()) return time;
  return `${d.toLocaleDateString("en-US", { month: "short", day: "numeric" })}, ${time}`;
}

/** The banner's errors by owner (usually the command): `msg` for `owner`, or null to clear
 * that owner's error only, so a poll that works doesn't hide an action that failed.
 * Returns the newest error left, the one to show. */
export function ownError(errors: Map<string, string>, msg: string | null | undefined, owner: string) {
  errors.delete(owner);
  if (msg) errors.set(owner, msg);
  return [...errors.values()].at(-1) ?? "";
}
