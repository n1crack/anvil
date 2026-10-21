/* ---------- Native menus, opened where they were asked for ---------- */

// A menu popped up without a position goes where the system says the pointer is. Under
// Wayland (Linux) a window can't know that, so the menu landed anywhere. Every menu
// here opens from the page instead: at the last click, or under the focused element
// when the keyboard opened it. Linux measures that from the GTK window's corner, above
// its title bar, so the page's offset in the window (`menu_origin`) is added.

const api = window.__TAURI__.menu;
const dpi = (window.__TAURI__ as any).dpi;
const invoke = window.__TAURI__.core.invoke;

let pointer: { x: number; y: number } | null = null;
const track = (e: MouseEvent) => (pointer = { x: e.clientX, y: e.clientY });
addEventListener("pointerdown", track, true);
addEventListener("contextmenu", track, true);
addEventListener("keydown", () => (pointer = null), true);

const linux = navigator.userAgent.includes("Linux") && !navigator.userAgent.includes("Windows");

async function onWindow(x: number, y: number) {
  const [ox, oy] = linux ? await invoke<[number, number]>("menu_origin").catch(() => [0, 0]) : [0, 0];
  return new dpi.LogicalPosition(x + ox, y + oy);
}

async function here() {
  if (!dpi) return undefined;
  if (pointer) return onWindow(pointer.x, pointer.y);
  const r = document.activeElement?.getBoundingClientRect();
  return r ? onWindow(r.left, r.bottom + 4) : undefined;
}

export const M = {
  ...api,
  Menu: {
    ...api.Menu,
    async new(options) {
      const menu = await api.Menu.new(options);
      const popup = menu.popup.bind(menu);
      menu.popup = async (at?) => popup(at ? (linux ? await onWindow(at.x, at.y) : at) : await here());
      return menu;
    },
  },
};
