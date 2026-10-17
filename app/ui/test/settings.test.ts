// The Settings window driven through its DOM, with a mocked backend (test/mock.js).
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, describe, expect, test } from "vite-plus/test";

const w = window as any;
const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));
const $ = (sel: string) => document.querySelector(sel) as HTMLElement;
const $$ = (sel: string) => [...document.querySelectorAll(sel)] as HTMLElement[];
const text = (sel: string) => $$(sel).map((e) => e.textContent?.trim());

async function open(page: string) {
  $(`[data-page="${page}"]`).click();
  await wait(60);
}

// happy-dom has no `new Option(text, value)`.
w.Option ??= function (text = "", value?: string) {
  const o = document.createElement("option");
  o.textContent = text;
  if (value !== undefined) o.value = value;
  return o;
};

beforeAll(async () => {
  localStorage.clear();
  localStorage.setItem("storageTab", "apps");
  const html = readFileSync(resolve(import.meta.dirname, "../settings.html"), "utf8");
  document.documentElement.innerHTML = html.replace(/<!doctype html>/i, "").replace(/<script[\s\S]*?<\/script>|<link[^>]*>/g, "");
  await import("./mock.js");
  w.mockDelay = { storage_last: 200 };
  await import("../src/settings");
  await wait(60);
});

describe("sidebar", () => {
  test("a Languages group lists one page per language, each with an icon", () => {
    const head = $$(".nav-head")[0];
    expect(head.textContent).toBe("Languages");
    expect(head.tagName).not.toBe("BUTTON");
    expect(text(".nav button")).toEqual(["General", "PHP", "Node.js", "Go", "Rust", ".NET", "Python", "Storage", "Shortcuts", "About"]);
    expect($$(".nav button").every((b) => b.querySelector("svg"))).toBe(true);
    expect($("[data-page='languages']")).toBeNull();
    for (const id of ["php", "node", "go", "rust", "dotnet", "python"]) expect($(`section#${id}`)).not.toBeNull();
  });
});

describe("versions", () => {
  test("PHP: the default is marked, missing versions are muted with Install only", async () => {
    await open("php");
    const rows = $$("#versions .ver");
    expect(rows.map((r) => r.querySelector(".ver-label")?.textContent)).toEqual([
      "PHP 8.5",
      "PHP 8.4",
      "PHP 8.3",
      "PHP 8.2",
      "PHP 8.1",
      "PHP 8.0",
    ]);
    const def = rows[1];
    expect(def.classList.contains("is-default")).toBe(true);
    expect(def.querySelector(".ver-mark svg")).not.toBeNull();
    expect(def.querySelector(".ver-default")?.textContent).toBe("Default");
    expect(def.querySelectorAll("button").length).toBe(0);
    expect([...rows[0].querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Make default", "Uninstall"]);
    for (const r of rows.slice(2)) {
      expect(r.classList.contains("missing")).toBe(true);
      expect([...r.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Install"]);
    }
  });

  test("installing shows progress in the row and holds the other Install buttons", async () => {
    w.mockDelay.php_install = 100;
    $$("#versions .ver")[2].querySelector("button")!.click();
    await wait(10);
    const rows = $$("#versions .ver");
    expect(rows[2].querySelector(".spin-dot")).not.toBeNull();
    expect(rows[2].textContent).toContain("Installing…");
    expect((rows[3].querySelector("button") as HTMLButtonElement).disabled).toBe(true);
    await wait(150);
  });

  test("Node.js: every version the backend offers, installed ones without a made-up default", async () => {
    await open("node");
    const rows = $$("#node .ver");
    expect(rows.map((r) => r.querySelector(".ver-label")?.textContent)).toEqual(["Node.js 26", "Node.js 24", "Node.js 22", "Node.js 20"]);
    expect(rows.map((r) => r.classList.contains("missing"))).toEqual([true, false, false, true]);
    expect($$("#node .is-default").length).toBe(0);
    expect([...rows[1].querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Uninstall"]);
    expect($("#node .note").textContent).toContain(".nvmrc");
  });

  test("Rust shows its toolchain", async () => {
    await open("rust");
    expect(text("#rust .ver-label")).toEqual(["Rust stable"]);
  });
});

describe("Node.js package managers", () => {
  test("each one with where it comes from and its version, or Not found", async () => {
    await open("node");
    const rows = $$("#pms .pm");
    expect(rows.map((r) => r.querySelector(".pm-name")?.textContent)).toEqual(["npm", "pnpm", "yarn", "bun"]);
    expect(rows[0].querySelector(".pm-from")?.textContent).toBe("Bundled with Node.js 24");
    expect(rows[0].querySelector(".pm-version")?.textContent).toBe("11.6.2");
    expect(rows[1].querySelector(".pm-from")?.textContent).toBe("Homebrew");
    expect(rows[2].classList.contains("missing")).toBe(true);
    expect(rows[2].querySelector(".pm-from")?.textContent).toBe("Not found");
    expect(rows[3].querySelector(".pm-from")?.textContent).toContain("~/.bun/bin/bun");
  });
});

describe("storage", () => {
  test("the toolbar is there, disabled, before the numbers arrive; then it fills in place", async () => {
    w.mockStorageLast = [
      Math.floor(Date.now() / 1000) - 120,
      [
        { name: "shop", kind: "laravel", total: 524288000, safe: 104857600, rebuild: 314572800 },
        { name: "notes", kind: "folder", total: 1048576, safe: 0, rebuild: 0 },
      ],
    ];
    $(`[data-page="storage"]`).click();
    await wait(20);
    const scan = $("#scan-apps") as HTMLButtonElement;
    const del = $("#clean-all-safe") as HTMLButtonElement;
    const bar = $(".storage-bar");
    expect(bar.compareDocumentPosition($("#app-sizes")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(scan.disabled).toBe(true);
    expect(scan.classList.contains("spinning")).toBe(true);
    expect($("#measured").textContent).toBe("Measuring…");
    expect(del.disabled).toBe(true);
    expect(del.textContent).toBe("Delete Logs and Caches in All Apps");
    expect($$("#app-sizes .placeholder").length).toBe(6);
    await wait(250);
    expect(scan.disabled).toBe(false);
    expect($("#measured").textContent).toBe("Measured 2 min ago");
    expect(del.disabled).toBe(false);
    expect(del.textContent).toBe("Delete Logs and Caches in All Apps (100 MB)");
    expect($$("#app-sizes .placeholder").length).toBe(0);
    expect($$("#app-sizes .row").length).toBe(2);
  });
});
