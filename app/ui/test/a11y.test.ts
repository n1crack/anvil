// Keyboard and screen-reader basics of the main window, with a mocked backend (test/mock.js).
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, describe, expect, test } from "vite-plus/test";

const w = window as any;
const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));
const $ = (sel: string) => document.querySelector(sel) as HTMLElement;
const $$ = (sel: string) => [...document.querySelectorAll(sel)] as HTMLElement[];
const key = (at: Element, k: string, shiftKey = false) =>
  at.dispatchEvent(new KeyboardEvent("keydown", { key: k, shiftKey, bubbles: true, cancelable: true }));

// happy-dom has no `new Option(text, value)`.
w.Option ??= function (text = "", value?: string) {
  const o = document.createElement("option");
  o.textContent = text;
  if (value !== undefined) o.value = value;
  return o;
};

beforeAll(async () => {
  localStorage.clear();
  localStorage.setItem("selected", "vuefinder");
  localStorage.setItem("mode", "apps");
  localStorage.setItem("tab", "overview");
  const html = readFileSync(resolve(import.meta.dirname, "../index.html"), "utf8");
  document.documentElement.innerHTML = html.replace(/<!doctype html>/i, "").replace(/<script[\s\S]*?<\/script>|<link[^>]*>/g, "");
  await import("./mock.js");
  await import("../src/main");
  await wait(150);
});

describe("keyboard", () => {
  test("a bubble keeps Tab inside, and Esc closes it and gives the focus back", async () => {
    const { popAt } = await import("../src/app");
    const anchor = Object.assign(document.createElement("button"), { textContent: "Remove" });
    document.body.append(anchor);
    const [no, yes] = ["Cancel", "Remove"].map((t) => Object.assign(document.createElement("button"), { textContent: t }));
    const pop = document.createElement("div");
    pop.append(no, yes);
    popAt(anchor, pop, no);
    expect(pop.getAttribute("aria-label")).toBe("Remove");
    expect(document.activeElement).toBe(no);
    yes.focus();
    key(yes, "Tab");
    expect(document.activeElement).toBe(no);
    key(no, "Tab", true);
    expect(document.activeElement).toBe(yes);
    key(yes, "Escape");
    expect(pop.isConnected).toBe(false);
    expect(document.activeElement).toBe(anchor);
    anchor.remove();
  });

  test("Esc in the New Application sheet's empty search closes the sheet", async () => {
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "n", metaKey: true, bubbles: true }));
    await wait(60);
    const sheet = $("#new-app") as HTMLDialogElement;
    expect(sheet.open).toBe(true);
    const search = $("#new-search") as HTMLInputElement;
    search.value = "lar";
    key(search, "Escape");
    expect([search.value, sheet.open]).toEqual(["", true]);
    key(search, "Escape");
    expect(sheet.open).toBe(false);
  });
});

describe("state in words", () => {
  test("the Dev card's dots and the sidebar's badges say their state, not only their colour", async () => {
    $('#list [data-name="vuefinder"]').click();
    await wait(80);
    const dots = $$(".procs .dev-card .dot");
    expect(dots.length).toBeGreaterThan(0);
    for (const d of dots) {
      expect(d.getAttribute("role")).toBe("img");
      expect(d.getAttribute("aria-label")).toMatch(/: (running|starting|crashed|stopped)$/);
    }
    $('[data-mode="services"]').click();
    await wait(80);
    const badges = $$("#list .run-badge");
    expect(badges.length).toBeGreaterThan(0);
    // A server elsewhere isn't run: its badge says it answers.
    for (const b of badges)
      expect([b.getAttribute("role"), b.getAttribute("aria-label")]).toEqual([
        "img",
        b.closest('[data-name="shared-db"]') ? "Reachable" : "Running",
      ]);
    $('[data-mode="apps"]').click();
    await wait(80);
  });
});
