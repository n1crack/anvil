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
    // Windows-only pages (Performance) are in the markup but hidden on macOS and Linux.
    expect($$(".nav button.windows-only").map((b) => b.dataset.page)).toEqual(["performance"]);
    // The app's pages, then Languages, then Features (a page per feature that is on), About at the foot.
    expect(text(".nav button:not(.windows-only):not([hidden])")).toEqual([
      "General",
      "Worktrees",
      "Storage",
      "Shortcuts",
      "PHP",
      "Node.js",
      "Go",
      "Rust",
      ".NET",
      "Python",
      "Features",
      "About",
    ]);
    expect(text(".nav-head")).toEqual(["Languages", ""]); // the empty one only spaces Features apart
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
  // A row's More menu: its items' names, or one of them chosen.
  const menu = async (row: Element) => {
    w.mockMenu = null;
    (row.querySelector(".ver-more") as HTMLElement | null)?.click();
    await wait(10);
    return ((w.mockMenu as any[] | null) ?? []).map((i) => i.text);
  };
  const choose = async (row: Element, text: string) => {
    await menu(row);
    (w.mockMenu as any[]).find((i) => i.text === text).action();
    await wait(10);
  };

  test("PHP: the default is marked, missing versions are muted with Install only", async () => {
    await open("php");
    const rows = $$("#versions .ver");
    expect(rows.map((r) => r.querySelector(".ver-label")?.textContent)).toEqual([
      "PHP 8.5.8",
      "PHP 8.4.23",
      "PHP 8.3",
      "PHP 8.2",
      "PHP 8.1",
    ]);
    const def = rows[1];
    expect(def.classList.contains("is-default")).toBe(true);
    expect(def.querySelector(".ver-default")?.textContent).toBe("Default");
    expect(def.querySelector("button.ver-more")).toBeNull();
    // Make default and Uninstall wait in the More menu, not on hover.
    expect(rows[0].querySelectorAll(".actions-inline button").length).toBe(0);
    expect(await menu(rows[0])).toEqual(["Make default", "Uninstall…"]);
    // Uninstall asks in the row, no dialog; Cancel empties it again.
    await choose(rows[0], "Uninstall…");
    expect(rows[0].querySelector(".actions-inline")!.textContent).toBe(
      "Uninstall? Applications asking for it use the default.CancelUninstall",
    );
    (rows[0].querySelectorAll(".actions-inline button")[0] as HTMLElement).click();
    expect(rows[0].querySelector(".actions-inline")!.textContent).toBe("");
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
      expect(await menu(row)).toEqual(["Make default"]);
    } finally {
      delete w.mockPhpPins;
      w.refresh();
      await open("php");
    }
  });

  test("PHP: a release candidate is tagged Preview", async () => {
    w.mockPhpPreview = ["8.5"];
    w.refresh();
    try {
      await open("php");
      const rows = $$("#versions .ver");
      expect(rows[0].querySelector(".ver-pinned")?.textContent).toBe("Preview");
      expect(rows[1].querySelector(".ver-pinned")).toBeNull();
    } finally {
      delete w.mockPhpPreview;
      w.refresh();
      await open("php");
    }
  });

  test("installing shows progress in the row and holds the other Install buttons", async () => {
    w.mockDelay.php_install = 100;
    ($$("#versions .ver")[2].querySelector(".actions-inline button") as HTMLElement).click();
    await wait(10);
    const rows = $$("#versions .ver");
    expect(rows[2].querySelector(".spin-dot")).not.toBeNull();
    expect(rows[2].textContent).toContain("Installing…");
    expect((rows[3].querySelector(".actions-inline button") as HTMLButtonElement).disabled).toBe(true);
    await wait(150);
  });

  const phpRow = (v: string) =>
    $$("#versions .ver").find((r) => r.querySelector(".ver-label")?.textContent?.match(/^PHP (\d+\.\d+)/)?.[1] === v)!;
  const buttons = (r: HTMLElement) => [...r.querySelectorAll("button")].map((b) => b.textContent);

  test("a newer patch shows before the extension count as Update to it, which runs php_update", async () => {
    w.mockPhpAvailable = { "8.4": ["8.4.23", "8.4.27"] };
    w.refresh();
    try {
      await open("php");
      const row = phpRow("8.4");
      expect(buttons(row)[0]).toBe("Update to 8.4.27");
      expect(row.querySelector(".ver-patch + .ext-toggle")!.textContent).toBe("Extensions 1/9›");
      (row.querySelector(".ver-patch button") as HTMLElement).click();
      await wait(10);
      expect(w.mockCalls).toContainEqual(["php_update", { version: "8.4" }]);
    } finally {
      delete w.mockPhpAvailable;
      w.refresh();
      await wait(60);
    }
  });
  test("Composer global packages: Update to the newer one, Install the Forge CLI, a failure in its row", async () => {
    await open("php");
    const rows = () => $$("#composer-globals .ver");
    expect(rows().map((r) => r.querySelector(".ver-label")?.textContent)).toEqual(["laravel/installer 5.11.2", "laravel/forge-cli"]);
    expect(rows().map(buttons)).toEqual([["Update to 5.12.0", "Remove"], ["Install"]]);
    expect(rows()[1].classList.contains("missing")).toBe(true);
    w.mockDelay.composer_global_require = 60;
    (rows()[0].querySelector("button") as HTMLElement).click();
    await wait(10);
    expect(rows()[0].querySelector(".ver-patch")!.textContent).toBe("Updating to 5.12.0…");
    await wait(90);
    expect(w.mockCalls).toContainEqual(["composer_global_require", { name: "laravel/installer", yours: false }]);
    expect(rows()[0].querySelector(".ver-label")!.textContent).toBe("laravel/installer 5.12.0");
    expect(buttons(rows()[0])).toEqual(["Remove"]);
    w.mockForgeFails = true;
    try {
      (rows()[1].querySelector("button") as HTMLElement).click();
      await wait(90);
      expect(rows()[1].querySelector(".ver-error")!.textContent).toContain("could not be resolved");
      delete w.mockForgeFails;
      (rows()[1].querySelector("button") as HTMLElement).click();
      await wait(90);
      expect(rows()[1].querySelector(".ver-label")!.textContent).toBe("laravel/forge-cli 1.9.0");
      expect(rows()[1].querySelector(".ver-error")).toBeNull();
    } finally {
      delete w.mockForgeFails;
      delete w.mockDelay.composer_global_require;
    }
  });

  test("Composer global packages: the user's own copy first, tagged, never removable; Anvil's copy asks, then goes", async () => {
    const before = w.mockGlobals;
    w.mockGlobals = [
      { name: "laravel/forge-cli", version: "2.0.1", latest: null, yours: true },
      { name: "laravel/forge-cli", version: "2.0.4", latest: null, yours: false },
      { name: "laravel/installer", version: null, latest: null, yours: false },
    ];
    w.refresh();
    try {
      await open("php");
      const rows = () => $$("#composer-globals .ver");
      expect(rows().map((r) => r.querySelector(".ver-name")?.textContent)).toEqual([
        "laravel/forge-cli 2.0.1Your Composer",
        "laravel/forge-cli 2.0.4",
        "laravel/installer",
      ]);
      expect(rows().map(buttons)).toEqual([[], ["Remove"], ["Install"]]);
      (rows()[1].querySelector("button") as HTMLElement).click();
      expect(rows()[1].textContent).toContain("Remove Anvil’s laravel/forge-cli?");
      ([...rows()[1].querySelectorAll("button")].find((b) => b.textContent === "Remove") as HTMLElement).click();
      await wait(30);
      expect(w.mockCalls).toContainEqual(["composer_global_remove", { name: "laravel/forge-cli" }]);
      expect(rows().map((r) => r.querySelector(".ver-label")?.textContent)).toEqual(["laravel/forge-cli 2.0.1", "laravel/installer"]);
    } finally {
      w.mockGlobals = before;
      w.refresh();
      await wait(60);
    }
  });

  async function withOpcache83(run: () => Promise<void>) {
    w.mockPhpInstalled = ["8.5", "8.4", "8.3"];
    w.mockOpcache = {};
    w.refresh();
    try {
      await open("php");
      await run();
    } finally {
      delete w.mockPhpInstalled;
      delete w.mockOpcache;
      delete w.mockOpcacheError;
      delete w.mockDelay.php_opcache;
      w.refresh();
      await open("php");
    }
  }

  const toggle = (v: string) => phpRow(v).querySelector(".ext-toggle") as HTMLElement | null;
  async function openExts(v: string) {
    // A click anywhere on the row's own space opens it; the count is part of the row.
    if (phpRow(v).getAttribute("aria-expanded") !== "true") toggle(v)!.click();
    await wait(10);
  }
  const lines = (v: string) => [...phpRow(v).querySelectorAll<HTMLElement>(".ext-fold.open .ext-line")];
  const line = (v: string, name: string) => lines(v).find((l) => l.querySelector(".ext-name")?.textContent === name)!;
  const exts = (v: string) =>
    lines(v)
      .slice(1)
      .map((l) => ({
        name: l.querySelector(".ext-name")?.textContent,
        on: (l.querySelector("input[role=switch]") as HTMLInputElement).checked,
        text: l.textContent,
      }));
  const extSwitch = (v: string, name: string) => line(v, name).querySelector("input") as HTMLInputElement;

  test("row_shows_opcache_state: Extensions n/9 after an installed version opens its list, OPcache first", () =>
    withOpcache83(async () => {
      expect(toggle("8.5")!.textContent).toBe("Extensions 1/9›");
      expect(toggle("8.3")!.textContent).toBe("Extensions 0/9›");
      // Not installed: nothing about extensions.
      expect(toggle("8.2")).toBeNull();
      expect(lines("8.5").length).toBe(0);
      await openExts("8.5");
      expect(lines("8.5").map((l) => l.querySelector(".ext-name")!.textContent)).toEqual([
        "OPcache",
        "xdebug",
        "pcov",
        "apcu",
        "igbinary",
        "msgpack",
        "mongodb",
        "memcached",
        "imagick",
      ]);
      expect(line("8.5", "OPcache").textContent).toBe("OPcacheBuilt in");
      // The list slides open in place: no redraw.
      expect(phpRow("8.5").querySelector(".ext-fold")!.classList.contains("open")).toBe(true);
      // Inside the list, a click is the list's: it stays open.
      (phpRow("8.5").querySelector(".ext-list") as HTMLElement).click();
      expect(phpRow("8.5").getAttribute("aria-expanded")).toBe("true");
      // The version's name closes it again; Enter opens it from the keyboard.
      (phpRow("8.5").querySelector(".ver-label") as HTMLElement).click();
      expect(phpRow("8.5").getAttribute("aria-expanded")).toBe("false");
      phpRow("8.5").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      expect(phpRow("8.5").getAttribute("aria-expanded")).toBe("true");
      // The More menu is its own: it doesn't fold the row.
      (phpRow("8.3").querySelector("button.ver-more") as HTMLElement).click();
      await wait(10);
      expect(phpRow("8.3").getAttribute("aria-expanded")).toBe("false");
      await openExts("8.4");
      expect(line("8.4", "OPcache").textContent).toBe("OPcacheRemove");
      await openExts("8.3");
      expect(line("8.3", "OPcache").textContent).toBe("OPcacheNot builtBuild");
      expect(line("8.3", "OPcache").querySelector("input")).toBeNull();
    }));

  test("OPcache has a switch, on unless turned off; off leaves it out of the count", () =>
    withOpcache83(async () => {
      await openExts("8.5");
      const sw = () => line("8.5", "OPcache").querySelector("input[role=switch]") as HTMLInputElement;
      expect(sw().checked).toBe(true);
      sw().checked = false;
      sw().dispatchEvent(new Event("change"));
      await wait(30);
      expect(w.mockCalls.filter(([c]: [string]) => c === "php_opcache_on").at(-1)[1]).toEqual({ version: "8.5", on: false });
      expect(sw().checked).toBe(false);
      expect(toggle("8.5")!.textContent).toMatch(/^Extensions 0\/9/);
      sw().checked = true;
      sw().dispatchEvent(new Event("change"));
      await wait(30);
      expect(sw().checked).toBe(true);
    }));

  test("a window opened during a build says Building… on OPcache too", () =>
    withOpcache83(async () => {
      w.mockOpcacheBuilding = ["8.3"];
      try {
        w.refresh();
        await open("php");
        expect(toggle("8.3")!.querySelector(".spin-dot")).not.toBeNull();
        await openExts("8.3");
        expect(line("8.3", "OPcache").textContent).toBe("OPcacheBuilding… a few minutes");
      } finally {
        delete w.mockOpcacheBuilding;
      }
    }));

  test("add_opcache_is_inline: Build asks in the row, then Building… until it resolves, then On", () =>
    withOpcache83(async () => {
      w.mockDelay.php_opcache = 100;
      await openExts("8.3");
      (line("8.3", "OPcache").querySelector(".text-act") as HTMLElement).click();
      expect(w.mockCalls.at(-1)?.[0]).not.toBe("php_opcache");
      expect(line("8.3", "OPcache").textContent).toBe("OPcacheBuild OPcache for PHP 8.3? It takes a few minutes.CancelBuild");
      (line("8.3", "OPcache").querySelector(".text-act.danger") as HTMLElement).click();
      await wait(10);
      expect(w.mockCalls.at(-1)).toEqual(["php_opcache", { version: "8.3" }]);
      expect(line("8.3", "OPcache").textContent).toBe("OPcacheBuilding… a few minutes");
      await wait(150);
      expect(line("8.3", "OPcache").textContent).toBe("OPcacheRemove");
      expect(toggle("8.3")!.textContent).toBe("Extensions 1/9›");
      // Remove asks in the row too, then it's Not built again.
      (line("8.3", "OPcache").querySelector(".text-act") as HTMLElement).click();
      expect(line("8.3", "OPcache").textContent).toBe("OPcacheRemove OPcache from PHP 8.3? Building it again takes minutes.CancelRemove");
      (line("8.3", "OPcache").querySelector(".text-act.danger") as HTMLElement).click();
      await wait(30);
      expect(w.mockCalls.at(-1)).toEqual(["php_opcache_remove", { version: "8.3" }]);
      expect(line("8.3", "OPcache").textContent).toBe("OPcacheNot builtBuild");
      expect($$('[role="alert"]:not([hidden]), .toast').length).toBe(0);
    }));

  test("row_offers_the_tools: a build without the command line tools says so on the row and offers them", () =>
    withOpcache83(async () => {
      w.mockOpcacheError = "Building OPcache failed — OPcache needs the Xcode Command Line Tools: xcode-select --install";
      await openExts("8.3");
      (line("8.3", "OPcache").querySelector(".text-act") as HTMLElement).click();
      (line("8.3", "OPcache").querySelector(".text-act.danger") as HTMLElement).click();
      await wait(30);
      const r = phpRow("8.3");
      expect(r.querySelector(".ver-error")?.textContent).toContain("xcode-select --install");
      expect($$('[role="alert"]:not([hidden])').length).toBe(0);
      const tools = [...r.querySelectorAll("button")].find((b) => b.textContent === "Install command line tools")!;
      expect(tools).toBeDefined();
      tools.click();
      await wait(10);
      expect(w.mockCalls.at(-1)).toEqual(["xcode_tools_install", undefined]);
    }));

  test("extensions_switches_per_version: the catalog as switches in each installed version's list", () =>
    withOpcache83(async () => {
      w.mockExt = { "8.4": ["xdebug"] };
      w.refresh();
      await open("php");
      try {
        expect(toggle("8.4")!.textContent).toBe("Extensions 2/9›");
        await openExts("8.4");
        await openExts("8.3");
        expect(exts("8.4")).toEqual(
          ["xdebug", "pcov", "apcu", "igbinary", "msgpack", "mongodb", "memcached", "imagick"].map((name) => ({
            name,
            on: name === "xdebug",
            text: name === "imagick" ? "imagickNeeds ImageMagick from Homebrew: brew install imagemagick" : name,
          })),
        );
        // Needs Homebrew's ImageMagick: held off; with it, a switch like the others.
        expect(extSwitch("8.4", "imagick").disabled).toBe(true);
        w.mockExtNeeds = {};
        w.refresh();
        await open("php");
        expect(extSwitch("8.4", "imagick").disabled).toBe(false);
        delete w.mockExtNeeds;
        expect(exts("8.3").every((e) => !e.on)).toBe(true);
        // Off: one call, for that version and extension.
        extSwitch("8.4", "xdebug").click();
        await wait(30);
        expect(w.mockCalls.at(-1)).toEqual(["php_ext", { version: "8.4", name: "xdebug", on: false }]);
        expect(exts("8.4").every((e) => !e.on)).toBe(true);

        // On, but no build for the installed PHP: off, Not built; on builds it.
        w.mockExt = { "8.4": ["pcov"] };
        w.mockExtUnbuilt = ["8.4 pcov"];
        w.refresh();
        await open("php");
        expect(exts("8.4").find((e) => e.name === "pcov")).toEqual({ name: "pcov", on: false, text: "pcovNot built" });
        extSwitch("8.4", "pcov").click();
        await wait(30);
        expect(w.mockCalls.at(-1)).toEqual(["php_ext", { version: "8.4", name: "pcov", on: true }]);
      } finally {
        delete w.mockExt;
        delete w.mockExtUnbuilt;
      }
    }));

  test("building_is_inline: Building… on the extension's line until it resolves; an error under the row", () =>
    withOpcache83(async () => {
      w.mockDelay.php_ext = 100;
      try {
        await openExts("8.4");
        extSwitch("8.4", "apcu").click();
        await wait(10);
        expect(w.mockCalls.at(-1)).toEqual(["php_ext", { version: "8.4", name: "apcu", on: true }]);
        const apcu = () => exts("8.4").find((e) => e.name === "apcu")!;
        expect(apcu().text).toBe("apcuBuilding…");
        expect(extSwitch("8.4", "apcu").disabled).toBe(true);
        await wait(150);
        expect(apcu()).toEqual({ name: "apcu", on: true, text: "apcu" });
        expect($$('[role="alert"]:not([hidden]), .toast').length).toBe(0);

        // A window opened while another build runs says so too.
        w.mockExtBuilding = ["8.4 pcov"];
        w.refresh();
        await open("php");
        expect(exts("8.4").find((e) => e.name === "pcov")!.text).toBe("pcovBuilding…");
        expect(extSwitch("8.4", "pcov").disabled).toBe(true);
        delete w.mockExtBuilding;
        w.refresh();
        await open("php");

        w.mockExtError = "Building pcov failed — it needs the Xcode Command Line Tools: xcode-select --install";
        delete w.mockDelay.php_ext;
        extSwitch("8.4", "pcov").click();
        await wait(30);
        const r = phpRow("8.4");
        expect(r.querySelector(".ver-error")?.textContent).toContain("xcode-select --install");
        expect(buttons(r)).toContain("Install command line tools");
        expect(extSwitch("8.4", "pcov").checked).toBe(false);
        expect($$('[role="alert"]:not([hidden])').length).toBe(0);
      } finally {
        delete w.mockExt;
        delete w.mockExtError;
        delete w.mockExtBuilding;
        delete w.mockDelay.php_ext;
      }
    }));

  test("Node.js: every version the backend offers, installed ones without a made-up default", async () => {
    await open("node");
    const rows = $$("#node .ver");
    expect(rows.map((r) => r.querySelector(".ver-label")?.textContent)).toEqual(["Node.js 26", "Node.js 24", "Node.js 22", "Node.js 20"]);
    expect(rows.map((r) => r.classList.contains("missing"))).toEqual([true, false, false, true]);
    expect($$("#node .is-default").length).toBe(0);
    expect(await menu(rows[1])).toEqual(["Uninstall…"]);
    // Apps are pinned to 22: named, and no Uninstall until they are unpinned.
    expect(rows[2].querySelector(".ver-pinned")?.textContent).toBe("Pinned by blog, shop");
    expect(rows[2].querySelectorAll("button").length).toBe(0);
    expect($("#node .note").textContent).toContain(".nvmrc");
  });

  test("Node.js on a Mac too old for 24 and later: those rows say why instead of Install", async () => {
    await open("node");
    w.mockOldMac = true;
    try {
      // An install reloads the list, now as an older Mac sees it.
      ($$("#node .ver")[3].querySelector("button") as HTMLElement).click();
      await wait(60);
      const rows = $$("#node .ver");
      expect(rows[0].querySelector(".actions-inline")?.textContent).toBe("Needs macOS 13.5 or later; this Mac has 13.4.");
      expect(rows[1].querySelector(".actions-inline")?.textContent).toBe("Needs macOS 13.5 or later; this Mac has 13.4.");
      expect(rows[0].querySelectorAll("button").length).toBe(0);
      expect([...rows[3].querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Install"]);
    } finally {
      delete w.mockOldMac;
      // Reload the list as this Mac sees it, so later tests find Install where it was.
      ($$("#node .ver")[3].querySelector("button") as HTMLElement).click();
      await wait(60);
    }
  });

  test("Rust shows its toolchain", async () => {
    await open("rust");
    expect(text("#rust .ver-label")).toEqual(["Rust stable"]);
  });

  test("a failed Install says why in its row, not at the top; the next action clears it", async () => {
    await open("node");
    const nodeRow = (i: number) => $$("#node .ver")[i];
    w.mockFail = { runtime_install: "Couldn't download Node.js 26: timed out" };
    try {
      nodeRow(0).querySelector("button")!.click();
      await wait(60);
      expect(nodeRow(0).querySelector(".ver-error")?.textContent).toBe("Couldn't download Node.js 26: timed out");
      expect(nodeRow(3).querySelector(".ver-error")).toBeNull();
      expect($("#error").hidden).toBe(true);
      w.mockFail = {};
      nodeRow(3).querySelector("button")!.click();
      await wait(60);
      expect($$("#node .ver-error").length).toBe(0);
    } finally {
      w.mockFail = {};
    }
  });

  test("PHP: a failed Uninstall says why in its row", async () => {
    await open("php");
    w.mockFail = { php_uninstall: "PHP 8.5 is in use" };
    try {
      const row = () => $$("#versions .ver")[0];
      await choose(row(), "Uninstall…");
      (row().querySelectorAll(".actions-inline button")[1] as HTMLElement).click(); // confirm
      await wait(60);
      expect(row().querySelector(".ver-error")?.textContent).toBe("PHP 8.5 is in use");
      expect($("#error").hidden).toBe(true);
      w.mockFail = {};
      await choose(row(), "Make default");
      await wait(60);
      expect(row().querySelector(".ver-error")).toBeNull();
    } finally {
      w.mockFail = {};
      w.refresh();
      await wait(60);
    }
  });
});

describe("Node.js package managers", () => {
  test("what apps find, and the terminal's only when it differs; paths in the tooltip", async () => {
    await open("node");
    const rows = $$("#pms .pm");
    expect(rows.map((r) => r.querySelector(".pm-name")?.textContent)).toEqual(["npm", "pnpm", "yarn", "bun"]);
    expect(rows[0].querySelector(".pm-from")?.textContent).toBe("Applications: Bundled with Node.js 24 11.6.2");
    expect(rows[0].querySelector(".pm-term")?.textContent).toBe("Terminal: Homebrew (Node.js 26) 11.12.1");
    expect((rows[0].querySelector(".pm-term .pm-src") as HTMLElement).title).toBe("/opt/homebrew/bin/npm");
    expect(rows[1].querySelector(".pm-from")?.textContent).toBe("Applications: Homebrew 12.14.0");
    expect(rows[1].querySelector(".pm-term")).toBeNull();
    expect(rows[2].classList.contains("missing")).toBe(true);
    expect(rows[2].querySelector(".pm-from")?.textContent).toBe("Applications: Not found");
    expect([...rows[2].querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Install"]);
    expect((rows[3].querySelector(".pm-from .pm-src") as HTMLElement).title).toBe("~/.anvil/runtimes/bun/bin/bun");
    // Anvil's own: Uninstall in its More menu, asked in the row.
    w.mockMenu = null;
    (rows[3].querySelector("button.ver-more") as HTMLElement).click();
    await wait(10);
    expect((w.mockMenu as any[]).map((i) => i.text)).toEqual(["Uninstall…"]);
    (w.mockMenu as any[])[0].action();
    expect(rows[3].querySelector(".actions-inline")!.textContent).toBe(
      "Uninstall? Applications find the next one on their PATH.CancelUninstall",
    );
    (rows[3].querySelectorAll(".actions-inline button")[0] as HTMLElement).click();
    expect(rows[3].querySelector("button.ver-more")).not.toBeNull();
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
    expect(del.textContent).toBe("Delete Logs and Caches in All Applications");
    expect($$("#app-sizes .placeholder").length).toBe(6);
    expect($("#apps-hint").textContent).toBe("Measuring every application…");
    await wait(250);
    expect($("#apps-hint").textContent).toMatch(/^2 applications use /);
    expect(scan.disabled).toBe(false);
    expect($("#measured").textContent).toBe("Measured 2 min ago");
    expect(del.disabled).toBe(false);
    expect(del.textContent).toBe("Delete Logs and Caches in All Applications (100 MB)");
    expect($$("#app-sizes .placeholder").length).toBe(0);
    expect($$("#app-sizes .row").length).toBe(2);
  });

  test("measuring shows only that; no applications is said once a measurement found none", async () => {
    w.mockDelay.storage_apps = 100;
    $("#scan-apps").click();
    await wait(20);
    expect($("#apps-hint").textContent).toBe("Measuring every application…");
    expect($("#app-sizes").textContent).not.toContain("No applications");
    await wait(150);
    // The mock's measurement counts no app: the ones from before drop out.
    expect($("#apps-hint").textContent).toBe("");
    expect($("#app-sizes").textContent).toBe("No applications take any space.");
    delete w.mockDelay.storage_apps;
  });

  test("Delete asks in the row, no dialog; every row ends with its size", async () => {
    const rows = $$("#anvil-items .row");
    expect(rows.every((r) => r.querySelector(".actions-inline")?.lastElementChild?.classList.contains("size"))).toBe(true);
    const logs = rows.find((r) => r.textContent?.startsWith("Logs"))!;
    (logs.querySelector(".actions-inline button") as HTMLElement).click();
    expect(logs.querySelector(".asking .muted")?.textContent).toBe("Delete 12 MB?");
    const [no] = logs.querySelectorAll<HTMLElement>(".asking button");
    no.click();
    expect(logs.querySelector(".asking")).toBeNull();
    expect(logs.querySelector(".actions-inline button")?.textContent).toBe("Delete");
    (logs.querySelector(".actions-inline button") as HTMLElement).click();
    logs.querySelectorAll<HTMLElement>(".asking button")[1].click();
    await wait();
    expect(w.mockCalls.some(([c, a]) => c === "clean_anvil" && a.path.endsWith("/log"))).toBe(true);
  });

  test("Delete Logs and Caches in All Applications asks in the toolbar", async () => {
    // The mock's measurement counts no app: one is put in by hand.
    const { sizes } = await import("../src/settings");
    sizes.set("shop", { name: "shop", kind: "laravel", total: 524288000, safe: 104857600, rebuild: 0 });
    ($("#clean-all-safe") as HTMLButtonElement).disabled = false;
    $("#clean-all-safe").click();
    expect($("#clean-all-box .muted").textContent).toBe("Delete 100 MB? Users get logged out.");
    expect(w.mockCalls.some(([c]) => c === "clean_all_safe")).toBe(false);
    $$("#clean-all-box button")[1].click();
    await wait();
    expect(w.mockCalls.some(([c]) => c === "clean_all_safe")).toBe(true);
    expect($("#clean-all-box .asking")).toBeNull();
    expect($("#clean-all-box .muted").textContent).toMatch(/^Freed \d/);
    expect($("#clean-all-safe")).not.toBeNull();
  });
});

describe("general", () => {
  test("a folder's Remove asks in the row first: its apps leave Anvil and stop, the files stay", async () => {
    await open("general");
    const row = $("#paths .row");
    (row.querySelector("button") as HTMLElement).click();
    expect(row.querySelector(".asking .muted")?.textContent).toBe("Its applications leave Anvil and their processes stop. The files stay.");
    expect(w.mockCalls.some(([c]) => c === "remove_path")).toBe(false);
    (row.querySelectorAll(".asking button")[0] as HTMLElement).click();
    expect(row.querySelector(".asking")).toBeNull();
    (row.querySelector("button") as HTMLElement).click();
    (row.querySelectorAll(".asking button")[1] as HTMLElement).click();
    await wait();
    expect(w.mockCalls.filter(([c]) => c === "remove_path").at(-1)[1]).toEqual({ path: "/Users/x/Sites" });
  });

  test("domain, ports and the config folder wait under a closed Advanced, and still work", async () => {
    await open("general");
    const advanced = $("#general details.advanced") as HTMLDetailsElement;
    expect(advanced.open).toBe(false);
    for (const id of ["#tld", "#ports", "#config-dir"]) expect(advanced.querySelector(id)).not.toBeNull();
    expect($("#ports").textContent).toBe("HTTP 80, DNS 53");
    expect($("#address").closest("details")).toBeNull();
    const tld = $("#tld") as HTMLSelectElement;
    tld.value = "localhost";
    tld.dispatchEvent(new Event("change"));
    await wait();
    expect(w.mockCalls.filter(([c]) => c === "set_tld").at(-1)[1]).toEqual({ tld: "localhost" });
  });
});

describe("about", () => {
  test("Check for Updates… opens the Software Update window", async () => {
    await open("about");
    $("#update-check").click();
    await wait();
    expect(w.mockCalls.at(-1)[0]).toBe("show_updates");
  });
});

describe("plugins", () => {
  test("Request log starts on; its switch turns it off", async () => {
    await open("plugins");
    const box = $("#request-log") as HTMLInputElement;
    expect(box.checked).toBe(true);
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await wait();
    expect(w.mockCalls.at(-1)).toEqual(["set_request_log", { on: false }]);
  });

  test("plugins_switch_shows_the_line: anvil mcp and Copy only while Agents (MCP) is on", async () => {
    await open("plugins");
    const box = $("#mcp") as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect($("#mcp-line").hidden).toBe(true);
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    await wait();
    expect(w.mockCalls.at(-1)).toEqual(["set_mcp", { on: true }]);
    expect($("#mcp-line").hidden).toBe(false);
    expect(text("#mcp-line code")).toEqual(["claude mcp add anvil -- anvil mcp", "anvil mcp"]);
    // Copy copies the whole command of its row.
    let copied = "";
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (t: string) => void (copied = t) } });
    $(".mcp-copy").click();
    await wait();
    expect(copied).toBe("claude mcp add anvil -- anvil mcp");
    expect($(".mcp-copy").textContent).toBe("Copied");
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await wait();
    expect($("#mcp-line").hidden).toBe(true);
  });

  test("a plugin that is on gets its page under Features; turned off while shown, Features takes its place", async () => {
    await open("plugins");
    const nav = () => text(".nav button.plugin-nav:not([hidden])");
    expect(nav()).toEqual([]);
    const box = $("#mail-catcher") as HTMLInputElement;
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    await wait();
    expect(nav()).toEqual(["Mail catcher"]);
    await open("plugin-mail");
    expect($("#plugin-mail").hidden).toBe(false);
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await wait();
    expect(nav()).toEqual([]);
    expect($("#plugins").hidden).toBe(false);
    expect($('.nav [data-page="plugins"]').getAttribute("aria-current")).toBe("true");
  });

  test("Tunnel is off until switched on", async () => {
    await open("plugins");
    const box = $("#tunnel") as HTMLInputElement;
    expect(box.checked).toBe(false);
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    await wait();
    expect(w.mockCalls.at(-1)).toEqual(["set_tunnel", { on: true }]);
    expect(box.checked).toBe(true);
    // No page of its own: how to share is in its row.
    expect($(".nav button.plugin-nav:not([hidden])")).toBeNull();
  });

  test("Mail catcher: its address while on, a taken port inline, and mail from no app below", async () => {
    await open("plugins");
    const box = $("#mail-catcher") as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect($("#mail-line").hidden).toBe(true);
    expect($("#mail-other").hidden).toBe(true);
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    await wait(60);
    expect(w.mockCalls.filter(([c]: [string]) => c === "set_mail_catcher").at(-1)).toEqual(["set_mail_catcher", { on: true }]);
    expect($("#mail-line").hidden).toBe(false);
    expect($("#mail-line code").textContent).toBe("127.0.0.1:2525");
    expect($("#mail-error").hidden).toBe(true);
    // Mail whose SMTP username is no app's.
    expect($("#mail-other").hidden).toBe(false);
    expect($("#mail-other .log-msg").textContent).toBe("From nobody");
    w.mockMailError = "The mail catcher can't use port 2525: Address already in use (os error 48)";
    box.dispatchEvent(new Event("change"));
    await wait(60);
    expect($("#mail-error").textContent).toBe(w.mockMailError);
    expect($("#mail-error").hidden).toBe(false);
    w.mockMailError = null;
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await wait(60);
    expect($("#mail-line").hidden).toBe(true);
    expect($("#mail-other").hidden).toBe(true);
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
    expect($$("#presets .hk").length).toBe(0);
    (rows()[0].querySelector("button") as HTMLButtonElement).click();
    await wait();
    // The same editors as an app's: no Check keys, and the database is Settings' own, above.
    expect(text("#presets .preset-field > span:first-child")).toEqual(["Copy files", "Env keys", "After create", ""]);
    expect(text("#presets .hk-row:not(.hk-add) .hk-text, #presets .hk-row:not(.hk-add) .hk-key")).toEqual([
      ".env",
      "APP_URL",
      "composer install",
    ]);
    expect(($("#presets .hk-row select") as HTMLSelectElement).selectedOptions[0].textContent).toBe("This worktree’s address");
    expect($$("#presets .seg").length).toBe(0);
    const after = $$("#presets .hk")[2];
    (after.querySelector(".hk-add input") as HTMLInputElement).value = "php artisan migrate";
    (after.querySelector(".hk-add .inline-action:last-child") as HTMLButtonElement).click();
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

describe("Esc", () => {
  test("closes Settings, but not while typing in a field or from a bubble", async () => {
    let closed = 0;
    w.__TAURI__.window = { ...w.__TAURI__.window, getCurrentWindow: () => ({ close: () => closed++ }) };
    const esc = (target: EventTarget) => target.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    const field = document.createElement("input");
    document.body.append(field);
    esc(field);
    expect(closed).toBe(0);
    field.remove();
    const pop = document.createElement("div");
    pop.addEventListener("keydown", (e) => e.stopPropagation());
    document.body.append(pop);
    esc(pop);
    expect(closed).toBe(0);
    pop.remove();
    esc(document.body);
    expect(closed).toBe(1);
  });
});

describe("Storage › Automatic cleanup", () => {
  test("Clean Rust builds is on by default and saves when switched", async () => {
    await open("storage");
    const sw = $("#clean-rust") as HTMLInputElement;
    expect(sw.checked).toBe(true);
    sw.checked = false;
    sw.dispatchEvent(new Event("change"));
    await wait(30);
    expect(w.mockCalls.filter(([c]: [string]) => c === "set_clean_rust_builds").at(-1)[1]).toEqual({ on: false });
  });
});
