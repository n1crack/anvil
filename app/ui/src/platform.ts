// Windows draws its own title bar and Mica (Windows 11's sidebar material) instead of
// macOS's overlay title bar and vibrancy; style.css adjusts under `html.windows`.
export const windows = navigator.userAgent.includes("Windows");
if (windows) document.documentElement.classList.add("windows");

/** The key shortcuts are held with: ⌘ on macOS, Ctrl on Windows. */
export const mod = (e: KeyboardEvent | MouseEvent) => (windows ? e.ctrlKey : e.metaKey);

/** A shortcut as macOS writes it ("⌘N", "⌥⌘Q", "⌘-click"), as this platform does
 * ("Ctrl+N", "Ctrl+Alt+Q", "Ctrl+click"). */
export const keys = (s: string) =>
  windows
    ? s
        .replace(/⌥⌘/g, "Ctrl+Alt+")
        .replace(/⌘-/g, "Ctrl+")
        .replace(/⌘/g, "Ctrl+")
    : s;

/** What shows a folder: Finder, or on Windows File Explorer. */
export const fileManager = windows ? "File Explorer" : "Finder";

/** Text written for macOS as this platform says it: its shortcuts and file manager. */
export const say = (s: string) => (windows ? keys(s).replace(/\bFinder\b/g, fileManager) : s);

/** The page's own markup says ⌘ and Finder; on Windows its text and tooltips are reworded. */
export function sayInPage(root: ParentNode = document.body) {
  if (!windows || !root) return;
  for (const t of root.querySelectorAll("template")) sayInPage(t.content);
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walk.nextNode(); n; n = walk.nextNode()) if (/[⌘⌥]|Finder/.test(n.nodeValue!)) n.nodeValue = say(n.nodeValue!);
  for (const e of root.querySelectorAll("[title], [aria-label]"))
    for (const a of ["title", "aria-label"]) {
      const v = e.getAttribute(a);
      if (v && /[⌘⌥]|Finder/.test(v)) e.setAttribute(a, say(v));
    }
}
if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", () => sayInPage());
else sayInPage();
