// Shortcut and file-manager wording on macOS and on Windows (the user agent decides).
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, describe, expect, test, vi } from "vite-plus/test";

const realAgent = navigator.userAgent;
async function as(agent: string) {
  Object.defineProperty(navigator, "userAgent", { value: agent, configurable: true });
  vi.resetModules();
  return import("../src/platform");
}
afterEach(() => {
  Object.defineProperty(navigator, "userAgent", { value: realAgent, configurable: true });
  document.documentElement.classList.remove("windows", "linux");
});

const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15";
const WIN = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/141.0";
const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const key = (k: string, mods: KeyboardEventInit) => new KeyboardEvent("keydown", { key: k, ...mods });

describe("platform", () => {
  test("macOS: ⌘ shortcuts and Finder, as written", async () => {
    const p = await as(MAC);
    expect(p.windows).toBe(false);
    expect(p.keys("⌥⌘Q")).toBe("⌥⌘Q");
    expect(p.say("Show Log in Finder (⌘L)")).toBe("Show Log in Finder (⌘L)");
    expect(p.fileManager).toBe("Finder");
    expect(p.mod(key("n", { metaKey: true }))).toBe(true);
    expect(p.mod(key("n", { ctrlKey: true }))).toBe(false);
  });

  test("Windows: Ctrl and Alt for ⌘ and ⌥, File Explorer for Finder", async () => {
    const p = await as(WIN);
    expect(p.windows).toBe(true);
    expect(document.documentElement.classList.contains("windows")).toBe(true);
    expect(p.keys("⌘N")).toBe("Ctrl+N");
    expect(p.keys("⌘1 … ⌘7")).toBe("Ctrl+1 … Ctrl+7");
    expect(p.keys("⌥⌘Q")).toBe("Ctrl+Alt+Q");
    expect(p.keys("⌘-click to open")).toBe("Ctrl+click to open");
    expect(p.say("Show Data in Finder")).toBe("Show Data in File Explorer");
    expect(p.mod(key("n", { ctrlKey: true }))).toBe(true);
    expect(p.mod(key("n", { metaKey: true }))).toBe(false);
  });

  test("Linux: Ctrl, not Super (the window manager's), and the file manager", async () => {
    const p = await as(LINUX);
    expect(p.windows).toBe(false);
    expect(p.linux).toBe(true);
    expect(document.documentElement.classList.contains("linux")).toBe(true);
    expect(p.keys("⌘G")).toBe("Ctrl+G");
    expect(p.say("Reaches this Mac by itself")).toBe("Reaches this computer by itself");
    expect(p.say("Show Data in Finder")).toBe("Show Data in File Manager");
    expect(p.mod(key("n", { ctrlKey: true }))).toBe(true);
    expect(p.mod(key("n", { metaKey: true }))).toBe(false);
  });

  test("Windows: Settings › Shortcuts and Show in Finder read the Windows way", async () => {
    const html = readFileSync(resolve(import.meta.dirname, "../settings.html"), "utf8");
    document.body.innerHTML = /<body[^>]*>([\s\S]*)<\/body>/.exec(html)![1].replace(/<script[\s\S]*?<\/script>/g, "");
    const p = await as(WIN);
    p.sayInPage();
    const kbd = [...document.querySelectorAll("#shortcuts kbd")].map((k) => k.textContent);
    expect(kbd).toContain("Ctrl+N");
    expect(kbd).toContain("Ctrl+1 … Ctrl+9");
    expect(kbd).toContain("Ctrl+,");
    expect(kbd.join(" ")).not.toMatch(/[⌘⌥]/);
    expect(document.body.textContent).not.toMatch(/[⌘⌥]|Finder/);
    expect(document.querySelector("#open-config")!.textContent).toBe("Show in File Explorer");
    // Windows has no app menu, so no quit keys: that group is macOS's only.
    const quit = [...document.querySelectorAll("#shortcuts h2")].find((h) => h.textContent === "Quitting")!;
    expect(quit.classList.contains("mac-only")).toBe(true);
    expect(quit.nextElementSibling!.classList.contains("mac-only")).toBe(true);
  });

  test("Windows: the main window's markup and templates, tooltips included", async () => {
    const html = readFileSync(resolve(import.meta.dirname, "../index.html"), "utf8");
    document.body.innerHTML = /<body[^>]*>([\s\S]*)<\/body>/.exec(html)![1].replace(/<script[\s\S]*?<\/script>/g, "");
    const p = await as(WIN);
    p.sayInPage();
    expect(document.querySelector("#reload")!.getAttribute("title")).toBe("Reload applications (Ctrl+R)");
    expect(document.querySelector("#new-reveal")!.textContent).toBe("Show Log in File Explorer");
    const tpl = (document.querySelector("#site-tpl") as HTMLTemplateElement).content;
    expect(tpl.querySelector('.path-open[data-open="finder"]')!.getAttribute("aria-label")).toBe("Show the folder in File Explorer");
    const all = [...document.querySelectorAll("[title], [aria-label]"), ...tpl.querySelectorAll("[title], [aria-label]")]
      .flatMap((e) => [e.getAttribute("title"), e.getAttribute("aria-label")])
      .join(" ");
    expect(all).not.toMatch(/[⌘⌥]|Finder/);
  });
});
