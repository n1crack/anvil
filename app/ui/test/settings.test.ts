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
    expect(text(".nav button")).toEqual([
      "General",
      "PHP",
      "Node.js",
      "Go",
      "Rust",
      ".NET",
      "Python",
      "Worktrees",
      "Storage",
      "Shortcuts",
      "About",
    ]);
    expect($$(".nav button").every((b) => b.querySelector("svg"))).toBe(true);
    expect($("[data-page='languages']")).toBeNull();
    // Each language its own glyph, drawn in currentColor by the stylesheet (no fill of its own).
    const glyphs = ["php", "node", "go", "rust", "dotnet", "python"].map((id) => $(`[data-page="${id}"] svg.glyph path`));
    expect(new Set(glyphs.map((p) => p.getAttribute("d"))).size).toBe(6);
    expect(glyphs.every((p) => !p.getAttribute("fill"))).toBe(true);
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

  test("PHP: a version apps are pinned to names them and offers Make default only", async () => {
    w.mockPhpPins = { "8.5": ["shop"] };
    w.refresh();
    try {
      await open("php");
      const row = $$("#versions .ver")[0];
      expect(row.querySelector(".ver-pinned")?.textContent).toBe("Pinned by shop");
      expect([...row.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Make default"]);
    } finally {
      delete w.mockPhpPins;
      w.refresh();
      await open("php");
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
    // Apps are pinned to 22: named, and no Uninstall until they are unpinned.
    expect(rows[2].querySelector(".ver-pinned")?.textContent).toBe("Pinned by blog, shop");
    expect(rows[2].querySelectorAll("button").length).toBe(0);
    expect($("#node .note").textContent).toContain(".nvmrc");
  });

  test("Rust shows its toolchain", async () => {
    await open("rust");
    expect(text("#rust .ver-label")).toEqual(["Rust stable"]);
  });
});

describe("Node.js package managers", () => {
  test("what apps find, and the terminal's only when it differs; paths in the tooltip", async () => {
    await open("node");
    const rows = $$("#pms .pm");
    expect(rows.map((r) => r.querySelector(".pm-name")?.textContent)).toEqual(["npm", "pnpm", "yarn", "bun"]);
    expect(rows[0].querySelector(".pm-from")?.textContent).toBe("Apps: Bundled with Node.js 24 11.6.2");
    expect(rows[0].querySelector(".pm-term")?.textContent).toBe("Terminal: Homebrew (Node.js 26) 11.12.1");
    expect((rows[0].querySelector(".pm-term .pm-src") as HTMLElement).title).toBe("/opt/homebrew/bin/npm");
    expect(rows[1].querySelector(".pm-from")?.textContent).toBe("Apps: Homebrew 12.14.0");
    expect(rows[1].querySelector(".pm-term")).toBeNull();
    expect(rows[2].classList.contains("missing")).toBe(true);
    expect(rows[2].querySelector(".pm-from")?.textContent).toBe("Apps: Not found");
    expect([...rows[2].querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Install"]);
    expect((rows[3].querySelector(".pm-from .pm-src") as HTMLElement).title).toBe("~/.anvil/runtimes/bun/bin/bun");
    expect(rows[3].querySelector(".on-hover button")?.textContent).toBe("Uninstall");
    expect(rows[0].querySelectorAll("button").length).toBe(0);
    expect(rows[1].querySelectorAll("button").length).toBe(0);
  });

  test("Install shows progress in the row, then an error inline", async () => {
    w.mockDelay.package_manager_install = 80;
    w.mockPmError = "npm error network request to https://registry.npmjs.org/yarn failed";
    $$("#pms .pm")[2].querySelector("button")!.click();
    await wait(10);
    let row = $$("#pms .pm")[2];
    expect(row.querySelector(".spin-dot")).not.toBeNull();
    expect(row.textContent).toContain("Installing…");
    expect(w.mockCalls.at(-1)).toEqual(["package_manager_install", { name: "yarn" }]);
    await wait(150);
    row = $$("#pms .pm")[2];
    expect(row.querySelector(".spin-dot")).toBeNull();
    expect(row.querySelector(".pm-error")?.textContent).toContain("registry.npmjs.org");
    expect(row.querySelector("button")?.textContent).toBe("Install");
    w.mockPmError = null;
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

describe("about", () => {
  test("Check for updates answers inline, with Restart to update only for a newer release", async () => {
    await open("about");
    $("#update-check").click();
    await wait();
    expect($("#update-result").textContent).toBe("Anvil is up to date");
    expect($("#update-go").hidden).toBe(true);

    w.mockUpdate = { version: "0.2.0", notes: "Faster starts." };
    $("#update-check").click();
    await wait();
    expect($("#update-result").textContent).toBe("Version 0.2.0 is available");
    expect($("#update-go").hidden).toBe(false);
    expect($("#update-notes").textContent).toBe("Faster starts.");
  });
});

describe("worktrees", () => {
  test("Copy the database too shows the stored default and stores a new one", async () => {
    await open("worktrees");
    const box = $("#worktree-db") as HTMLInputElement;
    expect(box.checked).toBe(true);
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await wait();
    expect(w.mockCalls.at(-1)).toEqual(["set_worktree_db", { choice: "empty" }]);
  });

  test("each preset is a row with Edit; an edited one opens its fields and offers Reset to default", async () => {
    await open("worktrees");
    const rows = () => $$("#presets > .row:not(.preset-field)");
    expect(rows().map((r) => r.querySelector("span")!.firstChild!.textContent)).toEqual(["Laravel", "Other"]);
    expect(rows()[0].querySelector("small")!.textContent).toBe("Laravel · edited");
    expect(rows()[1].querySelector("small")!.textContent).toBe("Any other type");
    expect($$("#presets textarea").length).toBe(0);
    (rows()[0].querySelector("button") as HTMLButtonElement).click();
    await wait();
    const areas = $$("#presets textarea") as HTMLTextAreaElement[];
    expect(areas.map((a) => a.value)).toEqual([".env", "APP_URL={url}", "", "composer install"]);
    // The database switch is Settings' own, above: not a preset field.
    expect($$("#presets input[type=checkbox]").length).toBe(0);
    areas[3].value = "composer install\nphp artisan migrate";
    areas[3].dispatchEvent(new Event("change"));
    await wait();
    expect(w.mockCalls.at(-1)).toEqual([
      "set_worktree_preset",
      { id: "laravel", field: "after_create", value: ["composer install", "php artisan migrate"] },
    ]);
    ($$("#presets button").find((b) => b.textContent === "Reset to default") as HTMLButtonElement).click();
    await wait();
    expect(w.mockCalls.at(-1)).toEqual(["set_worktree_preset", { id: "laravel", field: null, value: null }]);
  });
});
