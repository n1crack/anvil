// The main window on Windows: shortcuts are held with Ctrl, and editing keys stay editing keys.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, describe, expect, test } from "vite-plus/test";

const w = window as any;
const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));
const $ = (sel: string) => document.querySelector(sel) as HTMLElement;
const press = (target: EventTarget, key: string, mods: KeyboardEventInit) => {
  const e = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...mods });
  target.dispatchEvent(e);
  return e;
};

w.Option ??= function (text = "", value?: string) {
  const o = document.createElement("option");
  o.textContent = text;
  if (value !== undefined) o.value = value;
  return o;
};

beforeAll(async () => {
  Object.defineProperty(navigator, "userAgent", { value: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/141.0", configurable: true });
  localStorage.clear();
  localStorage.setItem("selected", "shop");
  localStorage.setItem("mode", "apps");
  const html = readFileSync(resolve(import.meta.dirname, "../index.html"), "utf8");
  document.documentElement.innerHTML = html.replace(/<!doctype html>/i, "").replace(/<script[\s\S]*?<\/script>|<link[^>]*>/g, "");
  await import("./mock.js");
  await import("../src/main");
  await wait(150);
});

describe("Windows main window", () => {
  test("Ctrl runs the shortcuts, ⌘ (the Windows key) doesn't", async () => {
    expect(press(document.body, "f", { metaKey: true }).defaultPrevented).toBe(false);
    expect(press(document.body, "f", { ctrlKey: true }).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe($("#search"));
    press(document.body, "4", { ctrlKey: true });
    await wait(60);
    expect($("#site .tabs .on, #site .tabs [aria-selected='true']")?.dataset.tab).toBe("logs");
  });

  test("in a text field, Ctrl+A/C/V/X/Z are left to editing", () => {
    const search = $("#search");
    search.focus();
    for (const k of ["a", "c", "v", "x", "z"]) expect(press(search, k, { ctrlKey: true }).defaultPrevented).toBe(false);
    // Ctrl+E (open in editor) belongs to the field while typing, as ⌘E does on macOS.
    expect(press(search, "e", { ctrlKey: true }).defaultPrevented).toBe(false);
  });

  test("tooltips and labels say Ctrl and File Explorer", () => {
    expect($("#open-settings").title).toBe("Settings (Ctrl+,)");
    expect($('#site .path-open[data-open="finder"]').title).toBe("Show in File Explorer");
    expect($('#site .open-with [data-open="finder"]').title).toBe("Open in File Explorer");
  });
});
