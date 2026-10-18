/* A divider dragged to resize the pane beside it: the sidebar, the Dev and Containers
 * lists, the Processes window's log. */

type Opts = {
  /** localStorage key the width is kept under. */
  key: string;
  min: number;
  /** The widest it may be now (the window may have shrunk). */
  max: () => number;
  /** The width the pointer asks for. */
  at: (e: PointerEvent) => number;
  /** Show a width; null puts the default back. */
  apply: (px: number | null) => void;
};

/** Drag `handle` to resize, kept across launches; a double-click resets to the default. */
export function splitter(handle: HTMLElement, o: Opts) {
  const fit = (w: number) => Math.round(Math.max(o.min, Math.min(w, o.max())));
  let width = Number(localStorage.getItem(o.key)) || 0;
  if (width) o.apply(fit(width));
  handle.onpointerdown = (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    handle.setPointerCapture?.(e.pointerId);
    document.documentElement.classList.add("col-resizing");
    handle.onpointermove = (m) => o.apply((width = fit(o.at(m))));
    handle.onpointerup = handle.onlostpointercapture = () => {
      handle.onpointermove = handle.onpointerup = handle.onlostpointercapture = null;
      document.documentElement.classList.remove("col-resizing");
      if (width) localStorage.setItem(o.key, String(width));
    };
  };
  handle.ondblclick = () => {
    width = 0;
    localStorage.removeItem(o.key);
    o.apply(null);
  };
  return { fit };
}

/** A vertical divider element, labelled for assistive tech. */
export function handle(cls: string, label: string) {
  const h = Object.assign(document.createElement("div"), { className: `split-handle ${cls}` });
  h.setAttribute("role", "separator");
  h.setAttribute("aria-orientation", "vertical");
  h.setAttribute("aria-label", label);
  return h;
}

/** The Dev and Containers tabs: a divider between the list and the output, kept per tab. */
export function listSplitter(main: HTMLElement, key: string, min: number) {
  const h = handle("list-split", "Resize the list");
  splitter(h, {
    key,
    min,
    max: () => Math.max(min, main.clientWidth - 320),
    at: (m) => m.clientX - main.getBoundingClientRect().left - h.offsetWidth / 2,
    apply: (w) => (w ? main.style.setProperty("--list-w", `${w}px`) : main.style.removeProperty("--list-w")),
  });
  return h;
}

/** The main window's sidebar, by the border between it and the detail pane. */
export function sidebarSplitter() {
  const h = handle("sidebar-split", "Resize the sidebar");
  const body = document.body;
  splitter(h, {
    key: "sidebar-width",
    min: 220,
    max: () => Math.min(520, innerWidth - 480),
    at: (m) => m.clientX,
    apply: (w) => (w ? body.style.setProperty("--sidebar", `${w}px`) : body.style.removeProperty("--sidebar")),
  });
  body.append(h);
}
