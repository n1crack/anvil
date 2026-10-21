// Windows draws its own title bar and Mica (Windows 11's sidebar material) instead of
// macOS's overlay title bar and vibrancy; style.css adjusts under `html.windows`.
export const windows = navigator.userAgent.includes("Windows");
// Linux has neither: its window manager draws the title bar and the sidebar is opaque.
export const linux = !windows && navigator.userAgent.includes("Linux");
if (windows) document.documentElement.classList.add("windows");
else if (linux) document.documentElement.classList.add("linux");
/** macOS: ⌘ and Finder. Windows and Linux say Ctrl and their own file manager. */
const mac = !windows && !linux;

/** The key shortcuts are held with: ⌘ on macOS, Ctrl elsewhere (Linux's Super key
 * belongs to the window manager). */
export const mod = (e: KeyboardEvent | MouseEvent) => (mac ? e.metaKey : e.ctrlKey);

/** A shortcut as macOS writes it ("⌘N", "⌥⌘Q", "⌘-click"), as this platform does
 * ("Ctrl+N", "Ctrl+Alt+Q", "Ctrl+click"). */
export const keys = (s: string) =>
  !mac
    ? s
        .replace(/⌥⌘/g, "Ctrl+Alt+")
        .replace(/⌘-/g, "Ctrl+")
        .replace(/⌘/g, "Ctrl+")
    : s;

/** What shows a folder: Finder, on Windows File Explorer, on Linux the file manager. */
export const fileManager = windows ? "File Explorer" : linux ? "File Manager" : "Finder";

/** Text written for macOS as this platform says it: its shortcuts, file manager, and
 * "this computer" for "this Mac". */
export const say = (s: string) =>
  mac
    ? s
    : keys(s)
        .replace(/\bFinder\b/g, fileManager)
        .replace(/\bthis Mac\b/g, "this computer");

const macWords = /[⌘⌥]|Finder|this Mac/;

/** The page's own markup says ⌘ and Finder; elsewhere its text and tooltips are reworded. */
export function sayInPage(root: ParentNode = document.body) {
  if (mac || !root) return;
  for (const t of root.querySelectorAll("template")) sayInPage(t.content);
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walk.nextNode(); n; n = walk.nextNode()) if (macWords.test(n.nodeValue!)) n.nodeValue = say(n.nodeValue!);
  for (const e of root.querySelectorAll("[title], [aria-label]"))
    for (const a of ["title", "aria-label"]) {
      const v = e.getAttribute(a);
      if (v && macWords.test(v)) e.setAttribute(a, say(v));
    }
}
if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", () => sayInPage());
else sayInPage();
