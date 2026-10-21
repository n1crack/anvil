// The main window driven through its DOM, with a mocked backend (test/mock.js).
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, describe, expect, test } from "vite-plus/test";

const w = window as any;
const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));
const calls = (cmd: string) => (w.mockCalls as [string, any][]).filter(([c]) => c === cmd);
const $ = (sel: string) => document.querySelector(sel) as HTMLElement;
const $$ = (sel: string) => [...document.querySelectorAll(sel)] as HTMLElement[];

async function select(name: string) {
  $(`#list [data-name="${name}"]`).click();
  await wait(80);
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
  localStorage.setItem("selected", "vuefinder");
  localStorage.setItem("mode", "apps");
  localStorage.setItem("tab", "overview");
  const html = readFileSync(resolve(import.meta.dirname, "../index.html"), "utf8");
  document.documentElement.innerHTML = html.replace(/<!doctype html>/i, "").replace(/<script[\s\S]*?<\/script>|<link[^>]*>/g, "");
  await import("./mock.js");
  w.mockSetup = { resolver: false, ports: false, trusted: false, php: true, shims: false };
  await import("../src/main");
  await wait(150);
});

describe("setup", () => {
  // The titlebar button only opens the bubble; each step runs from its own button there.
  const step = (name: string) => $$(".setup-pop button").find((b) => b.textContent === name)!;

  test("one button opens the steps; the password is said before Set up Anvil runs", async () => {
    w.mockSetup.php = false;
    (w as any).refresh();
    await wait(80);
    expect($$("#setup button").map((b) => b.textContent)).toEqual(["Set up Anvil"]);
    $("#setup .setup-go").click();
    await wait();
    expect(calls("setup_fix")).toEqual([]);
    expect($$(".setup-pop button").map((b) => b.textContent)).toEqual(["Set up Anvil", "Install PHP"]);
    expect($(".setup-pop p").textContent).toContain("Asks for your administrator password");
    expect(document.activeElement).toBe(step("Set up Anvil"));
    w.mockSetup.trusted = false;
    step("Set up Anvil").click();
    await wait(80);
    expect($(".setup-pop")).toBeNull();
    expect(calls("setup_fix").map(([, a]) => a.step)).toEqual(["all", "trust"]);
    expect($$("#setup button").map((b) => b.textContent)).toEqual(["Install PHP"]);
    w.mockSetup.php = true;
    (w as any).refresh();
    await wait(80);
    expect($("#setup").hidden).toBe(true);
  });

  test("several installs left: Finish setup with their count", async () => {
    Object.assign(w.mockSetup, { node: false, git: false });
    (w as any).refresh();
    await wait(80);
    expect($$("#setup button").map((b) => b.textContent)).toEqual(["Finish setup (2)"]);
    Object.assign(w.mockSetup, { node: true, git: true });
  });

  test("Install Git where Anvil can install it (git false), and nowhere else", async () => {
    w.mockSetup.git = false;
    (w as any).refresh();
    await wait(80);
    expect($$("#setup button").map((b) => b.textContent)).toEqual(["Install Git"]);
    w.mockDelay = { setup_fix: 60 };
    $("#setup button").click();
    step("Install Git").click();
    await wait(10);
    expect($("#setup button").textContent).toBe("Installing Git…");
    w.mockSetup.git = true;
    await wait(120);
    w.mockDelay = {};
    expect(calls("setup_fix").at(-1)![1]).toEqual({ step: "git" });
    expect($("#setup").hidden).toBe(true);
  });

  test("Install Node.js when an app needs it and none is installed", async () => {
    w.mockSetup.node = false;
    (w as any).refresh();
    await wait(80);
    expect($$("#setup button").map((b) => b.textContent)).toEqual(["Install Node.js"]);
    w.mockDelay = { setup_fix: 60 };
    $("#setup button").click();
    step("Install Node.js").click();
    await wait(10);
    expect($("#setup button").textContent).toBe("Installing Node.js…");
    w.mockSetup.node = true;
    await wait(120);
    w.mockDelay = {};
    expect(calls("setup_fix").at(-1)![1]).toEqual({ step: "node" });
    expect($("#setup").hidden).toBe(true);
  });
});

describe("sidebar menu", () => {
  test("Open Folder comes first, for every kind, and shows that app's folder in Finder", async () => {
    for (const name of ["shop", "stack"]) {
      $(`#list [data-name="${name}"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
      await wait(80);
      const item = (w.mockMenu as any[])[0];
      expect(item.text).toBe("Open Folder");
      item.action();
      await wait();
      expect(calls("open").at(-1)![1]).toEqual({ site: name, target: "finder" });
    }
  });
});

describe("https", () => {
  test("the Settings tab's HTTPS switch turns it on or off with a spinner, the header lock following; none for Compose", async () => {
    await select("shop");
    const box = $("#app-https") as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect($(".https-cell small").textContent).toBe("Serve at https:// with a trusted local certificate; http:// redirects there");
    expect($("#site .site-meta .lock.on")).not.toBeNull();
    w.mockDelay = { set_secure: 200 };
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await wait(40);
    expect(box.disabled).toBe(true);
    expect(($(".https-cell .securing") as HTMLElement).hidden).toBe(false);
    expect($(".https-cell .securing").textContent).toBe("Turning off…");
    await wait(300);
    w.mockDelay = {};
    expect(calls("set_secure").at(-1)![1]).toEqual({ site: "shop", on: false });
    const now = $("#app-https") as HTMLInputElement;
    expect([now.checked, now.disabled]).toEqual([false, false]);
    expect(($(".https-cell .securing") as HTMLElement).hidden).toBe(true);
    // Plain HTTP: a red open lock.
    expect($("#site .site-meta .lock.off")).not.toBeNull();
    now.checked = true;
    now.dispatchEvent(new Event("change"));
    await wait(80);
    expect($("#site .site-meta .lock.on")).not.toBeNull();
    await select("stack");
    expect($("#app-https")).toBeNull();
  });

  test("Remove Link… asks in a bubble, naming the settings that go, before it removes", async () => {
    const { snap } = await import("../src/app");
    const linked = w.mockSite("blog", "laravel", { linked: true, path: "/Users/x/Code/blog", parked_in: null });
    w.mockSnap.sites.push(linked);
    snap.sites.push(structuredClone(linked));
    $("#search").dispatchEvent(new Event("input"));
    await wait();
    $(`#list [data-name="blog"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait(80);
    (w.mockMenu as any[]).find((i) => i.text === "Remove Link…").action();
    await wait();
    expect(calls("remove_site")).toEqual([]);
    expect($(".pop-confirm p").textContent).toContain(
      "its settings here are deleted: PHP version, HTTPS, php.ini and extensions. Its folder stays.",
    );
    ($(".pop-confirm .primary") as HTMLElement).click();
    await wait(80);
    expect(calls("remove_site")).toEqual([["remove_site", { site: "blog" }]]);
    expect($(`#list [data-name="blog"]`)).toBeNull();
    // Later tests count remove_site from none.
    w.mockCalls.splice(0, Infinity, ...w.mockCalls.filter(([c]) => c !== "remove_site"));
  });

  test("the row shows a spinner while HTTPS is turned on or off", async () => {
    w.mockDelay = { set_secure: 200 };
    $(`#list [data-name="shop"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait(80);
    const item = (w.mockMenu as any[]).find((i) => i.text === "Use HTTPS");
    expect(item.checked).toBe(true);
    item.action();
    await wait(40);
    expect($(`#list [data-name="shop"] .spin-dot`)).toBeTruthy();
    await wait(300);
    expect($(`#list [data-name="shop"] .spin-dot`)).toBeNull();
    w.mockDelay = {};
  });
});

describe("web root", () => {
  test("the Settings tab's Web root: detected as the placeholder, set inline, a refusal under it, Reset; none for proxied or Compose apps", async () => {
    await select("shop");
    const input = () => $("#app-root") as HTMLInputElement;
    const reset = () => $(".root-reset");
    const fail = () => $(".root-error");
    expect([input().value, input().placeholder, reset().hidden, fail().hidden]).toEqual(["", "public (detected)", true, true]);
    input().value = " web ";
    input().dispatchEvent(new Event("change"));
    await wait(80);
    expect(calls("set_root").at(-1)![1]).toEqual({ site: "shop", dir: "web" });
    expect([input().value, reset().hidden]).toEqual(["web", false]);
    expect($('[data-field="root"]').textContent).toBe("~/Sites/shop/web");

    input().value = "dist";
    input().dispatchEvent(new Event("change"));
    await wait(80);
    expect(fail().textContent).toBe("dist isn't a folder in /Users/x/Sites/shop");
    expect(fail().hidden).toBe(false);
    expect($("#error").hidden).toBe(true);
    expect(input().value).toBe("dist");

    reset().click();
    await wait(80);
    expect(calls("set_root").at(-1)![1]).toEqual({ site: "shop", dir: null });
    expect([input().value, reset().hidden, fail().hidden]).toEqual(["", true, true]);

    await select("vuefinder");
    expect((input().closest(".row") as HTMLElement).hidden).toBe(true);
    await select("stack");
    expect(input()).toBeNull();
  });
});

describe("app extensions", () => {
  const exts = () =>
    $$('[data-panel="settings"] .app-exts .ver-ext').map((l) => {
      const input = l.querySelector("input") as HTMLInputElement;
      return { name: l.querySelector(".ver-ext-name")?.textContent, on: input.checked, held: input.disabled, text: l.textContent };
    });
  const flip = (name: string) =>
    (
      $$('[data-panel="settings"] .app-exts .ver-ext')
        .find((l) => l.textContent?.startsWith(name))!
        .querySelector("input") as HTMLInputElement
    ).click();

  test("app_tab_extensions: the catalog as switches for this app; the version's held on; Building… and errors inline", async () => {
    w.mockSnap.php_ext = { "8.4": ["apcu"] };
    w.mockDelay = { app_ext: 100 };
    try {
      await wait(550); // refresh reads the state again only half a second after the last read
      w.refresh();
      await wait(80);
      await select("shop");
      expect($('[data-panel="settings"] .app-exts-row dt').textContent).toBe("PHP extensions");
      expect(exts().map((e) => [e.name, e.on, e.held])).toEqual([
        ["xdebug", false, false],
        ["pcov", false, false],
        ["apcu", true, true],
        ["igbinary", false, false],
        ["msgpack", false, false],
      ]);
      flip("xdebug");
      await wait(10);
      expect(calls("app_ext").at(-1)![1]).toEqual({ site: "shop", name: "xdebug", on: true });
      expect(exts()[0]).toMatchObject({ text: "xdebugBuilding…", held: true });
      await wait(150);
      expect(exts()[0]).toMatchObject({ on: true, held: false, text: "xdebug" });
      expect(w.mockSnap.sites.find((s: any) => s.name === "shop").ext).toEqual(["xdebug"]);

      delete w.mockDelay.app_ext;
      w.mockExtError = "Building pcov failed — it needs the Xcode Command Line Tools: xcode-select --install";
      flip("pcov");
      await wait(80);
      const error = $('[data-panel="settings"] .app-ext-error');
      expect(error.textContent).toContain("xcode-select --install");
      expect([...error.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Install command line tools"]);
      expect(exts()[1].on).toBe(false);
      expect($$('[role="alert"]:not([hidden]), .toast:not([hidden])').length).toBe(0);
      delete w.mockExtError;

      flip("xdebug");
      await wait(80);
      expect(calls("app_ext").at(-1)![1]).toEqual({ site: "shop", name: "xdebug", on: false });
      expect($('[data-panel="settings"] .app-ext-error')).toBeNull();

      // No PHP of Anvil's: no line.
      await select("vuefinder");
      expect($('[data-panel="settings"] .app-exts-row')).toBeNull();
    } finally {
      w.mockSnap.php_ext = {};
      w.mockDelay = {};
      delete w.mockExtError;
    }
  });

  test("app_ext_not_built: one on without a build for the app's PHP shows off, Not built; on builds it", async () => {
    const shop = w.mockSnap.sites.find((s: any) => s.name === "shop");
    w.mockSnap.php_ext = { "8.4": ["apcu"] };
    w.mockSnap.php_ext_built = { "8.4": [] };
    shop.ext = ["xdebug"];
    try {
      await wait(550);
      w.refresh();
      await wait(80);
      await select("shop");
      const [xdebug, , apcu] = exts();
      expect(xdebug).toMatchObject({ on: false, held: false, text: "xdebugNot built" });
      expect(apcu).toMatchObject({ on: false, held: true, text: "apcuNot built" });
      flip("xdebug");
      await wait(10);
      expect(calls("app_ext").at(-1)![1]).toEqual({ site: "shop", name: "xdebug", on: true });
    } finally {
      w.mockSnap.php_ext = {};
      w.mockSnap.php_ext_built = {};
      shop.ext = [];
    }
  });
});

describe("sidebar", () => {
  test("an app's dot: green while anything of it runs, amber while starting, red after a crash", async () => {
    w.mockSnap.app_states = { shop: "running", vuefinder: "starting", notes: "crashed" };
    await wait(550); // refresh reads the state again only half a second after the last read
    w.refresh();
    await wait(80);
    const badge = (name: string) => $(`#list [data-name="${name}"] .run-badge`);
    expect(badge("shop").className).toBe("run-badge on");
    expect(badge("vuefinder").className).toBe("run-badge warm");
    expect([badge("notes").className, badge("notes").title]).toEqual(["run-badge bad", "A process crashed"]);
    w.mockSnap.app_states = {};
    await wait(550);
    w.refresh();
    await wait(80);
    expect(badge("shop")).toBeNull();
  });
});

describe("overview", () => {
  test("one Dev line with a dot per process, and no process or command lists", async () => {
    await select("vuefinder");
    const line = $(".procs .dev-summary");
    expect(line.textContent).toContain("Dev");
    // The count is the header's alone.
    expect(line.querySelector(".dev-summary-state")).toBeNull();
    expect([...line.querySelectorAll(".dot")].map((d) => d.className)).toEqual(["dot ", "dot on", "dot "]);
    expect($$(".procs .proc")).toHaveLength(0);
    expect($$("#site .res-sub").map((h) => h.textContent)).not.toContain("Commands");
    expect($$("#site .res-sub").map((h) => h.textContent)).not.toContain("Processes");
    expect($('[data-field="path"]').textContent).toBe("~/Sites/vuefinder");
    line.click();
    await wait(80);
    expect($('.tabs [data-tab="dev"]').getAttribute("aria-selected")).toBe("true");
    $('.tabs [data-tab="overview"]').click();
  });

  test("the header says what runs and opens Dev; Dev's Start leaves out what Dev start does", async () => {
    await select("vuefinder");
    expect($("#site [data-all]")).toBeNull();
    // Configure in the bar; Start or Stop in All's row, ▶ or ■ in each process's; Run
    // lives with the commands.
    expect($$(".dev-bar button").map((b) => b.className)).toEqual(["dev-configure"]);
    expect($('.dev-item[data-key="all"] .dev-all').className).toBe("dev-all dev-stop dev-act");
    expect($$(".dev-item .dev-proc").length).toBe(w.mockOv.vuefinder.rows.length);
    expect($(".dev-list .dev-cmds .dev-cmds-head .dev-run")).not.toBeNull();
    const ov = w.mockOv.vuefinder;
    const before = structuredClone(ov.rows);
    for (const r of ov.rows) r.running = false;
    await select("shop");
    await select("vuefinder");
    expect($(".dev-list .dev-all").textContent).toBe("Start");
    expect($(".dev-list .dev-all").className).toBe("dev-all dev-start dev-act");
    for (const r of ov.rows) r.running = r.id !== "docs-site";
    ov.rows.find((r) => r.id === "docs-site").manual = true;
    await select("shop");
    await select("vuefinder");
    expect($(".dev-list .dev-all").textContent).toBe("Stop");
    expect($("#site .state-text").textContent).toBe("2 running");
    ($("#site .state-link") as HTMLElement).click();
    await wait(80);
    expect($('.tabs [data-tab="dev"]').getAttribute("aria-selected")).toBe("true");
    // Stop: "Stopping…" with a spinner while it works, then Dev stop for everything.
    const n = calls("process_run").length;
    w.mockDelay = { process_run: 60 };
    ($(".dev-list .dev-all") as HTMLElement).click();
    await wait(10);
    expect($(".dev-list .dev-all").textContent).toBe("Stopping…");
    expect($(".dev-list .dev-all .spin-dot")).not.toBeNull();
    await wait(120);
    w.mockDelay = {};
    expect(calls("process_run").slice(n)[0][1]).toEqual({ site: "vuefinder", name: null, action: "stop" });
    expect($(".dev-list .dev-all").textContent).toBe("Stop");
    ov.rows = before;
    $('.tabs [data-tab="overview"]').click();
    await select("shop");
  });
});

describe("dev", () => {
  const devTab = () => $('.tabs [data-tab="dev"]');
  const chosen = () => $(".dev-item[aria-selected=true]")?.dataset.key;
  const item = (key: string) => $(`.dev-item[data-key="${key}"]`);
  const lines = () => $$(".dev-out .dev-line").map((l) => l.textContent);

  test("the list: All, each process in its color, then the runs with how they ended", async () => {
    w.mockLogs = { app: "server up\n", "proc:docs-site": "" };
    await select("vuefinder");
    devTab().click();
    await wait(80);
    expect(devTab().textContent).toBe("Dev");
    expect($('[data-panel="dev"]').hidden).toBe(false);
    expect(chosen()).toBe("all");
    expect($$(".dev-item").map((i) => i.dataset.key)).toEqual([
      "all",
      "app",
      "proc:docs-dev",
      "proc:docs-site",
      "once:test",
      "once:preview",
      "once:format",
    ]);
    const named = (key: string) => item(key).querySelector(".dev-name")!.className;
    expect([named("all"), named("app"), named("proc:docs-dev"), named("proc:docs-site")]).toEqual([
      "dev-name",
      "dev-name pc0",
      "dev-name pc1",
      "dev-name pc2",
    ]);
    // The runs sit in a box of their own below the processes.
    expect($$(".dev-list > .dev-card").length).toBe(2);
    expect($(".dev-cmds").contains(item("once:test"))).toBe(true);
    // A run's status sits in the dot column: a spinner, then a check or a cross (the code in its tooltip).
    expect(item("once:test").querySelector(".dev-status .spin-dot")).not.toBeNull();
    expect(item("once:preview").firstElementChild!.className).toBe("dev-status ok");
    expect(item("once:preview").querySelector(".dev-status svg")).not.toBeNull();
    const bad = item("once:format").firstElementChild as HTMLElement;
    expect([bad.className, bad.title]).toEqual(["dev-status bad", "Exited with code 1"]);
    expect(item("app").firstElementChild!.className).toContain("dot");
  });

  test("All merges the processes' lines, each tagged with its name in its color", async () => {
    await wait(600);
    expect(lines()).toContain("dev       │ server up");
    expect(lines().some((l) => l!.startsWith("docs:dev  │ ✓ built in"))).toBe(true);
    const tag = (name: string) => $$(".dev-out .dev-tag").find((t) => t.textContent!.startsWith(`${name} `))!.className;
    expect(tag("dev")).toBe("dev-tag pc0");
    expect(tag("docs:dev")).toBe("dev-tag pc1");
  });

  test("one process: colored output, new output appended without redrawing, \\r redraws the last line", async () => {
    w.mockLogs["proc:docs-dev"] = "\x1b[32mready\x1b[39m in 412 ms\n12%\r50%";
    item("proc:docs-dev").click();
    await wait(600);
    expect(chosen()).toBe("proc:docs-dev");
    expect($(".dev-head .console-title").textContent).toBe("docs:dev");
    expect($(".dev-head .dev-cmd").textContent).toBe("npm run docs:dev");
    expect($(".dev-out span.ansi2").textContent).toBe("ready");
    expect(lines()).toEqual(["ready in 412 ms", "50%"]);
    const first = $(".dev-out .dev-line");
    w.mockLogs["proc:docs-dev"] += "\r100%\nwatching for changes\n";
    await wait(600);
    expect(lines()).toEqual(["ready in 412 ms", "100%", "watching for changes"]);
    expect($(".dev-out .dev-line")).toBe(first);
    expect($(".dev-empty").hidden).toBe(true);
  });

  test("Restart in the pane restarts that process", async () => {
    const before = calls("process_run").length;
    ($('.dev-head [aria-label="Restart docs:dev"]') as HTMLElement).click();
    await wait();
    expect(
      calls("process_run")
        .slice(before)
        .map(([, a]) => a),
    ).toEqual([{ site: "vuefinder", name: "docs-dev", action: "restart" }]);
  });

  test("a right-click offers Stop, Restart, Open in Browser, Edit Address and Clear; Clear empties it", async () => {
    item("proc:docs-dev").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait();
    const menu = w.mockMenu as any[];
    expect(menu.map((i) => i.text ?? i.item)).toEqual(["Stop", "Restart", "Open in Browser", "Edit Address…", "Separator", "Clear"]);
    menu.find((i) => i.text === "Clear").action();
    await wait();
    expect(calls("log_clear").at(-1)![1]).toEqual({ site: "vuefinder", source: "proc:docs-dev" });
    expect(lines()).toEqual([]);
  });

  test("Edit Address… is the prompt sheet: the label, what it opens, Save", async () => {
    item("proc:docs-dev").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait();
    (w.mockMenu as any[]).find((i) => i.text === "Edit Address…").action();
    await wait();
    expect(($("#ask") as HTMLDialogElement).open).toBe(true);
    expect([$("#ask-label").textContent, $("#ask-name").textContent, $("#ask-ok").textContent]).toEqual([
      "Address of docs:dev",
      "Label",
      "Save",
    ]);
    const input = $("#ask-input") as HTMLInputElement;
    input.value = "manual";
    input.dispatchEvent(new Event("input"));
    expect($("#ask-hint").textContent).toMatch(/^Opens vuefinder--manual\./);
    $("#ask-form").dispatchEvent(new Event("submit"));
    await wait(80);
    expect(calls("set_process").at(-1)![1]).toEqual({ site: "vuefinder", id: "docs-dev", host: "manual" });
    expect(($("#ask") as HTMLDialogElement).open).toBe(false);
    // Empty: back to the automatic label, as the sheet says.
    item("proc:docs-dev").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait();
    (w.mockMenu as any[]).find((i) => i.text === "Edit Address…").action();
    await wait();
    expect($("#ask-hint").textContent).toBe("Empty: back to the automatic label");
    $("#ask-form").dispatchEvent(new Event("submit"));
    await wait(80);
    expect(calls("set_process").at(-1)![1]).toEqual({ site: "vuefinder", id: "docs-dev", resetHost: true });
    expect(($("#ask") as HTMLDialogElement).open).toBe(false);
  });

  test("↓ moves the selection; a stopped process says Not running, with a small ▶", async () => {
    $(".dev-list").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await wait(600);
    expect(chosen()).toBe("proc:docs-site");
    expect($(".dev-empty").hidden).toBe(false);
    expect($(".dev-empty").textContent).toContain("Not running");
    const before = calls("process_run").length;
    ($('.dev-empty [aria-label="Start docs:site"]') as HTMLElement).click();
    await wait();
    expect(calls("process_run").slice(before)[0][1]).toEqual({ site: "vuefinder", name: "docs-site", action: "start" });
  });

  test("a start says how many characters the output pane fits (COLUMNS, LINES)", async () => {
    const out = $(".dev-out");
    Object.defineProperty(out, "clientWidth", { value: 660, configurable: true });
    Object.defineProperty(out, "clientHeight", { value: 480, configurable: true });
    const rect = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function () {
      return { ...rect.call(this), width: 132, height: 16 }; // 20 chars of 6.6 px, 16 px lines
    };
    try {
      const before = calls("process_run").length;
      ($('.dev-empty [aria-label="Start docs:site"]') as HTMLElement).click();
      await wait();
      expect(calls("process_run").slice(before)[0][1]).toEqual({
        site: "vuefinder",
        name: "docs-site",
        action: "start",
        columns: 100,
        lines: 30,
      });
    } finally {
      HTMLElement.prototype.getBoundingClientRect = rect;
      delete (out as any).clientWidth;
      delete (out as any).clientHeight;
    }
  });

  test("Run ▾ lists the one-off scripts only, and runs one after a confirm", async () => {
    $(".dev-cmds .dev-run").click();
    const names = $$(".dev-run-menu .dev-run-name").map((n) => n.textContent);
    // Each source titled, even alone, in its color: package.json Node's green.
    const groups = $$(".dev-run-menu .dev-run-group");
    expect(groups.map((g) => g.textContent)).toEqual([...new Set(w.mockOv.vuefinder.commands.map((c) => c.source))]);
    expect(groups.find((g) => g.textContent === "package.json")?.classList.contains("pc4")).toBe(true);
    expect(names).toEqual(w.mockOv.vuefinder.commands.map((c) => c.name));
    expect(names).not.toContain("docs:dev");
    const build = $$(".dev-run-item").find((b) => b.querySelector(".dev-run-name")!.textContent === "build")!;
    // The script is the tooltip: the menu stays a short list of names.
    expect(build.title).toBe("vite build");
    expect(build.textContent).toBe("build");
    build.click();
    await wait();
    expect(calls("command_run")).toHaveLength(0);
    const pop = $(".pop-confirm");
    expect(pop.textContent).toContain("Run npm run build?");
    (pop.querySelector(".primary") as HTMLElement).click();
    await wait(80);
    expect(calls("command_run").map(([, a]) => a)).toEqual([{ site: "vuefinder", id: "build" }]);
    expect(chosen()).toBe("once:build");
    expect($(".dev-head .dev-cmd").textContent).toBe("npm run build");
  });

  test("a run's × dismisses it", async () => {
    (item("once:preview").querySelector(".dev-dismiss") as HTMLElement).click();
    await wait(80);
    expect(calls("once_stop").at(-1)![1]).toEqual({ site: "vuefinder", id: "preview" });
    expect(item("once:preview")).toBeNull();
  });

  test("Configure: changes wait for Done; Cancel drops them and closes", async () => {
    const open = async () => {
      $(".dev-bar .dev-configure").click();
      await wait();
    };
    const names = () => $$(".dev-conf-table .dev-conf-name").map((n) => n.textContent);
    const box = (aria: string) => $(`.dev-conf [aria-label="${aria}"]`) as HTMLInputElement;
    const foot = (t: string) => $$(".dev-conf-foot button").find((b) => b.textContent === t)! as HTMLButtonElement;
    const type = (aria: string, v: string) => {
      box(aria).value = v;
      box(aria).dispatchEvent(new Event("input"));
    };
    const saved = () => calls("set_process").length + calls("process_add").length;

    await open();
    expect($(".dev-main").hidden).toBe(true);
    // Table | JSON at the right end of the toolbar, where Start or Stop was.
    expect($(".dev-bar").lastElementChild!.classList.contains("dev-conf-mode")).toBe(true);
    expect(names()).toEqual(["dev", "docs:dev", "docs:site"]);
    expect(box("dev in Dev start").disabled).toBe(true);
    expect($('.dev-conf [aria-label="Remove dev"]')).toBeNull();

    // Edits, a switch and a new process: nothing is saved until Done.
    const before = saved();
    type("Command for docs:dev", "npm run docs:dev -- --open");
    box("docs:site in Dev start").click();
    foot("+ Add process").click();
    type("Name of the new process", "worker");
    type("Command of the new process", "node worker.js");
    box("Command of the new process").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await wait();
    expect(names()).toEqual(["dev", "docs:dev", "docs:site", "worker"]);
    expect(saved()).toBe(before);
    foot("Done").click();
    await wait();
    expect(
      calls("set_process")
        .slice(-2)
        .map((c) => c[1]),
    ).toEqual([
      { site: "vuefinder", id: "docs-dev", command: "npm run docs:dev -- --open" },
      { site: "vuefinder", id: "docs-site", manual: true },
    ]);
    expect(calls("process_add").at(-1)![1]).toEqual({ site: "vuefinder", name: "worker", command: "node worker.js" });
    expect($(".dev-conf").hidden).toBe(true);

    // Cancel drops the changes and closes Configure.
    await open();
    const after = saved();
    type("Command for docs:site", "something else");
    foot("Cancel").click();
    await wait();
    expect($(".dev-conf").hidden).toBe(true);
    expect(saved()).toBe(after);

    // Closing from the toolbar with changes asks first.
    await open();
    type("Command for docs:site", "something else");
    $(".dev-bar .dev-configure").click();
    expect($(".dev-conf").hidden).toBe(false);
    ($(".pop-confirm .primary") as HTMLElement).click();
    await wait();
    expect($(".dev-conf").hidden).toBe(true);
    expect(saved()).toBe(after);

    // An empty command is refused inline, not in a toast, and Configure stays open.
    await open();
    type("Command for docs:site", " ");
    foot("Done").click();
    await wait();
    expect($(".dev-conf-error").textContent).toBe("docs:site: a command is needed");
    expect($(".toast")).toBeNull();
    expect(saved()).toBe(after);

    const reset = foot("Reset to detected");
    reset.click();
    expect(calls("processes_reset")).toHaveLength(0);
    ($(".pop-confirm .primary") as HTMLElement).click();
    await wait();
    expect(calls("processes_reset")).toHaveLength(1);
    expect(names()).toEqual(["dev", "docs:dev", "docs:site"]);

    // JSON: the same processes in an editor, the same Cancel and Done.
    ($('.dev-conf-mode [data-id="json"]') as HTMLElement).click();
    await wait(400);
    expect($(".dev-conf-table")).toBeNull();
    const editor = $(".dev-json-editor .cm-content");
    expect(editor).toBeTruthy();
    expect(JSON.parse(editor.textContent!.replace(/\s+/g, " ")).map((x) => x.name)).toEqual(["dev", "docs:dev", "docs:site"]);
    expect($$(".dev-json .dev-conf-foot button").map((b) => b.textContent)).toEqual(["Format", "Cancel", "Done"]);
    foot("Cancel").click();
    await wait();
    expect($(".dev-main").hidden).toBe(false);
    expect($(".dev-conf").hidden).toBe(true);
  });

  test("keys act with the focus in the list or output, never in a field nor with ⌘", async () => {
    const key = (at: HTMLElement, k: string, o = {}) => at.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, ...o }));
    const runs = (from: number) =>
      calls("process_run")
        .slice(from)
        .map(([, a]) => [a.name, a.action]);
    const list = $(".dev-list");
    const out = $(".dev-out");
    // The line, and when the pane is too narrow for it Keys, which opens them as a card.
    expect($(".dev-keys .dev-keys-full").textContent).toBe("↑↓ select · s start · x stop · r restart · q stop all · c clear");
    const card = $(".dev-keys-card") as HTMLElement;
    expect(card.hidden).toBe(true);
    ($(".dev-keys-short") as HTMLElement).click();
    expect(card.hidden).toBe(false);
    expect($$(".dev-keys-card kbd").map((k) => k.textContent)).toEqual(["↑↓", "s", "x", "r", "q", "c"]);
    document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(card.hidden).toBe(true);
    expect(out.tabIndex).toBe(0);
    $('.tabs [data-tab="overview"]').click();
    devTab().click();
    await wait(80);
    expect(document.activeElement).toBe(list);

    item("app").click();
    await wait(80);
    key(list, "j");
    await wait(80);
    expect(chosen()).toBe("proc:docs-dev");
    key(out, "k");
    await wait(80);
    expect(chosen()).toBe("app");
    key(list, "ArrowDown");
    await wait(80);
    expect(chosen()).toBe("proc:docs-dev");

    // A modifier, a field (in the pane or elsewhere): not the pane's keys.
    let before = calls("process_run").length;
    key(list, "j", { metaKey: true });
    key(list, "x", { ctrlKey: true });
    const field = document.createElement("input");
    $(".dev-head").append(field);
    key(field, "x");
    key(field, "j");
    field.remove();
    key($("#search") ?? document.body, "x");
    await wait(80);
    expect(chosen()).toBe("proc:docs-dev");
    expect(runs(before)).toEqual([]);

    // docs:dev runs: x stops, s does nothing, r restarts; c clears its output.
    key(out, "s");
    key(out, "x");
    key(list, "r");
    await wait(80);
    expect(runs(before)).toEqual([
      ["docs-dev", "stop"],
      ["docs-dev", "restart"],
    ]);
    key(list, "c");
    await wait();
    expect(calls("log_clear").at(-1)![1]).toEqual({ site: "vuefinder", source: "proc:docs-dev" });

    // docs:site is stopped: s starts it.
    key(list, "j");
    await wait(80);
    before = calls("process_run").length;
    key(list, "x");
    key(list, "s");
    await wait(80);
    expect(runs(before)).toEqual([["docs-site", "start"]]);

    // All: r restarts what runs, q is the header's Stop.
    item("all").click();
    await wait(80);
    before = calls("process_run").length;
    key(list, "r");
    key(list, "q");
    await wait(80);
    expect(runs(before)).toEqual([
      ["docs-dev", "restart"],
      [null, "stop"],
    ]);
  });

  test("a crashed process has a red dot; a folder has no Dev tab", async () => {
    w.mockOv.shop.rows[1].starting = true;
    await select("shop");
    expect(item("proc:logs").querySelector(".dot.bad")).not.toBeNull();
    // Starting is amber, as in the sidebar; All is red while one has crashed, also then.
    expect(item("proc:queue").querySelector(".dot.amber")).not.toBeNull();
    expect(item("all").querySelector(".dot.bad")).not.toBeNull();
    w.mockOv.shop.rows[1].starting = false;
    // A crash says how it ended; its ✕ dismisses, the ■ only stops.
    item("proc:logs").click();
    await wait(80);
    expect($(".dev-head .dev-exit.bad").textContent).toBe("Exited with code 1");
    const dismiss = $$(".dev-head button").find((b) => b.getAttribute("aria-label") === "Dismiss logs")!;
    expect(dismiss.querySelector("path")!.getAttribute("d")).toBe("M4.5 4.5l7 7M11.5 4.5l-7 7");
    item("all").click();
    await wait(40);
    await select("notes");
    expect(devTab().hidden).toBe(true);
    expect($('.tabs [data-tab="overview"]').getAttribute("aria-selected")).toBe("true");
    w.mockLogs = null;
  });
});

describe("containers", () => {
  const tabBtn = () => $('.tabs [data-tab="containers"]');
  const item = (key: string) => $(`.containers .dev-item[data-key="${key}"]`);
  const lines = () => $$('[data-panel="containers"] .dev-line').map((l) => l.textContent);
  const keyDown = (key: string) => $(".containers .dev-list").dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  let saved: any;

  test("the Overview has one Containers line with a dot each, which opens the tab", async () => {
    saved = structuredClone(w.mockOv.stack);
    await select("stack");
    const line = $(".procs .dev-summary");
    expect(line.querySelector(".dev-summary-title")!.textContent).toBe("Containers");
    expect(line.querySelector(".dev-summary-state")).toBeNull();
    // The header counts the exited worker as crashed: red, never green.
    expect($("#site .state-text").textContent).toBe("3 running, 1 crashed");
    expect($("#site .run-box .dot").className).toBe("dot bad");
    expect([...line.querySelectorAll(".dot")].map((d) => d.className)).toEqual(["dot on", "dot on", "dot amber", "dot bad"]);
    expect($$(".procs .proc, .procs .proc-log")).toHaveLength(0);
    expect($('.tabs [data-tab="dev"]').hidden).toBe(true);
    expect(tabBtn().hidden).toBe(false);
    line.click();
    await wait(80);
    expect(tabBtn().getAttribute("aria-selected")).toBe("true");
  });

  test("the list: All, then each service with its state, port and health", async () => {
    expect($$(".containers .dev-item").map((i) => i.dataset.key)).toEqual(["all", "web", "db", "cache", "worker"]);
    const dot = (k: string) => item(k).firstElementChild!.className;
    expect([dot("web"), dot("db"), dot("cache"), dot("worker")]).toEqual(["dot on", "dot on", "dot amber", "dot bad"]);
    expect(item("db").querySelector(".dev-exit")!.textContent).toBe(":5432 · healthy");
    expect(item("worker").querySelector(".dev-exit")!.textContent).toBe("exited (1)");
    // Start, as in Dev: something is down. No Run nor Configure.
    expect($$(".containers .dev-bar button").map((b) => b.className)).toEqual(["dev-all dev-start"]);
  });

  test("All merges the logs by time, each line tagged with its service in its color", async () => {
    await wait(50);
    const l = lines();
    expect(l[0]).toBe("db     │ PostgreSQL init process complete; ready for start up.");
    expect(l[1]).toBe("web    │ /docker-entrypoint.sh: Configuration complete; ready for start up");
    expect(l.at(-1)).toBe('web    │ 172.18.0.1 - - [17/Oct/2026:10:00:12 +0000] "GET /favicon.ico HTTP/1.1" 404 153');
    const tag = $$('[data-panel="containers"] .dev-tag').find((t) => t.textContent!.startsWith("db "))!;
    expect(tag.className).toBe("dev-tag pc1");
    // Then only what came after the last line read.
    const n = calls("container_log_read").length;
    await wait(1100);
    const next = calls("container_log_read")
      .slice(n)
      .find(([, a]) => a.service === "web")![1];
    expect(next.after).toBe(w.mockContainerLogs.web.length);
    expect(lines()).toHaveLength(l.length);
  });

  test("one container: its log, colored; keys start, stop, restart and clear", async () => {
    item("worker").click();
    await wait(50);
    expect(lines()).toEqual(["Starting queue worker", "Error: connect ECONNREFUSED 172.18.0.3:6379", "exited with code 1"]);
    expect($('[data-panel="containers"] .dev-out span.ansi1').textContent).toBe("Error:");
    expect($('[data-panel="containers"] .dev-head .dev-cmd').textContent).toBe("exited (1)");
    const n = calls("container_run").length;
    keyDown("x"); // already stopped: nothing
    keyDown("s");
    await wait(50);
    keyDown("r");
    keyDown("x"); // still restarting: nothing
    await wait(50);
    keyDown("x");
    await wait(50);
    expect(
      calls("container_run")
        .slice(n)
        .map(([, a]) => [a.service, a.action]),
    ).toEqual([
      ["worker", "start"],
      ["worker", "restart"],
      ["worker", "stop"],
    ]);
    keyDown("c");
    expect(lines()).toEqual([]);
    keyDown("ArrowUp");
    await wait(50);
    expect($(".containers .dev-item[aria-selected=true]").dataset.key).toBe("cache");
  });

  test("a right-click offers Stop, Restart and Open in Browser on its port", async () => {
    item("web").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait();
    const menu = w.mockMenu as any[];
    expect(menu.map((i) => i.text ?? i.item)).toEqual(["Stop", "Restart", "Separator", "Open in Browser"]);
    menu.at(-1).action();
    await wait();
    expect(calls("open_url").at(-1)![1]).toEqual({ url: "http://localhost:8080" });
  });

  test("Start and Stop in the toolbar run compose for the project, never down; q stops all", async () => {
    const n = calls("containers_run").length;
    w.mockDelay = { containers_run: 60 };
    $(".containers .dev-bar .dev-all").click();
    await wait(10);
    expect($(".containers .dev-bar .dev-all").textContent).toBe("Starting…");
    expect($(".containers .dev-bar .spin-dot")).not.toBeNull();
    await wait(120);
    w.mockDelay = {};
    expect($(".containers .dev-bar .dev-all").textContent).toBe("Stop");
    keyDown("q");
    await wait(50);
    expect(
      calls("containers_run")
        .slice(n)
        .map(([, a]) => a.action),
    ).toEqual(["start", "stop"]);
    expect(JSON.stringify(w.mockCalls)).not.toMatch(/"down"|"rm"/);
  });

  test("leaving the tab ends Anvil's log streams; back, they read from the start", async () => {
    const n = calls("container_logs_stop").length;
    $('.tabs [data-tab="overview"]').click();
    await wait(1100);
    expect(calls("container_logs_stop").length).toBe(n + 1);
    const m = calls("container_log_read").length;
    tabBtn().click();
    await wait(50);
    expect(calls("container_log_read").slice(m)[0][1].after).toBe(0);
  });

  test("⌘3 and the header's state open Containers when the app has no Dev", async () => {
    $('.tabs [data-tab="overview"]').click();
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "3", metaKey: true, bubbles: true }));
    await wait();
    expect(tabBtn().getAttribute("aria-selected")).toBe("true");
    $('.tabs [data-tab="overview"]').click();
    ($("#site .run-box .state") as HTMLElement).click();
    await wait();
    expect(tabBtn().getAttribute("aria-selected")).toBe("true");
    w.mockOv.stack = saved;
    $('.tabs [data-tab="overview"]').click();
    await select("shop");
    expect(tabBtn().hidden).toBe(true);
  });

  test("a compose file that can't be read: one muted Overview line, no tab, the message on a click", async () => {
    const error =
      "no port specified: :<empty>\ndocker-compose.yml needs variables that aren't set (DB_PASSWORD, DB_USERNAME, MEILISEARCH_PORT…). Copy .env.docker.example to .env.docker?";
    const shop = structuredClone(w.mockOv.shop);
    Object.assign(w.mockOv.shop, { containers: null, docker_error: error, compose_file: "docker-compose.yml" });
    try {
      await select("stack");
      await select("shop");
      const line = $$(".procs .dev-summary").find((b) => b.textContent!.startsWith("Containers"))!;
      expect(line.textContent).toBe("Containers" + "can’t read docker-compose.yml");
      expect(line.querySelector(".error-text")).toBeNull();
      expect(line.title).toBe(error);
      expect(tabBtn().hidden).toBe(true);
      const detail = $(".procs .dev-summary-detail");
      expect(detail.hidden).toBe(true);
      line.click();
      await wait();
      expect(tabBtn().getAttribute("aria-selected")).not.toBe("true");
      expect(detail.hidden).toBe(false);
      expect(detail.textContent).toBe(error);
      // Readable again: the tab is back.
      Object.assign(w.mockOv.shop, { containers: [], docker_error: null });
      await select("stack");
      await select("shop");
      expect(tabBtn().hidden).toBe(false);
    } finally {
      w.mockOv.shop = shop;
      await select("stack");
      await select("shop");
    }
  });

  test("stamped: docker's times sort as text; only lines after `since`", async () => {
    const { stamped } = await import("../src/containers");
    const raw = "2026-10-17T10:00:01.5Z a\n2026-10-17T10:00:01.123456789Z b\ncontinued\n2026-10-17T10:00:02Z c\n";
    expect(stamped(raw, null).map((l) => [l.at, l.text])).toEqual([
      ["2026-10-17T10:00:01.500000000", "a"],
      ["2026-10-17T10:00:01.123456789", "b"],
      ["2026-10-17T10:00:01.123456789", "continued"],
      ["2026-10-17T10:00:02.000000000", "c"],
    ]);
    expect(stamped(raw, "2026-10-17T10:00:01.5Z").map((l) => l.text)).toEqual(["c"]);
    expect(stamped("", "2026-10-17T10:00:01.5Z")).toEqual([]);
  });
});

describe("audit fixes", () => {
  test("a folder shows no lock, only its grey folder, nor 'Runs on', even with an old HTTPS flag", async () => {
    await select("notes");
    expect($('#list [data-name="notes"] .lock:not(.none)')).toBeNull();
    expect($(`#list [data-name="vuefinder"] .lock`)).not.toBeNull();
    expect($('[data-field="runtime"]')).toBeNull();
    expect($('[data-field="path"]').textContent).toBe("~/Sites/notes");
  });

  test("adding a folder or creating an app selects it without a toast", async () => {
    const open = w.__TAURI__.dialog.open;
    w.__TAURI__.dialog.open = async () => "/Users/x/Sites/notes";
    await select("shop");
    ($("#new-existing") as HTMLElement).click();
    await wait(80);
    w.__TAURI__.dialog.open = open;
    expect($('#list [data-name="notes"]').getAttribute("aria-current")).toBe("true");
    expect($(".toast")).toBeNull();

    await select("shop");
    ($("#new-name") as HTMLInputElement).value = "notes";
    const parent = $("#new-parent") as HTMLSelectElement;
    parent.append(new w.Option("~/Sites", "/Users/x/Sites"));
    parent.value = "/Users/x/Sites";
    $("#new-form").dispatchEvent(new Event("submit", { cancelable: true }));
    await wait(80);
    expect(calls("create_app")).toHaveLength(1);
    $("#new-done").click();
    await wait();
    expect($('#list [data-name="notes"]').getAttribute("aria-current")).toBe("true");
    expect($(".toast")).toBeNull();
  });

  test("Add Folder of Applications: a project folder is offered as one app, an empty one said", async () => {
    const open = w.__TAURI__.dialog.open;
    w.__TAURI__.dialog.open = async () => "/Users/x/Code/shop";
    const addFolder = async () => {
      $("#add").click();
      await wait();
      await w.mockMenu.find((i) => i.text === "Add Folder of Applications…").action();
      await wait();
    };
    w.mockParked = { project: "laravel", apps: 0 };
    await addFolder();
    expect($("#list .parked-note p").textContent).toBe("~/Code/shop is one application (Laravel), not a folder of them.");
    $$("#list .parked-note button")
      .find((b) => b.textContent === "Add It as One Application")!
      .click();
    await wait(80);
    expect(calls("link_folder").at(-1)![1]).toEqual({ path: "/Users/x/Code/shop" });
    expect($(".parked-note")).toBeNull();
    expect($('#list [data-name="notes"]').getAttribute("aria-current")).toBe("true");

    w.mockParked = { project: null, apps: 0 };
    await addFolder();
    expect($("#list .parked-note p").textContent).toContain("~/Code/shop has no applications yet");
    ($("#list .parked-close") as HTMLElement).click();
    expect($(".parked-note")).toBeNull();
    delete w.mockParked;
    w.__TAURI__.dialog.open = open;
  });

  test("PHP version: what it builds is said beside the select, a failed build under it", async () => {
    const { snap } = await import("../src/app");
    const saved = [snap.php_installed, snap.php_ext_built];
    for (const m of [w.mockSnap, snap]) {
      m.php_installed = ["8.4", "8.3"];
      m.php_ext_built = { "8.3": [] };
      m.sites.find((s) => s.name === "shop").ext = ["xdebug"];
    }
    await select("shop");
    $('.tabs [data-tab="settings"]').click();
    w.mockDelay = { set_php: 60 };
    w.mockSetPhpFail = "PHP 8.3 works without xdebug for now: couldn't get xdebug-3.4.5 from pecl.php.net";
    const php = $("#php") as HTMLSelectElement;
    php.value = "8.3";
    php.dispatchEvent(new Event("change"));
    await wait(20);
    expect($(".php-busy").textContent).toBe("Building xdebug…");
    expect(($("#php") as HTMLSelectElement).disabled).toBe(true);
    await wait(120);
    expect($(".php-busy")).toBeNull();
    expect($(".php-error").textContent).toBe(w.mockSetPhpFail);
    expect($("#error").hidden).toBe(true);
    delete w.mockSetPhpFail;
    w.mockDelay = {};
    for (const m of [w.mockSnap, snap]) {
      [m.php_installed, m.php_ext_built] = saved;
      m.sites.find((s) => s.name === "shop").ext = [];
    }
    $('.tabs [data-tab="overview"]').click();
  });

  test("New application: a folder can be chosen with nothing parked", async () => {
    const { snap } = await import("../src/app");
    const { openNewApp } = await import("../src/newapp");
    const open = w.__TAURI__.dialog.open;
    const { paths, sites } = snap;
    snap.paths = [];
    snap.sites = [];
    w.__TAURI__.dialog.open = async () => "/Users/x/Elsewhere";
    openNewApp();
    const parent = $("#new-parent") as HTMLSelectElement;
    // Choose a folder… isn't the one selected, so picking it is a change.
    expect(parent.value).toBe("");
    expect(parent.options[parent.options.length - 1].value).toBe("choose");
    parent.value = "choose";
    parent.dispatchEvent(new Event("change"));
    await wait(20);
    expect(parent.value).toBe("/Users/x/Elsewhere");
    expect([...parent.options].map((o) => o.value)).not.toContain("");
    w.__TAURI__.dialog.open = open;
    snap.paths = paths;
    snap.sites = sites;
    ($("#new-app") as HTMLDialogElement).close();
  });

  test("Copy says Copied on the button for a moment, not in a toast", async () => {
    $('.modes [data-mode="services"]').click();
    await wait(80);
    const copy = $(".svc-copy");
    copy.click();
    await wait();
    expect(copy.textContent).toBe("Copied");
    expect($(".toast")).toBeNull();
    await wait(1600);
    // The copy icon again.
    expect(copy.textContent!.trim()).toBe("");
    expect(copy.querySelector("svg")).not.toBeNull();
    $('.modes [data-mode="apps"]').click();
  });
});

describe("service page", () => {
  const show = async (id: string) => {
    $('.modes [data-mode="services"]').click();
    await wait(40);
    $(`#list [data-name="${id}"]`).click();
    await wait(80);
  };
  const log = () => $('[data-svc-open="log"]');

  test("no Log section; the log icon opens Processes on that service, or says Not running", async () => {
    await show("cache");
    expect($$("#site .res-sub").map((h) => h.textContent)).not.toContain("Log");
    expect($("#site .svc-log")).toBeNull();
    expect(log().title).toBe("Show the log in Processes");
    log().click();
    await wait();
    expect(calls("show_processes").at(-1)![1]).toEqual({ select: "service:cache" });
    const n = calls("show_processes").length;
    await show("postgres");
    expect(log().title).toBe("Not running");
    expect(log().getAttribute("aria-disabled")).toBe("true");
    log().click();
    await wait();
    expect(calls("show_processes")).toHaveLength(n);
  });

  test("a service row's right-click menu: Stop or Start, its client, data, log, Remove last", async () => {
    await show("cache");
    const texts = () => (w.mockMenu as any[]).map((i) => i.text ?? i.item);
    $('#list [data-name="mysql"]').dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await wait();
    expect($('#list [data-name="mysql"]').getAttribute("aria-current")).toBe("true");
    expect(texts()).toEqual([
      "Stop",
      "Separator",
      "Open in TablePlus",
      "Show Data in Finder",
      "Show Log in Processes",
      "Separator",
      "Remove Service…",
    ]);
    (w.mockMenu as any[]).find((i) => i.text === "Show Log in Processes").action();
    await wait();
    expect(calls("show_processes").at(-1)![1]).toEqual({ select: "service:mysql" });
    // A stopped one: Start, and no log.
    $('#list [data-name="postgres"]').dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await wait();
    expect(texts()).toEqual(["Start", "Separator", "Show Data in Finder", "Separator", "Remove Service…"]);
    (w.mockMenu as any[])[0].action();
    await wait();
    expect(calls("service_run").at(-1)![1]).toEqual({ id: "postgres", action: "start" });
  });

  test("Redis: keys and memory on one line that opens to the keys", async () => {
    await show("cache");
    const sum = $(".svc-summary");
    expect(sum.textContent).toBe("12 keys · 911 KB memory");
    expect($$(".svc-contents .proc").length).toBe(1);
    sum.click();
    await wait();
    expect($(".svc-summary").getAttribute("aria-expanded")).toBe("true");
    expect($$(".svc-sub .proc-name").map((n) => n.textContent)).toEqual(["db0"]);
    expect($(".svc-sub .danger").textContent).toBe("Flush All…");
    expect($(".svc-contents-head").hidden).toBe(true);
  });

  test("MySQL: Databases kept; one client is an icon in the header, several a row", async () => {
    await show("mysql");
    expect($(".svc-contents-head").hidden).toBe(false);
    expect($$(".svc-contents .proc-name").map((n) => n.textContent)).toContain("shop");
    const client = $('[data-svc-open="client"]');
    expect([client.hidden, client.title]).toEqual([false, "Open in TablePlus"]);
    expect($(".svc-clients").hidden).toBe(true);
    client.click();
    await wait();
    expect(calls("open_in_client").at(-1)![1]).toEqual({ id: "mysql", client: "TablePlus", database: null });
    const svc = w.mockServices.find((s) => s.id === "mysql");
    svc.clients = ["TablePlus", "Sequel Ace"];
    w.loadServices();
    await show("cache");
    await show("mysql");
    expect($('[data-svc-open="client"]').hidden).toBe(true);
    expect($$(".svc-clients button").map((b) => b.textContent)).toEqual(["TablePlus", "Sequel Ace"]);
    svc.clients = ["TablePlus"];
    w.loadServices();
    expect($(".svc-remove").textContent).toBe("Remove Service…");
    $('.modes [data-mode="apps"]').click();
    await wait(40);
  });

  test("Applications says it changes no .env; Write to .env asks with the keys and says how it went", async () => {
    const elsewhere = w.mockSite("notes2", "laravel", { linked: true, path: "/Users/x/Elsewhere/notes2", parked_in: null });
    w.mockSnap.sites.push(elsewhere);
    const { call } = await import("../src/app");
    try {
      await call("state");
      await show("mysql");
      expect($(".svc-apps-hint").textContent).toContain("Their .env isn’t changed");
      // A linked app can be picked; an alias (admin.shop: shop's folder) can't.
      const picks = $$("#site .svc-apps select option").map((o) => o.textContent);
      expect(picks).toContain("notes2.test");
      expect(picks).not.toContain("admin.shop.test");
      const write = $(".svc-write button");
      expect(write.textContent).toBe("Write to shop’s .env…");
      write.click();
      await wait();
      expect($(".pop-confirm p").textContent).toBe(
        "Set DB_CONNECTION, DB_HOST, DB_PORT, DB_USERNAME, DB_PASSWORD in shop’s .env? Its other lines stay.",
      );
      expect(calls("service_write_env")).toEqual([]);
      $$(".pop-confirm button")
        .find((b) => b.textContent === "Write")!
        .click();
      await wait();
      expect(calls("service_write_env").at(-1)![1]).toEqual({ id: "mysql", site: "shop", database: null });
      expect($(".svc-write .muted").textContent).toBe("Written. Running processes get it when they restart.");
    } finally {
      w.mockSnap.sites.pop();
      await call("state");
    }
  });

  test("Drop and Remove ask in a bubble, no dialog; Remove's two answers are labelled", async () => {
    await show("mysql");
    const drop = $$(".svc-contents .svc-row").find((r) => r.querySelector(".proc-name")?.textContent === "shop")!;
    (drop.querySelector(".danger") as HTMLElement).click();
    await wait();
    expect($(".pop-confirm").textContent).toContain("Drop shop?");
    $$(".pop-confirm button")
      .find((b) => b.textContent === "Drop")!
      .click();
    await wait();
    expect(calls("service_data").at(-1)![1]).toEqual({ id: "mysql", action: "drop", name: "shop" });
    $(".svc-remove").click();
    await wait();
    expect($(".pop-confirm p").textContent).toContain("shop uses it; its .env stays as it is.");
    expect($$(".pop-confirm button").map((b) => b.textContent)).toEqual(["Cancel", "Remove and Delete Data", "Remove, Keep Data"]);
    expect(document.activeElement?.textContent).toBe("Remove, Keep Data");
    $$(".pop-confirm button")
      .find((b) => b.textContent === "Remove and Delete Data")!
      .click();
    await wait();
    expect(calls("remove_service").at(-1)![1]).toEqual({ id: "mysql", deleteData: true });
    expect($(".pop-confirm")).toBeNull();
    $('.modes [data-mode="apps"]').click();
    await wait(40);
  });
});

describe("logs", () => {
  test("a ×24 exception opens to one summary line, its class and file, and three frames", async () => {
    await select("shop");
    $('.tabs [data-tab="logs"]').click();
    await wait(80);
    const first = $$(".logs-list .log")[0];
    expect(first.querySelector(".log-count")?.textContent).toBe("×24");
    (first.querySelector(".log-head") as HTMLElement).click();
    expect(first.querySelector(".log-meta > span")?.textContent).toMatch(
      /^24 times · first \d+ \w{3} \d\d:\d\d · last (\d+ \w{3} )?\d\d:\d\d · local show all times$/,
    );
    expect(first.querySelector(".log-times")).toBeNull();
    (first.querySelector(".log-link") as HTMLElement).click();
    expect(first.querySelectorAll(".log-times li")).toHaveLength(24);
    expect(first.querySelectorAll(".log-copy")).toHaveLength(1);
    const exc = first.querySelector(".log-exc")!;
    expect(exc.querySelector(".log-exc-cls")?.textContent).toBe("BindingResolutionException");
    expect((exc.querySelector(".log-exc-cls") as HTMLElement).title).toBe("Illuminate\\Contracts\\Container\\BindingResolutionException");
    expect(exc.querySelector(".log-file")?.textContent).toBe("Illuminate/Container/Container.php:1127");
    // No raw JSON, no repeated message: the row shows it whole.
    expect(first.querySelector(".log-json")).toBeNull();
    expect(first.querySelector("pre.log-text")).toBeNull();
    const trace = first.querySelector(".log-trace")!;
    expect(trace.querySelectorAll("li:not(.fold)")).toHaveLength(3);
    const frame = trace.querySelector("li")!;
    expect(frame.querySelector(".log-no")?.textContent).toBe("#0");
    expect(frame.querySelector(".log-file")?.textContent).toBe("Illuminate/Container/Container.php:1127");
    expect(frame.querySelector(".log-fn")?.textContent).toBe("Illuminate\\Container\\Container->build()");
    expect(first.querySelector(".log-vendor")?.textContent).toBe("+57 more");
  });

  test("a row with nothing more to show doesn't open", async () => {
    const warn = $$(".logs-list .log")[1];
    expect(warn.classList.contains("flat")).toBe(true);
    expect(warn.querySelector("button.log-head")).toBeNull();
    (warn.querySelector(".log-head") as HTMLElement).click();
    expect(warn.querySelector(".log-detail")).toBeNull();
  });

  test("with app frames, vendor frames fold and app frames open in the editor", async () => {
    const row = $$(".logs-list .log")[2];
    expect(row.querySelector(".log-count")?.textContent).toBe("×4");
    expect(row.querySelector(".log-level")?.textContent).toBe("error");
    (row.querySelector(".log-head") as HTMLElement).click();
    const trace = row.querySelector(".log-trace")!;
    expect(trace.classList.contains("folded")).toBe(true);
    expect([...trace.querySelectorAll(".own .log-file")].map((f) => f.textContent)).toEqual([
      "app/Http/Controllers/CheckoutController.php:42",
      "app/Http/Middleware/EnsureCartNotEmpty.php:23",
      "app/Providers/RouteServiceProvider.php:51",
    ]);
    const more = row.querySelector(".log-vendor") as HTMLElement;
    expect(more.textContent).toBe("+56 vendor frames");
    more.click();
    expect(trace.classList.contains("folded")).toBe(false);
    (trace.querySelector("button.log-file") as HTMLElement).click();
    expect(calls("open_in_editor").at(-1)![1]).toEqual({ site: "shop", path: "app/Http/Controllers/CheckoutController.php", line: 42 });
  });

  test("a JSX tree in a message is highlighted, as text only, with React's diff lines marked", async () => {
    const { messageView } = await import("../src/logs");
    const msg = [
      "Hydration failed. This can happen if:",
      "- A server/client branch",
      "  <Table>",
      '    <div className="wrap" style={{...}}>',
      '+     <td className="cell">',
      '-     <td className="old">',
      '>      <span title="%s" data-x="<img src=x onerror=alert(1)>">{"Total"}</span>',
      "Expected %s.",
    ].join("\n");
    const pre = messageView(msg);
    expect(pre.textContent).toBe(msg.replace(/\n/g, ""));
    expect(pre.querySelector("img")).toBeNull();
    expect(pre.innerHTML).toContain("&lt;img src=x onerror=alert(1)&gt;");
    const lines = [...pre.children] as HTMLElement[];
    expect(lines.map((l) => l.className)).toEqual([
      "hl-line",
      "hl-line", // a bullet of the prose, not a diff line
      "hl-line",
      "hl-line",
      "hl-line hl-add",
      "hl-line hl-del",
      "hl-line hl-here",
      "hl-line",
    ]);
    const of = (cls: string) => [...pre.querySelectorAll(`.${cls}`)].map((s) => s.textContent);
    expect(of("hl-tag")).toEqual(["Table", "div", "td", "td", "span", "span"]);
    expect(of("hl-attr")).toEqual(["className", "style", "className", "className", "title", "data-x"]);
    expect(of("hl-str")).toEqual(['"wrap"', '"cell"', '"old"', '"%s"', '"<img src=x onerror=alert(1)>"']);
    expect(of("hl-mute")).toEqual(["{{...}}", "%s", '{"Total"}', "%s"]);
    expect(lines[6].querySelector(".hl-mark")!.textContent).toBe(">");
    // Plain prose: only placeholders.
    const plain = messageView('Expected "a" to be 3 < 4, got %d times');
    expect(plain.querySelectorAll("span span").length).toBe(1);
    expect(plain.querySelector(".hl-mute")!.textContent).toBe("%d");
  });

  test("times summary and exception parsing", async () => {
    const { timesSummary, parseException } = await import("../src/logs");
    expect(timesSummary(["2026-10-05 16:24:11", "2026-10-05 15:00:00", "2026-10-05 13:23:39"])).toBe(
      "3 times · first 5 Oct 13:23 · last 16:24",
    );
    expect(timesSummary(["2026-10-06 09:01:00", "2026-10-05 13:23:39"])).toBe("2 times · first 5 Oct 13:23 · last 6 Oct 09:01");
    expect(timesSummary(["2026-10-05 16:24:11"])).toBe("5 Oct 16:24:11");
    expect(
      parseException(
        "[object] (Illuminate\\Contracts\\Container\\BindingResolutionException(code: 0): Target class [request] does not exist. at /x/vendor/a/B.php:1127)",
      ),
    ).toEqual({
      cls: "Illuminate\\Contracts\\Container\\BindingResolutionException",
      message: "Target class [request] does not exist.",
      file: "/x/vendor/a/B.php",
      line: 1127,
    });
    expect(parseException("just text")).toBeNull();
  });

  test("only adjacent repeats group", async () => {
    const { groupEntries } = await import("../src/logs");
    const e = (level: string, message: string) => ({ level, message });
    const groups = groupEntries([e("error", "a"), e("error", "a"), e("info", "a"), e("error", "a")]);
    expect(groups.map((g) => g.length)).toEqual([2, 1, 1]);
  });

  test("Errors asks the backend for errors only", async () => {
    ($$(".logs-bar .seg button").find((b) => b.textContent === "Errors") as HTMLElement).click();
    await wait(80);
    expect(calls("log_search").at(-1)![1]).toMatchObject({ site: "shop", level: "error" });
  });

  test("a plain source is terminal text: colored lines, no rows; search and Errors filter lines", async () => {
    ($$(".logs-bar .seg button").find((b) => b.textContent === "All") as HTMLElement).click();
    ($('.logs-files .seg [data-id="once:build"]') as HTMLElement).click();
    await wait(80);
    expect(calls("log_text").at(-1)![1]).toMatchObject({ site: "shop", source: "once:build" });
    const term = $(".logs-term");
    expect(term.hidden).toBe(false);
    expect(($(".logs-list") as HTMLElement).hidden).toBe(true);
    expect(term.textContent).toContain("✓ built in 3.41s");
    expect(term.querySelector(".ansi2")).not.toBeNull();
    expect(term.querySelector(".log-copy, .log-head")).toBeNull();
    const search = $(".logs-search") as HTMLInputElement;
    search.value = "err!";
    search.dispatchEvent(new Event("input"));
    await wait(350);
    expect(term.textContent!.split("\n")).toHaveLength(3);
    expect([...term.querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["ERR!", "ERR!", "ERR!"]);
    search.value = "";
    search.dispatchEvent(new Event("input"));
    await wait(350);
    ($$(".logs-bar .seg button").find((b) => b.textContent === "Errors") as HTMLElement).click();
    await wait(30);
    expect(term.textContent!.split("\n")).toHaveLength(5);
    expect(calls("log_search").at(-1)![1]).not.toMatchObject({ source: "once:build" });
    ($$(".logs-bar .seg button").find((b) => b.textContent === "All") as HTMLElement).click();
    ($('.logs-files .seg [data-id="laravel.log"]') as HTMLElement).click();
    await wait(80);
  });
});

describe("requests", () => {
  const rows = () => $$(".req-list .log");
  const cell = (r: HTMLElement, c: string) => r.querySelector(c)?.textContent;

  test("the tab comes after Logs, and not for an app Caddy doesn't serve", async () => {
    await select("stack");
    expect($('.tabs [data-tab="requests"]').hidden).toBe(true);
    await select("shop");
    const tabs = $$(".tabs [role=tab]").map((t) => t.dataset.tab);
    expect(tabs.indexOf("requests")).toBe(tabs.indexOf("logs") + 1);
    expect($('.tabs [data-tab="requests"]').hidden).toBe(false);
  });

  test("lists the newest first: method, status, path, duration and size; assets hidden", async () => {
    $('.tabs [data-tab="requests"]').click();
    await wait(80);
    expect(calls("requests_read").at(-1)![1]).toEqual({
      site: "shop",
      filter: { status: null, method: null, query: "", assets: false },
      before: null,
    });
    const [post, docs, home] = rows();
    expect(cell(post, ".req-method")).toBe("POST");
    expect(cell(post, ".req-status")).toBe("419");
    expect(post.querySelector(".req-status")!.classList.contains("st-4")).toBe(true);
    expect(cell(post, ".log-msg")).toBe("/login?next=%2F");
    expect(cell(post, ".req-dur")).toBe("0.4 ms");
    expect(cell(post, ".req-size")).toBe("0 B");
    expect(cell(home, ".req-dur")).toBe("12 ms");
    expect(cell(docs, ".req-dur")).toBe("1.25 s");
    // Another of the app's hosts (a server row) shows its host; the app's own doesn't.
    expect(cell(docs, ".log-msg")).toBe("shop--docs.test/docs/intro");
    expect(home.querySelector(".req-host")).toBeNull();
  });

  test("a row opens to its headers, the redacted ones said so, the host first", async () => {
    const home = rows()[2];
    (home.querySelector(".log-head") as HTMLElement).click();
    const detail = home.querySelector(".log-detail")!;
    expect(detail.querySelector(".log-meta")?.textContent).toContain("HTTP/2.0 · TLS · from 127.0.0.1");
    const req = [...detail.querySelectorAll(".req-headers")[0].querySelectorAll("div")].map((d) => d.textContent);
    expect(req).toEqual(["Hostshop.test", "Accepttext/html", "Cookieredacted"]);
    expect(detail.querySelectorAll(".req-headers")[1].textContent).toContain("Set-Cookieredacted");
    expect(detail.querySelector(".req-headers .redacted")).not.toBeNull();
  });

  test("filters go to the backend; Clear puts them back", async () => {
    ($('.req-bar .seg [data-id="4xx"]') as HTMLElement).click();
    await wait(30);
    expect(calls("requests_read").at(-1)![1].filter.status).toBe("4xx");
    const method = $(".req-method-pick") as HTMLSelectElement;
    method.value = "POST";
    method.dispatchEvent(new Event("change"));
    await wait(30);
    expect(calls("requests_read").at(-1)![1].filter.method).toBe("POST");
    const assets = $(".req-assets input") as HTMLInputElement;
    assets.click();
    await wait(30);
    expect(calls("requests_read").at(-1)![1].filter.assets).toBe(true);
    const search = $(".req-bar .logs-search") as HTMLInputElement;
    search.value = "login";
    search.dispatchEvent(new Event("input"));
    await wait(350);
    expect(calls("requests_read").at(-1)![1].filter.query).toBe("login");
    const clear = $(".req-clear");
    expect(clear.hidden).toBe(false);
    clear.click();
    await wait(30);
    expect(calls("requests_read").at(-1)![1].filter).toEqual({ status: null, method: null, query: "", assets: false });
    expect(clear.hidden).toBe(true);
    expect(search.value).toBe("");
    expect(assets.checked).toBe(false);
    expect($('.req-bar .seg [data-id="all"]').getAttribute("aria-pressed")).toBe("true");
  });

  test("Load older asks for what comes before the page", async () => {
    $(".req-more").click();
    await wait(30);
    expect(calls("requests_read").at(-1)![1].before).toBe(400);
    expect(cell(rows().at(-1)!, ".log-msg")).toBe("/older");
    expect($(".req-more").hidden).toBe(true);
  });

  test("new requests come in on top while the tab is shown, not while another is", async () => {
    const req = await import("../src/requests");
    req.poll(false); // ticks by hand
    w.mockRequestsTail = [w.mockRequest({ uri: "/new" })];
    await req.tick();
    expect(cell(rows()[0], ".log-msg")).toBe("/new");
    expect(calls("requests_tail").at(-1)![1]).toEqual({
      site: "shop",
      filter: { status: null, method: null, query: "", assets: false },
      from: 1200,
    });
    $('.tabs [data-tab="overview"]').click();
    const n = calls("requests_tail").length;
    await req.tick();
    expect(calls("requests_tail").length).toBe(n);
  });
});

describe("polls and races", () => {
  const entry = (message: string) => ({ time: "2026-10-17 10:00:00", env: "local", level: "error", message, context: null, trace: [] });

  test("a process's output opened directly is followed, new lines added to what is drawn", async () => {
    document.hasFocus = () => true;
    const logs = await import("../src/logs");
    await select("shop");
    logs.showLog("shop", "once:build");
    await wait(80);
    logs.follow(false); // ticks by hand below
    const term = $(".logs-term");
    const base = (await w.__TAURI__.core.invoke("log_text", { site: "shop", source: "once:build", from: 0 })).text;
    const first = term.firstElementChild;
    const n = calls("log_text").length;
    w.mockLogs = { "once:build": `${base}next http://shop.test/a\n` };
    await logs.followTick();
    w.mockLogs = { "once:build": `${base}next http://shop.test/a\nand more\n` };
    await logs.followTick();
    expect(calls("log_text").length).toBe(n + 2);
    expect(term.firstElementChild).toBe(first); // not drawn again
    // A block per tick: the last two are what each tick added.
    expect([...term.children].slice(-2).map((b) => b.textContent)).toEqual(["next http://shop.test/a", "and more"]);
    expect(term.textContent!.split("✓ built in 3.41s")).toHaveLength(2);
    expect(term.querySelector(".term-link")?.textContent).toBe("http://shop.test/a");
    delete w.mockLogs;
  });

  test("log_tail: one at a time, and an answer for a view changed meanwhile is dropped", async () => {
    const logs = await import("../src/logs");
    logs.showLog("shop", "laravel.log");
    await wait(80);
    logs.follow(false);
    const n = calls("log_tail").length;
    w.mockDelay = { log_tail: 60 };
    w.mockTail = [entry("tail once")];
    await Promise.all([logs.followTick(), logs.followTick(), logs.followTick()]);
    expect(calls("log_tail").length).toBe(n + 1);
    const rows = () => $$(".logs-list .log-msg").filter((m) => m.textContent === "tail once");
    expect(rows()).toHaveLength(1);
    expect(rows()[0].closest(".log")!.querySelector(".log-count")).toBeNull();
    w.mockTail = [entry("tail late")];
    const tick = logs.followTick();
    ($$(".logs-bar .seg button").find((b) => b.textContent === "Errors") as HTMLElement).click();
    await tick;
    await wait(80);
    w.mockDelay = {};
    expect($$(".logs-list .log-msg").filter((m) => m.textContent === "tail late")).toHaveLength(0);
    ($$(".logs-bar .seg button").find((b) => b.textContent === "All") as HTMLElement).click();
    await wait(80);
  });

  test("following keeps at most MAX_ENTRIES entries; Load older starts again from the newest page", async () => {
    const logs = await import("../src/logs");
    w.mockTail = Array.from({ length: logs.MAX_ENTRIES + 300 }, (_, i) => entry(`flood ${i}`));
    await logs.followTick();
    const shown = $$(".logs-list .log").reduce((n, li: any) => n + li.entries.length, 0);
    expect(shown).toBeLessThanOrEqual(logs.MAX_ENTRIES);
    expect(logs.lv.entries.length).toBeLessThanOrEqual(logs.MAX_ENTRIES);
    expect(logs.lv.shown).toBe(shown);
    expect($(".logs-more").hidden).toBe(false);
    const reads = calls("log_read").length;
    $(".logs-more").click();
    await wait(80);
    expect(calls("log_read").length).toBe(reads + 1);
    expect(logs.lv.trimmed).toBe(false);
  });

  test("Dev drops the lines over its cap in one removal", async () => {
    const { trimLines } = await import("../src/stream");
    const out = document.createElement("div");
    for (let i = 0; i < 600; i++) out.append(Object.assign(document.createElement("div"), { textContent: `l${i}` }));
    trimLines(out, 500);
    expect(out.childElementCount).toBe(500);
    expect(out.firstElementChild!.textContent).toBe("l100");
    trimLines(out, 500);
    expect(out.childElementCount).toBe(500);
  });

  test("a processes poll asked before an action doesn't undo the action's answer", async () => {
    const p = await import("../src/processes");
    const row = () => p.procCache.get("vuefinder")!.rows.find((r) => r.kind !== "site")!;
    const ov = w.mockOv.vuefinder;
    const r = ov.rows.find((x) => x.kind !== "site");
    const was = r.running;
    w.mockDelay = { processes: 60 };
    const poll = p.loadProcs("vuefinder");
    r.running = !was; // what the action does
    await p.loadProcs("vuefinder", "process_run", { name: r.id, action: was ? "stop" : "start" });
    r.running = was; // what the poll read before it
    await poll;
    w.mockDelay = {};
    expect(row().running).toBe(!was);
    await p.loadProcs("vuefinder"); // a poll asked after it does count
    expect(row().running).toBe(was);
  });

  test("a state poll asked before an action doesn't undo the action's snapshot", async () => {
    const app = await import("../src/app");
    const was = w.mockSnap.running;
    w.mockDelay = { state: 60 };
    const poll = app.call("state");
    w.mockSnap.running = !was;
    await app.call("app_run", { site: "shop", action: "start" });
    w.mockSnap.running = was;
    await poll;
    w.mockDelay = {};
    expect(app.snap.running).toBe(!was);
    await app.call("state");
    expect(app.snap.running).toBe(was);
  });

  test("the error banner stays until its owner clears it, or it is dismissed", async () => {
    const app = await import("../src/app");
    w.mockSnap.error = "Caddy failed to start";
    w.mockDelay = { services: 50 };
    const services = w.loadServices(); // answers after the state
    await app.call("state");
    await services;
    w.mockDelay = {};
    expect($("#error").hidden).toBe(false);
    expect($("#error-text").textContent).toBe("Caddy failed to start");
    w.mockSnap.error = null;
    await app.call("state");
    expect($("#error").hidden).toBe(true);
    app.showError("Push failed");
    await w.loadServices();
    await app.call("state");
    expect($("#error-text").textContent).toBe("Push failed");
    $("#error-close").click();
    expect($("#error").hidden).toBe(true);
  });
});

describe("packages", () => {
  const pins = () => $$(".pkg-pin").map((c) => `${c.firstChild!.textContent} ${c.querySelector("b")?.textContent ?? ""}`);

  test("the key packages as badges: the defaults, one off with ×, Reset back", async () => {
    $('.tabs [data-tab="packages"]').click();
    await wait(80);
    expect(pins()).toEqual(["Laravel 11.36.1", "Livewire 3.5.18", "Tailwind 3.4.17", "Vite 6.0.6", "PHP ^8.2", "Node >=20"]);
    // No list, no search, no Outdated · All: the badges and the two checks.
    expect($$(".pkg-list, .pkg-search, .pkg-tools")).toHaveLength(0);
    expect($$(".pkg-pin-reset")).toHaveLength(0);
    ($('.pkg-pin [aria-label="Unpin Livewire"]') as HTMLElement).click();
    await wait(80);
    expect(calls("set_packages_pinned").at(-1)![1]).toEqual({
      site: "shop",
      pins: ["laravel/framework", "tailwindcss", "vite", "PHP", "Node"],
    });
    expect(pins()).toHaveLength(5);
    $(".pkg-pin-reset").click();
    await wait(80);
    expect(calls("set_packages_pinned").at(-1)![1]).toEqual({ site: "shop", pins: null });
    expect(pins()).toHaveLength(6);
    expect($$(".pkg-pin-reset")).toHaveLength(0);
  });

  test("+ searches every installed package, the lock file's too, and pins the pick", async () => {
    $(".pkg-pin-add").click();
    const q = $(".pkg-pin-pop input") as HTMLInputElement;
    expect(document.activeElement).toBe(q);
    expect(q.getAttribute("role")).toBe("combobox");
    q.value = "symf";
    q.dispatchEvent(new Event("input"));
    expect($$(".pkg-pin-pop [role=option] .pkg-opt-name").map((o) => o.textContent)).toEqual(["symfony/console"]);
    // Pinned ones aren't offered again.
    q.value = "laravel/fr";
    q.dispatchEvent(new Event("input"));
    expect($$(".pkg-pin-pop [role=option]")).toHaveLength(0);
    q.value = "guzzle";
    q.dispatchEvent(new Event("input"));
    q.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await wait(80);
    expect(calls("set_packages_pinned").at(-1)![1].pins.at(-1)).toBe("guzzlehttp/guzzle");
    expect($$(".pkg-pin-pop")).toHaveLength(0);
    expect(pins().at(-1)).toBe("guzzlehttp/guzzle 7.9.2");
    expect($$(".pkg-pin-reset")).toHaveLength(1);
    $(".pkg-pin-reset").click();
    await wait(80);
  });

  test("nothing below the checks until one runs; Check outdated: the notice, then a tab per manager", async () => {
    expect(calls("package_updates")).toHaveLength(0);
    expect($$(".pkg-checks button").map((b) => b.textContent)).toEqual(["Outdated packages", "Security advisories"]);
    expect($$(".pkg-notice, .pkg-results, .pkg-checked")).toHaveLength(0);
    $(".pkg-check-outdated").click();
    expect($(".pkg-check-outdated").textContent).toBe("Checking outdated…");
    await wait(80);
    expect(calls("package_updates")).toHaveLength(1);
    expect($(".pkg-notice.warn").textContent).toBe("7 updates available");
    expect($$(".pkg-notice.bad")).toHaveLength(0);
    expect($(".pkg-checked").textContent).toBe("Updates checked just now");
    expect(JSON.parse(localStorage.getItem("pkgUpdates:shop")!).list).toHaveLength(7);
    expect($$(".pkg-ecos button").map((b) => b.textContent)).toEqual(["Composer4", "pnpm3"]);
    expect($$(".pkg-ecos .pkg-count.bad")).toHaveLength(0);
    const rows = $$(".pkg-results .row");
    expect(rows).toHaveLength(4);
    const fw = rows.find((r) => r.querySelector("a")?.textContent === "laravel/framework")!;
    expect(fw.querySelector(".pkg-ver")?.textContent).toBe("11.36.1→12.4.0major");
    ($('.pkg-ecos [data-id="npm"]') as HTMLElement).click();
    expect($$(".pkg-results .row")).toHaveLength(3);
    $('.tabs [data-tab="logs"]').click();
    $('.tabs [data-tab="packages"]').click();
    await wait(80);
    expect(calls("package_updates")).toHaveLength(1);
  });

  test("a saved check comes back after a restart", async () => {
    const { pkgUpdates, drawPackages } = await import("../src/packages");
    pkgUpdates.clear();
    drawPackages("shop");
    expect($(".pkg-notice.warn").textContent).toBe("7 updates available");
    expect(calls("package_updates")).toHaveLength(1);
  });

  test("Check security audits each manager: advisories in red with severity, CVE and range; a failure inline", async () => {
    $(".pkg-check-security").click();
    expect($(".pkg-check-security").textContent).toBe("Checking security…");
    await wait(80);
    expect(calls("package_audit").map(([, a]) => a.ecosystem)).toEqual(["composer", "npm"]);
    expect($(".pkg-notice.bad").textContent).toBe("1 security advisory");
    expect($(".pkg-notice.warn").textContent).toBe("7 updates available");
    expect($(".pkg-errors").textContent).toBe("pnpm audit failed: request to registry timed out");
    // guzzle has an advisory and no update: Composer counts 5, in red.
    const composer = $('.pkg-ecos [data-id="composer"]');
    expect(composer.textContent).toBe("Composer5");
    expect(composer.querySelector(".pkg-count")!.classList.contains("bad")).toBe(true);
    composer.click();
    const adv = $(".pkg-advisory");
    expect(adv.querySelector(".pkg-sev")!.textContent).toBe("high");
    expect(adv.querySelector("a")!.textContent).toBe("Noncanonical host can bypass host-based checks");
    expect(adv.textContent).toContain("guzzlehttp/guzzle 7.9.2");
    expect(adv.textContent).toContain("CVE-2026-69246");
    expect(adv.textContent).toContain(">=7.0.0,<7.9.3");
    expect(adv.textContent).toContain("fixed in 7.9.3");
    (adv.querySelector("a") as HTMLElement).click();
    await wait(30);
    expect(calls("open_url").at(-1)![1].url).toBe("https://github.com/advisories/GHSA-v5mv-p594-2x33");
  });
});

describe("new application", () => {
  const openSheet = async () => {
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "n", metaKey: true, bubbles: true }));
    await wait(60);
  };
  const kinds = () => $$("#new-kinds .kind-item");
  const chosenKind = () => $('#new-kinds [aria-selected="true"]')?.dataset.kind;
  const pick = async (kind: string) => {
    kinds()
      .find((k) => k.dataset.kind === kind)!
      .click();
    await wait();
  };
  const key = async (k: string) => {
    $("#new-kinds").dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
    await wait(40);
  };
  const fill = (name: string) => {
    const input = $("#new-name") as HTMLInputElement;
    input.value = name;
    input.dispatchEvent(new Event("input"));
  };
  const submit = () => $("#new-form").dispatchEvent(new Event("submit", { cancelable: true }));
  const steps = () => $$("#new-steps .new-step").map((s) => `${s.textContent}:${s.className.split(" ")[1]}`);

  test("templates are grouped by language, each with a monochrome glyph; a click only chooses", async () => {
    localStorage.setItem("newKind", "laravel");
    await openSheet();
    expect($("#new-app").hasAttribute("open")).toBe(true);
    expect($$("#new-kinds .kind-head").map((h) => h.textContent)).toEqual(["PHP", "JavaScript", "Python", "Other"]);
    expect(kinds()).toHaveLength(15);
    for (const k of kinds()) {
      expect(k.querySelector(".kind-icon svg")).not.toBeNull();
      expect(k.querySelector(".kind-icon")!.innerHTML).toContain("currentColor");
      expect(k.querySelector(".kind-about")!.textContent).not.toBe("");
    }
    expect(chosenKind()).toBe("laravel");
    const created = calls("create_app").length;
    await pick("vite");
    expect(chosenKind()).toBe("vite");
    expect(calls("create_app")).toHaveLength(created);
    $("#new-cancel").click();
  });

  test("arrows move the choice, Enter creates", async () => {
    await openSheet();
    await pick("laravel");
    await key("ArrowDown");
    expect(chosenKind()).toBe("statamic");
    await key("ArrowRight");
    expect(chosenKind()).toBe("symfony");
    await key("ArrowUp");
    await key("ArrowLeft");
    expect(chosenKind()).toBe("laravel");
    fill("my-app");
    await key("Enter");
    await wait(80);
    expect(calls("create_app").at(-1)![1]).toMatchObject({
      kind: "laravel",
      name: "my-app",
      parent: "/Users/x/Sites",
      columns: 100,
      lines: 30,
    });
    $("#new-done").click();
  });

  test("the search has focus, filters templates by name and line, hides empty groups, and Esc clears it before closing", async () => {
    await openSheet();
    const search = $("#new-search") as HTMLInputElement;
    expect(document.activeElement).toBe(search);
    const type = (q: string) => {
      search.value = q;
      search.dispatchEvent(new Event("input"));
    };
    const shown = () =>
      kinds()
        .filter((k) => !k.hidden)
        .map((k) => k.dataset.kind);
    const groups = () => $$("#new-kinds .kind-group:not([hidden]) .kind-head").map((h) => h.textContent);
    type("server rendering");
    expect(shown()).toEqual(["next", "nuxt"]);
    expect(groups()).toEqual(["JavaScript"]);
    expect(chosenKind()).toBe("next");
    search.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(chosenKind()).toBe("nuxt");
    await key("ArrowDown");
    expect(chosenKind()).toBe("nuxt"); // the last one shown
    await key("ArrowUp");
    expect(chosenKind()).toBe("next");
    type("FLASK");
    expect(shown()).toEqual(["flask"]);
    expect(chosenKind()).toBe("flask");
    type("zzz");
    expect(shown()).toEqual([]);
    expect($(".kind-none").hidden).toBe(false);
    search.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(search.value).toBe("");
    expect(shown()).toHaveLength(15);
    expect($(".kind-none").hidden).toBe(true);
    expect($("#new-app").hasAttribute("open")).toBe(true);
    $("#new-cancel").click();
  });

  test("the address follows the typed name and the configured TLD", async () => {
    await openSheet();
    expect($("#new-url").textContent).toBe("Its address: my-app.test");
    fill("Shop Front");
    expect(($("#new-name") as HTMLInputElement).value).toBe("shop-front");
    expect($("#new-url").textContent).toBe("Its address: shop-front.test");
    expect($("#new-url .new-addr").textContent).toBe("shop-front.test");
    $("#new-cancel").click();
  });

  test("Options show the chosen template's choices, always open, remembered per template", async () => {
    await openSheet();
    await pick("laravel");
    const { summary } = await import("../src/newapp");
    expect($("#new-options .opt-panel")).not.toBeNull(); // no disclosure: always shown
    expect(summary()).toBe("No starter kit · SQLite · npm · PHP 8.4 · Git · HTTPS");
    const keys = () => $$("#new-opts [data-key]").map((x) => x.dataset.key);
    expect(keys()).toEqual(["kit", "db", "js", "php", "git", "https"]);
    const kit = $('#new-opts select[data-key="kit"]') as HTMLSelectElement;
    kit.value = "livewire";
    kit.dispatchEvent(new Event("change"));
    expect(summary()).toBe("Livewire · SQLite · npm · PHP 8.4 · Git · HTTPS");

    await pick("next");
    expect(keys()).toEqual(["js", "node", "ts", "tailwind", "git", "https"]);
    const tw = $('#new-opts input[data-key="tailwind"]') as HTMLInputElement;
    tw.checked = false;
    tw.dispatchEvent(new Event("change"));
    expect(summary()).toBe("TypeScript · npm · Node 24 · Git · HTTPS");

    await pick("flask");
    expect(keys()).toEqual(["git", "https"]);

    await pick("laravel");
    expect(summary()).toContain("Livewire");
    fill("kit-app");
    submit();
    await wait(80);
    expect(calls("create_app").at(-1)![1].options).toEqual({
      kit: "livewire",
      db: "sqlite",
      js: "npm",
      php: "8.4",
      git: "on",
      https: "on",
    });
    $("#new-done").click();
    localStorage.removeItem("newOpts:laravel");
  });

  test("HTTPS sits next to Git, starts from the Settings default each time, and is sent with the options", async () => {
    w.mockHttpsDefault = false;
    const { summary } = await import("../src/newapp");
    await openSheet();
    await pick("flask");
    const https = $('#new-opts input[data-key="https"]') as HTMLInputElement;
    expect(https.closest(".opt-checks")!.textContent).toBe("GitHTTPS");
    expect(https.checked).toBe(false);
    expect(summary()).toBe("Git");
    https.checked = true;
    https.dispatchEvent(new Event("change"));
    expect(summary()).toBe("Git · HTTPS");
    await pick("go");
    await pick("flask");
    expect(($('#new-opts input[data-key="https"]') as HTMLInputElement).checked).toBe(false); // not remembered
    w.mockHttpsDefault = true;
    await pick("go");
    expect(($('#new-opts input[data-key="https"]') as HTMLInputElement).checked).toBe(true);
    expect($("#new-opts .opt-panel .opt-checks")).not.toBeNull();
    fill("api");
    submit();
    await wait(80);
    expect(calls("create_app").at(-1)![1].options).toEqual({ git: "on", https: "on" });
    $("#new-done").click();
    localStorage.removeItem("newOpts:flask");
  });

  test("Create turns the sheet into the creation: steps tick, the log is colored, then ready, and Open selects the app", async () => {
    await openSheet();
    await pick("laravel");
    fill("notes");
    w.mockCreateLog =
      "==> Download\n\u001b[32m  - Installing laravel/laravel\u001b[39m\n==> PHP dependencies\nProgress 10%\rProgress 90%\n";
    w.mockDelay = { create_app: 900 };
    submit();
    await wait(450);
    expect($("#new-form").hidden).toBe(true);
    expect($("#new-progress").hidden).toBe(false);
    expect($("#new-progress-title").textContent).toBe("Creating notes…");
    expect($("#new-state .spin-dot")).not.toBeNull();
    expect(steps()).toEqual(["Download:done", "PHP dependencies:now", "JS dependencies:todo", "Set up:todo"]);
    expect($("#new-progress .new-out .ansi2")?.textContent).toContain("Installing laravel/laravel");
    expect($("#new-progress .new-out").textContent).not.toContain("Progress 10%");
    expect(($("#new-done") as HTMLButtonElement).disabled).toBe(true);

    w.mockCreateLog += "==> JS dependencies\n==> Set up\nDone\n";
    await wait(650);
    w.mockDelay = {};
    expect($("#new-progress-title").textContent).toBe("notes is ready");
    expect($("#new-state").classList.contains("ok")).toBe(true);
    expect(steps().every((s) => s.endsWith(":done"))).toBe(true);
    expect($("#new-done").textContent).toBe("Open in Browser");
    expect($("#new-reveal").hidden).toBe(true);
    await select("shop");
    $("#new-done").click();
    await wait();
    expect($("#new-app").hasAttribute("open")).toBe(false);
    expect($('#list [data-name="notes"]').getAttribute("aria-current")).toBe("true");
    // Open in Browser does what it says, and selects the app too.
    expect(calls("open").at(-1)![1]).toEqual({ site: "notes", target: "browser" });
  });

  test("a failure shows ✕, highlights the error in the log, and offers Close and Show Log in Finder", async () => {
    await openSheet();
    fill("broken");
    w.mockCreateLog =
      "==> Download\n==> PHP dependencies\nYour requirements could not be resolved.\n\nPHP dependencies failed (exit status: 1)\n";
    w.mockCreateFail = "PHP dependencies failed (exit status: 1)";
    submit();
    await wait(120);
    w.mockCreateFail = null;
    expect($("#new-progress-title").textContent).toBe("Couldn't create broken");
    expect($("#new-state").classList.contains("bad")).toBe(true);
    expect($(".new-step.bad")?.textContent).toBe("PHP dependencies");
    const errors = $$(".new-out .term-error").map((l) => l.textContent!.trim());
    expect(errors).toContain("PHP dependencies failed (exit status: 1)");
    expect(errors).toContain("Your requirements could not be resolved.");
    expect($("#new-done").textContent).toBe("Close");
    expect($("#new-reveal").hidden).toBe(false);
    expect($("#new-reveal").textContent).toBe("Show Log in Finder");
    $("#new-reveal").click();
    await wait();
    expect(calls("create_reveal").at(-1)![1]).toEqual({ name: "broken" });
    $("#new-done").click();
    expect($("#new-app").hasAttribute("open")).toBe(false);
  });

  test("a refusal before anything ran stays in the form", async () => {
    await openSheet();
    fill("shop");
    w.mockCreateLog = "";
    w.mockCreateFail = "/Users/x/Sites/shop already exists";
    submit();
    await wait(80);
    w.mockCreateFail = null;
    expect($("#new-form").hidden).toBe(false);
    expect($("#new-error").textContent).toBe("/Users/x/Sites/shop already exists");
    $("#new-cancel").click();
  });

  test("a missing PHP is a form error with Install; the typed name stays on reopen", async () => {
    await openSheet();
    await pick("laravel");
    fill("blog");
    const created = calls("create_app").length;
    w.mockNeed = { id: "php", name: "PHP", version: "8.4" };
    submit();
    await wait(80);
    expect(calls("create_app").length).toBe(created);
    expect($("#new-error").hidden).toBe(false);
    expect($("#new-error").textContent).toBe("Laravel needs PHP, which isn't installed. Install PHP 8.4");
    $("#new-cancel").click();
    await openSheet();
    expect(($("#new-name") as HTMLInputElement).value).toBe("blog");
    submit();
    await wait(80);
    w.mockNeed = null;
    ($("#new-error .inline-action") as HTMLElement).click();
    await wait(80);
    expect(calls("php_install").at(-1)![1]).toEqual({ version: "8.4" });
    expect($("#new-error").hidden).toBe(true);
    submit();
    await wait(80);
    expect(calls("create_app").length).toBe(created + 1);
    $("#new-done").click();
  });
});

describe("show hidden", () => {
  const toggle = async () => {
    $("#filter").click();
    await wait(60);
    (w.mockMenu as any[]).find((i) => i.text === "Show Hidden Applications").action();
    await wait(60);
  };

  test("off by default: hidden apps are not listed", () => {
    expect(localStorage.getItem("showHidden")).toBeNull();
    expect($(`#list [data-name="old-blog"]`)).toBeNull();
  });

  test("on: a hidden app is listed dimmed, with a hidden label and no lock, and its pane is a notice", async () => {
    await toggle();
    expect(localStorage.getItem("showHidden")).toBe("1");
    const row = $(`#list [data-name="old-blog"]`);
    expect(row.classList.contains("hidden-app")).toBe(true);
    expect(row.querySelector(".kind")!.textContent).toBe("hidden");
    expect(row.querySelector(".lock")).toBeNull();
    row.click();
    await wait(80);
    expect($("#site .site-name").textContent).toBe("old-blog");
    expect($("#site .hidden-note").textContent).toContain("Hidden from Anvil — not served.");
    expect($$("#site [role=tab]").length).toBe(0);
  });

  test("right-click offers only Show in Anvil, which unhides the app and keeps it selected", async () => {
    $(`#list [data-name="old-blog"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait(80);
    const menu = w.mockMenu as any[];
    expect(menu.map((i) => i.text)).toEqual(["Show in Anvil"]);
    menu[0].action();
    await wait(80);
    expect(calls("unhide_site").at(-1)![1]).toEqual({ site: "old-blog" });
    const row = $(`#list [data-name="old-blog"]`);
    expect(row.classList.contains("hidden-app")).toBe(false);
    expect(row.getAttribute("aria-current")).toBe("true");
    expect($("#site .hidden-note")).toBeNull();
  });

  test("a tab stays selected: an app without the chosen tab shows its first, and the choice comes back", async () => {
    await select("vuefinder");
    ($('#site .tabs [data-tab="git"]') as HTMLElement).click();
    await wait(80);
    const selectedTab = () =>
      $$("#site .tabs [role=tab]")
        .filter((b) => !b.hidden && b.getAttribute("aria-selected") === "true")
        .map((b) => b.dataset.tab);
    expect(selectedTab()).toEqual(["git"]);
    await select("notes"); // a folder that isn't a repository: no Git tab
    await wait(120);
    expect(selectedTab()).toEqual(["overview"]);
    await select("vuefinder");
    await wait(120);
    expect(selectedTab()).toEqual(["git"]);
    ($('#site .tabs [data-tab="overview"]') as HTMLElement).click();
  });

  test("Add service: two columns, and Add waits for the license unless it was accepted before", async () => {
    w.mockRedisAccepted = { "8.10.2": "2026-10-01" };
    const { openAddService } = await import("../src/services");
    await openAddService();
    await wait(30);
    const names = $$("#svc-kinds .kind-item .kind-name").map((n) => n.textContent);
    expect(names).toEqual(["MySQL", "Redis"]);
    ($("#svc-kinds-mysql") as HTMLElement).click();
    expect($("#svc-license-text").textContent).toContain("GPL-2.0 with the Universal FOSS Exception");
    const add = $("#svc-create") as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    const check = $("#svc-license-check") as HTMLInputElement;
    check.checked = true;
    check.dispatchEvent(new Event("change"));
    expect(add.disabled).toBe(false);
    // Redis 8 was accepted before: a note, and Add is ready.
    ($("#svc-kinds-redis") as HTMLElement).click();
    expect($("#svc-license-done").hidden).toBe(false);
    expect(add.disabled).toBe(false);
    // Redis 7.2 comes under another license: asked again.
    const ver = $("#svc-version") as HTMLSelectElement;
    ver.value = "7.2.16";
    ver.dispatchEvent(new Event("change"));
    expect($("#svc-license-text").textContent).toContain("BSD-3-Clause");
    expect(add.disabled).toBe(true);
    check.checked = true;
    check.dispatchEvent(new Event("change"));
    $("#svc-port").setAttribute("value", "6380");
    ($("#svc-port") as HTMLInputElement).value = "6380";
    // A first add: what it downloads, then that it builds, beside the buttons.
    w.mockDelay = { ...w.mockDelay, add_service: 1500 };
    w.mockInstalling = { what: "Redis 7.2.16", build: true, downloaded: 3145728 };
    ($("#svc-form") as HTMLFormElement).requestSubmit();
    await wait(600);
    expect($("#svc-create").textContent).toBe("Adding…");
    expect($("#svc-progress").textContent).toBe("Downloading Redis 7.2.16… 3.0 MB");
    w.mockInstalling = { what: "Redis 7.2.16", build: true, downloaded: null };
    await wait(550);
    expect($("#svc-progress").textContent).toBe("Building Redis 7.2.16 from source…");
    delete w.mockInstalling;
    delete w.mockDelay.add_service;
    await wait(500);
    expect(calls("add_service").at(-1)![1]).toMatchObject({ kind: "redis", version: "7.2.16", acceptLicense: true });
    ($("#new-service") as HTMLDialogElement).close();
  });
});

describe("project file", () => {
  test("one quiet Overview line for what .anvil.toml asks and Anvil lacks: Install and Add", async () => {
    const need = (kind, version, install, label, met = false, overridden = null) => ({ kind, version, install, label, met, overridden });
    w.mockSnap.projects = {
      shop: [
        need("php", "8.3", "8.3", "PHP 8.3"),
        need("node", "22", "22", "Node.js 22", true),
        need("mysql", "8.4", "8.4.11", "MySQL 8.4"),
        need("redis", "", "8.10.2", "Redis", true),
      ],
      vuefinder: [need("node", "22", "22", "Node.js 22", true)],
    };
    // The test before leaves the add_service mock's null as the service list.
    await w.loadServices();
    $('.modes [data-mode="apps"]').click();
    w.refresh();
    await wait(80);
    try {
      await select("vuefinder");
      expect($$('[data-panel="overview"] dt').map((d) => d.textContent)).not.toContain("Project");
      await select("shop");
      const row = $$('[data-panel="overview"] .row').find((r) => r.querySelector("dt")!.textContent === "Project")!;
      expect(row.querySelector("dd")!.firstChild!.textContent).toBe("This project asks for PHP 8.3 and MySQL 8.4");
      const [install, add] = [...row.querySelectorAll("button")] as HTMLButtonElement[];
      expect([install.textContent, add.textContent]).toEqual(["Install PHP 8.3", "Add MySQL 8.4"]);
      // Settings: unset would follow the file only once its PHP is installed.
      expect(($("#php") as HTMLSelectElement).options[0].textContent).toBe("Default (8.4)");
      add.click();
      await wait(60);
      expect(($("#new-service") as HTMLDialogElement).open).toBe(true);
      expect(($("#svc-version") as HTMLSelectElement).value).toBe("8.4.11");
      expect($("#svc-license-text").textContent).toContain("MySQL 8.4.11");
      expect(($("#svc-create") as HTMLButtonElement).disabled).toBe(true);
      ($("#new-service") as HTMLDialogElement).close();
      install.click();
      await wait(60);
      expect(calls("php_install").at(-1)![1]).toEqual({ version: "8.3" });
    } finally {
      delete w.mockSnap.projects;
      w.refresh();
      await wait(80);
    }
  });

  test("missing_line_offers_extension: Add turns it on for the app, Building… and errors inline", async () => {
    w.mockSnap.projects = {
      shop: [{ kind: "ext", version: "xdebug", install: "xdebug", label: "xdebug", met: false, overridden: null }],
    };
    w.mockDelay = { app_ext: 100 };
    const line = () => $$('[data-panel="overview"] .row').find((r) => r.querySelector("dt")!.textContent === "Project")!;
    const button = () => line().querySelector("button") as HTMLButtonElement;
    try {
      await wait(550);
      w.refresh();
      await wait(80);
      await select("shop");
      expect(line().querySelector("dd")!.firstChild!.textContent).toBe("This project asks for xdebug");
      expect(button().textContent).toBe("Add xdebug");
      button().click();
      await wait(10);
      expect(calls("app_ext").at(-1)![1]).toEqual({ site: "shop", name: "xdebug", on: true });
      expect([button().textContent, button().disabled]).toEqual(["Building…", true]);
      await wait(150);
      expect([button().textContent, button().disabled]).toEqual(["Add xdebug", false]);

      delete w.mockDelay.app_ext;
      w.mockExtError = "Building xdebug failed";
      button().click();
      await wait(80);
      expect(line().textContent).toContain("Building xdebug failed");
      expect($$('[role="alert"]:not([hidden]), .toast:not([hidden])').length).toBe(0);
    } finally {
      delete w.mockExtError;
      delete w.mockSnap.projects;
      const shop = w.mockSnap.sites.find((s: any) => s.name === "shop");
      shop.ext = [];
      shop.own_master = false;
      w.mockDelay = {};
      w.refresh();
      await wait(80);
    }
  });
});

describe("mail", () => {
  test("an app sending to an Anvil Mailpit gets a Mail line: Start while stopped, then Open inbox", async () => {
    const mailpit = w.mockServices.find((s: any) => s.id === "mailpit");
    const row = () => $('[data-panel="overview"] .mail-row');
    try {
      await w.loadServices();
      $('.modes [data-mode="apps"]').click();
      await select("vuefinder");
      expect(row().hidden).toBe(true);
      await select("shop");
      expect(row().hidden).toBe(false);
      expect(row().querySelector("dd")!.textContent).toBe("mailpit is stopped Start");
      mailpit.running = true; // what service_run hands back
      (row().querySelector("button") as HTMLButtonElement).click();
      await wait(40);
      expect(calls("service_run").at(-1)![1]).toEqual({ id: "mailpit", action: "start" });
      expect(row().querySelector("dd")!.textContent).toBe("mailpit Open inbox");
      (row().querySelector("button") as HTMLButtonElement).click();
      expect(calls("open_url").at(-1)![1]).toEqual({ url: "http://127.0.0.1:8025" });
    } finally {
      mailpit.running = false;
      await w.loadServices();
    }
  });
});

describe("share", () => {
  const row = () => $('[data-panel="overview"] .share-row');
  const state = async () => (await import("../src/app")).call("state");
  test("Request log off in Settings › Features: no Requests tab", async () => {
    w.mockSnap.request_log = false;
    try {
      await state();
      await select("shop");
      expect(($('.tabs [data-tab="requests"]') as HTMLElement).hidden).toBe(true);
    } finally {
      w.mockSnap.request_log = true;
      await state();
    }
    await select("shop");
    expect(($('.tabs [data-tab="requests"]') as HTMLElement).hidden).toBe(false);
  });

  test("off in Settings › Features: no Share line", async () => {
    await select("shop");
    expect(row().hidden).toBe(true);
  });

  test("Share asks first, then shows the public address with Copy and Stop, and the app's note", async () => {
    w.mockSnap.tunnel = true;
    try {
      await state();
      await select("notes"); // a folder has no address
      expect(row().hidden).toBe(true);
      await select("shop");
      expect(row().hidden).toBe(false);
      expect(row().querySelector("dd")!.textContent).toBe("Share…");
      (row().querySelector("button") as HTMLButtonElement).click();
      // Nothing is public before the confirm.
      expect(calls("share_start").length).toBe(0);
      const pop = $(".pop-confirm");
      expect(pop.textContent).toContain("Anyone with the address can open shop");
      expect(pop.textContent).toContain("changes every time");
      (pop.querySelector("button.primary") as HTMLButtonElement).click();
      await wait(40);
      expect(calls("share_start").at(-1)![1]).toEqual({ site: "shop" });
      const dd = row().querySelector("dd")!;
      expect(dd.querySelector("a")!.textContent).toBe("https://quiet-river.trycloudflare.com");
      expect([...dd.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Copy", "Stop"]);
      expect(dd.querySelector(".share-note")!.textContent).toContain("shop.test");
      (dd.querySelector("a") as HTMLElement).click();
      expect(calls("open_url").at(-1)![1]).toEqual({ url: "https://quiet-river.trycloudflare.com" });
      ([...dd.querySelectorAll("button")].at(-1) as HTMLButtonElement).click();
      await wait(40);
      expect(calls("share_stop").at(-1)![1]).toEqual({ site: "shop" });
      expect(row().querySelector("dd")!.textContent).toBe("Share…");
    } finally {
      w.mockSnap.tunnel = false;
      await state();
    }
  });

  test("a tunnel that couldn't open says why on its line, not in the banner", async () => {
    w.mockSnap.tunnel = true;
    w.mockShareFail = "Anvil's web server isn't running";
    try {
      await state();
      await select("shop");
      (row().querySelector("button") as HTMLButtonElement).click();
      ($(".pop-confirm button.primary") as HTMLButtonElement).click();
      await wait(40);
      expect(row().querySelector(".share-error")!.textContent).toBe("Anvil's web server isn't running");
      expect($("#error").hidden).toBe(true);
      // Starting, then failed later: the backend's reason on the same line.
      w.mockShareFail = null;
      w.mockSnap.shares = { shop: { state: "failed", error: "Couldn't reach Cloudflare", note: null } };
      await state();
      expect(row().querySelector(".share-error")!.textContent).toBe("Couldn't reach Cloudflare");
      w.mockSnap.shares = { shop: { state: "starting", note: null } };
      await state();
      expect(row().querySelector("dd")!.textContent).toContain("Opening…");
    } finally {
      w.mockSnap.tunnel = false;
      w.mockShareFail = null;
      w.mockSnap.shares = {};
      await state();
    }
  });

  test("a Stop that failed says why on the line, the address still shown", async () => {
    w.mockSnap.tunnel = true;
    w.mockSnap.shares = { shop: { state: "ready", url: "https://quiet-river.trycloudflare.com", note: null } };
    w.mockShareStopFail = "cloudflared didn't stop";
    try {
      await state();
      await select("shop");
      ([...row().querySelectorAll("button")].at(-1) as HTMLButtonElement).click();
      await wait(40);
      expect(row().querySelector(".share-error")!.textContent).toBe("cloudflared didn't stop");
      expect(row().querySelector("a")!.textContent).toBe("https://quiet-river.trycloudflare.com");
    } finally {
      w.mockSnap.tunnel = false;
      w.mockShareStopFail = null;
      w.mockSnap.shares = {};
      await state();
    }
  });
});

describe("resources", () => {
  test("Delete Logs and Caches asks in the row, in Settings › Storage's words, then says what it freed", async () => {
    const item = (label: string, path: string, bytes: number, safety: string) => ({ label, path, bytes, safety, note: "" });
    w.mockResources = {
      total: 734003200,
      error: null,
      processes: [],
      services: [],
      items: [item("Logs", "storage/logs", 12582912, "safe"), item("Sessions", "storage/framework/sessions", 2097152, "safe")],
    };
    try {
      await select("shop");
      $('.tabs [data-tab="resources"]').click();
      await wait(80);
      expect($(".clean-all").textContent).toBe("Delete Logs and Caches (14 MB)");
      $(".clean-all").click();
      expect($(".res-head .asking .muted").textContent).toBe("Delete 14 MB? Users get logged out.");
      expect(calls("clean")).toEqual([]);
      $$(".res-head .asking button")[1].click();
      await wait(80);
      expect(calls("clean").map(([, a]) => a.path)).toEqual(["storage/logs", "storage/framework/sessions"]);
      expect($(".res-freed").textContent).toMatch(/^Freed \d/);
    } finally {
      delete w.mockResources;
      $('.tabs [data-tab="overview"]').click();
    }
  });
});

describe("mail catcher", () => {
  const rows = () => $$(".mail-list .log");
  const cell = (r: HTMLElement, c: string) => r.querySelector(c)?.textContent;
  const frame = () => $(".mail-list .log-detail iframe") as HTMLIFrameElement;
  const view = (id: string) => ($(`.mail-list .log-detail .seg [data-id="${id}"]`) as HTMLElement).click();

  test("off: no Mail tab; on: the Overview's Mail line says Anvil catches it, Show mail opens the tab", async () => {
    await select("shop");
    expect($('.tabs [data-tab="mail"]').hidden).toBe(true);
    w.mockSnap.catcher = { port: 2525, error: null, caught: ["shop"] };
    await wait(550);
    w.refresh();
    await wait(80);
    await select("shop");
    const tabs = $$(".tabs [role=tab]").map((t) => t.dataset.tab);
    expect(tabs.indexOf("mail")).toBe(tabs.indexOf("requests") + 1);
    expect($('.tabs [data-tab="mail"]').hidden).toBe(false);
    const row = $('[data-panel="overview"] .mail-row');
    expect(row.hidden).toBe(false);
    // The catcher instead of Mailpit, whose line shop would get otherwise.
    expect(row.querySelector("dd")!.textContent).toBe("Caught by Anvil Show mail");
    (row.querySelector("button") as HTMLElement).click();
    await wait(80);
    expect($('[data-panel="mail"]').hidden).toBe(false);
  });

  test("lists the newest first: time, from, to, subject, files and size; says how it catches", async () => {
    expect(calls("mail_list").at(-1)![1]).toEqual({ inbox: "shop" });
    const [first, second] = rows();
    expect(cell(first, ".mail-from")).toBe("Shop <hello@example.com>");
    expect(cell(first, ".mail-to")).toBe("user@example.com");
    expect(cell(first, ".log-msg")).toBe("Şifre sıfırlama");
    expect(cell(first, ".mail-files")).toBe("1");
    expect(cell(first, ".req-size")).toBe("1.5 KB");
    expect(cell(second, ".log-msg")).toBe("(no subject)");
    expect(second.querySelector(".mail-files")!.textContent).toBe("");
    expect($(".mail-how").textContent).toBe(
      "Anvil catches shop’s mail: its processes and PHP get MAIL_HOST=127.0.0.1, MAIL_PORT=2525 and MAIL_USERNAME=shop, overriding its .env. A running process gets them when it restarts.",
    );
    expect(($(".mail-catch input") as HTMLInputElement).checked).toBe(true);
  });

  test("a mail opens to its HTML in a sandbox: no scripts, no remote loads until asked", async () => {
    (rows()[0].querySelector(".log-head") as HTMLElement).click();
    await wait(40);
    expect(calls("mail_read").at(-1)![1]).toEqual({ inbox: "shop", id: "1792505339198-2" });
    const f = frame();
    expect(f.getAttribute("sandbox")).toBe("allow-same-origin");
    expect(f.srcdoc).toContain(`content="default-src 'none'; img-src data:; style-src 'unsafe-inline'"`);
    expect(f.srcdoc).toContain("https://tracker.example/p.gif");
    const remote = $(".mail-remote") as HTMLButtonElement;
    expect(remote.hidden).toBe(false);
    remote.click();
    expect(frame().srcdoc).toContain("img-src data: https: http:");
    expect($(".mail-remote")).toBeNull();
  });

  test("Text, Headers and Source views; Save puts the file in Downloads and says where", async () => {
    view("text");
    expect($(".mail-list .log-detail .mail-text").textContent).toBe("Reset: https://shop.test/reset");
    view("headers");
    expect($(".mail-list .log-detail .req-headers").textContent).toContain("SubjectŞifre sıfırlama");
    view("source");
    await wait(30);
    expect(calls("mail_source").at(-1)![1]).toEqual({ inbox: "shop", id: "1792505339198-2" });
    expect($(".mail-list .log-detail .mail-text").textContent).toBe("From: x\r\n\r\nraw");
    const files = $$(".mail-list .log-detail .mail-file");
    expect(files.map((f) => f.querySelector("span")!.textContent)).toEqual(["invoice.pdf 13 B"]);
    (files[0].querySelector("button") as HTMLElement).click();
    await wait(30);
    expect(calls("mail_save").at(-1)![1]).toEqual({ inbox: "shop", id: "1792505339198-2", index: 1 });
    expect(files[0].textContent).toBe("invoice.pdf 13 B Saved to ~/Downloads/invoice.pdf");
  });

  test("new mail comes in on top while the tab is shown; Delete asks in place, then takes one out", async () => {
    const mail = await import("../src/mail");
    mail.poll(false); // ticks by hand
    w.mockMails.shop.unshift({ ...w.mockMails.shop[1], id: "1792505400000-9", subject: "Newest" });
    await mail.tick();
    expect(cell(rows()[0], ".log-msg")).toBe("Newest");
    expect(rows().length).toBe(3);
    // The open one stays open.
    expect(rows()[1].querySelector(".log-detail")).not.toBeNull();
    const n = calls("mail_delete").length;
    (rows()[1].querySelector(".mail-delete") as HTMLElement).click();
    const ask = rows()[1].querySelector(".mail-actions.asking") as HTMLElement;
    expect(ask.querySelector(".muted")!.textContent).toBe("Delete this message?");
    (ask.querySelectorAll("button")[0] as HTMLElement).click(); // Cancel
    expect(rows()[1].querySelector(".mail-actions.asking")).toBeNull();
    expect(calls("mail_delete").length).toBe(n);
    (rows()[1].querySelector(".mail-delete") as HTMLElement).click();
    (rows()[1].querySelectorAll(".mail-actions button")[1] as HTMLElement).click();
    await wait(30);
    expect(calls("mail_delete").at(-1)![1]).toEqual({ inbox: "shop", id: "1792505339198-2" });
    expect(rows().map((r) => cell(r, ".log-msg"))).toEqual(["Newest", "(no subject)"]);
  });

  test("Clear asks in the bar, no dialog; Cancel puts Clear back", async () => {
    const saved = structuredClone(w.mockMails.shop);
    $(".mail-clear").click();
    expect($(".mail-clear-box .muted").textContent).toBe("Delete all 2 messages? This can’t be undone.");
    $$(".mail-clear-box button")[0].click();
    expect($(".mail-clear-box .asking")).toBeNull();
    $(".mail-clear").click();
    $$(".mail-clear-box button")[1].click();
    await wait(30);
    expect(calls("mail_clear").at(-1)![1]).toEqual({ inbox: "shop" });
    expect(rows()).toEqual([]);
    expect($(".mail-clear").hidden).toBe(true);
    w.mockMails.shop = saved;
  });

  test("turning catching off or on while processes run offers to restart them, asked first", async () => {
    const box = $(".mail-catch input") as HTMLInputElement;
    expect($(".mail-restart")).toBeNull();
    const n = calls("process_run").length;
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await wait(60);
    const line = $(".mail-restart");
    expect(line.textContent).toMatch(/still sends? mail the old way until restarted\. Restart/);
    (line.querySelector("button") as HTMLElement).click();
    expect(calls("process_run").length).toBe(n);
    expect($(".pop-confirm").textContent).toMatch(/^Restart .+ for a moment\./);
    ($(".pop-confirm button.primary") as HTMLElement).click();
    await wait(60);
    expect(
      calls("process_run")
        .slice(n)
        .every(([, a]) => a.action === "restart" && a.site === "shop" && a.name),
    ).toBe(true);
    expect(calls("process_run").length).toBeGreaterThan(n);
    expect($(".mail-restart")).toBeNull();
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    await wait(60);
  });

  test("the switch leaves the app to its own mail setup; the line says so", async () => {
    const box = $(".mail-catch input") as HTMLInputElement;
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await wait(60);
    expect(calls("set_mail_caught").at(-1)![1]).toEqual({ site: "shop", on: false });
    expect($(".mail-how").textContent).toBe("shop sends mail with its own settings; Anvil doesn’t catch it.");
    expect($('[data-panel="overview"] .mail-row dd').textContent).toBe("mailpit is stopped Start");
    w.mockSnap.catcher = null;
  });
});

describe("php.ini fields", () => {
  test("the upload limit sets both limits in one restart, said beside the field", async () => {
    await select("shop");
    $('.tabs [data-tab="settings"]').click();
    await wait(60);
    const n = calls("set_site_ini").length;
    const input = $("#ini-upload") as HTMLInputElement;
    input.value = "200M";
    input.dispatchEvent(new Event("change"));
    expect($(".ini-note").textContent).toBe("Restarting the web server…");
    await wait(60);
    expect(calls("set_site_ini").slice(n)).toEqual([
      [
        "set_site_ini",
        {
          site: "shop",
          values: [
            ["upload_max_filesize", "200M"],
            ["post_max_size", "200M"],
          ],
        },
      ],
    ]);
    expect($(".ini-note").textContent).toBe("Applied; the web server restarted.");
    input.value = "";
    input.dispatchEvent(new Event("change"));
    await wait(60);
    $('.tabs [data-tab="overview"]').click();
  });
});

describe("run settings", () => {
  test("a .NET app has no version picker: dotnet picks the SDK from global.json", async () => {
    w.mockSnap.sites.push(w.mockSite("api", "dotnet", { runtime_version: "10.0" }));
    w.mockSnap.runtimes.dotnet = ["10.0", "9.0"];
    const state = async () => {
      await (await import("../src/app")).call("state");
      await wait(40);
    };
    try {
      await state();
      await select("vuefinder");
      expect($("#site #app-runtime")).not.toBeNull();
      await select("api");
      expect($("#site #app-runtime")).toBeNull();
      expect($("#site #app-command")).not.toBeNull();
    } finally {
      w.mockSnap.sites.pop();
      delete w.mockSnap.runtimes.dotnet;
      await state();
    }
  });
});

describe("worktrees", () => {
  const wt = "shop--feature-x";
  const sheetOpen = () => ($("#ask") as HTMLDialogElement).open;
  const view = (o = {}) => ({
    db: "copy",
    choices: ["copy"],
    prepared: true,
    database: "shop_feature_x",
    main_database: "shop",
    pending: false,
    preparing: null,
    compose: false,
    cached_config: false,
    error: null,
    copies: ["shop_feature_x"],
    adopted: false,
    missing: { copy: [], installs: [] },
    same: [],
    ...o,
  });
  const ovText = () => $('[data-panel="overview"]').textContent!;
  const dbRow = () => $$('[data-panel="overview"] .row').find((r) => r.querySelector("dt")?.textContent === "Database")!;
  async function show(o = {}, name = wt) {
    w.mockSnap.worktrees = { [wt]: view(o) };
    await (await import("../src/app")).call("state");
    await wait(40);
    await select(name);
  }
  beforeAll(() => {
    w.mockSnap.sites.push(
      w.mockSite(wt, "laravel", {
        worktree_of: "shop",
        branch: "feature/login",
        path: `/Users/x/Sites/${wt}`,
        root: `/Users/x/Sites/${wt}/public`,
      }),
    );
  });

  test("a worktree's Overview: what it is, its own database with the choice, what stays shared", async () => {
    await show();
    // The header says whose worktree it is, its mark a branch; no line of its own.
    expect($("#site .meta").textContent).toContain("worktree of shop");
    expect($("#site .site-title .mark").classList.contains("wt-mark")).toBe(true);
    expect($('[data-panel="overview"] .worktree-of')).toBeNull();
    expect(dbRow().querySelector("dd")!.firstChild!.textContent).toBe("shop_feature_x (copy of shop)");
    // Made already: nothing left to choose, nothing that would remake it.
    expect(dbRow().querySelector("select")).toBeNull();
    expect(dbRow().textContent).not.toContain("Share");
    expect(ovText()).toContain("S3 buckets and search indexes are shared with shop");
    expect(ovText()).not.toContain("Docker services");
    expect(ovText()).not.toContain("config:clear");
    expect(dbRow().querySelector("button")).toBeNull(); // made already: no Prepare
  });

  test("the database row says which it is, with no choice to make; check warns with Rewrite, confirmed first", async () => {
    await show({ pending: true, prepared: false, copies: [], db: "empty", choices: ["copy", "empty"] });
    expect(dbRow().querySelector("dd")!.firstChild!.textContent).toBe("shop_feature_x (empty)");
    expect(dbRow().querySelector("select")).toBeNull();
    const env = () => $$('[data-panel="overview"] .row').find((r) => r.querySelector("dt")?.textContent === "Env");
    expect(env()).toBeUndefined();
    w.mockRewriteFail = "shop--feature-x has none of the env files its hooks copy";
    try {
      await show({ same: ["DB_DATABASE", "SESSION_COOKIE"] });
      expect(env()!.textContent).toContain("DB_DATABASE, SESSION_COOKIE have shop's value in its env files");
      (env()!.querySelector("button") as HTMLButtonElement).click();
      await wait();
      expect($(".pop-confirm").textContent).toContain("Only those lines change");
      expect(calls("worktree_rewrite").length).toBe(0);
      ($(".pop-confirm .primary") as HTMLButtonElement).click();
      await wait(80);
      expect(calls("worktree_rewrite").at(-1)![1]).toEqual({ site: wt });
      expect(env()!.textContent).toContain("has none of the env files its hooks copy");
    } finally {
      delete w.mockRewriteFail;
    }
  });

  test("Docker services and a cached config each get one line", async () => {
    await show({ compose: true, cached_config: true });
    expect(ovText()).toContain("Anvil doesn't start this worktree's Docker services");
    expect(ovText()).toContain("Cached config ignores the worktree's database; run php artisan config:clear");
  });

  test("Prepare database while it isn't made yet, with the preparing line inline", async () => {
    await show({ pending: true, copies: [] });
    const b = [...dbRow().querySelectorAll("button")].find((x) => x.textContent === "Prepare database") as HTMLButtonElement;
    expect(b).toBeTruthy();
    w.mockDelay = { worktree_prepare: 150 };
    b.click();
    await wait(40);
    expect(b.disabled).toBe(true);
    await wait(200);
    w.mockDelay = {};
    expect(calls("worktree_prepare").at(-1)![1]).toEqual({ site: wt });
    // A copy under way (from Start or this button) shows its line in place of the button.
    await show({ pending: true, copies: [], preparing: "Anvil: preparing shop_feature_x (copy of shop)" });
    expect(dbRow().textContent).toContain("Anvil: preparing shop_feature_x (copy of shop)");
    expect(dbRow().querySelector("button")).toBeNull();
    // A failure shows next to the button.
    w.mockPrepareFail = "Start mysql to prepare shop_feature_x";
    await show({ pending: true, copies: [] });
    (dbRow().querySelector("button") as HTMLButtonElement).click();
    await wait(80);
    expect(dbRow().textContent).toContain("Start mysql to prepare shop_feature_x");
    delete w.mockPrepareFail;
  });

  test("a database that is there but isn't Anvil's: Use existing, confirmed first", async () => {
    w.mockPrepareFail = "shop_feature_x already exists and Anvil didn't make it";
    try {
      await show({ pending: true, prepared: false, copies: [] });
      (dbRow().querySelector("button") as HTMLButtonElement).click();
      await wait(80);
      const use = [...dbRow().querySelectorAll("button")].find((b) => b.textContent === "Use existing shop_feature_x") as HTMLButtonElement;
      expect(use).toBeTruthy();
      use.click();
      await wait();
      expect($(".pop-confirm").textContent).toContain("Use shop_feature_x for this worktree?");
      expect(calls("worktree_adopt").length).toBe(0);
      ($(".pop-confirm .primary") as HTMLButtonElement).click();
      await wait(60);
      expect(calls("worktree_adopt").at(-1)![1]).toEqual({ site: wt });
    } finally {
      delete w.mockPrepareFail;
    }
    // Used: the line says so.
    await show({ adopted: true });
    expect(dbRow().querySelector("dd")!.firstChild!.textContent).toBe("shop_feature_x (existing)");
  });

  test("a worktree without .env or packages: Copy .env, and installs confirmed before they run", async () => {
    const setup = () => $$('[data-panel="overview"] .row').find((r) => r.querySelector("dt")?.textContent === "Setup")!;
    await show();
    expect(setup()).toBeUndefined();
    await show({
      missing: {
        copy: [".env"],
        installs: [
          ["composer", "composer install"],
          ["js", "pnpm install"],
        ],
      },
    });
    const btn = (t: string) => [...setup().querySelectorAll("button")].find((b) => b.textContent === t) as HTMLButtonElement;
    btn("Copy .env from shop").click();
    await wait(60);
    expect(calls("worktree_copy_files").at(-1)![1]).toEqual({ site: wt });
    btn("composer install").click();
    await wait();
    expect($(".pop-confirm").textContent).toContain(`Run composer install in ${wt}?`);
    expect(calls("worktree_install").length).toBe(0);
    ($(".pop-confirm .primary") as HTMLButtonElement).click();
    await wait(60);
    expect(calls("worktree_install").at(-1)![1]).toEqual({ site: wt, what: "composer" });
    expect(btn("pnpm install")).toBeTruthy();
  });

  test("the Git tab has New Worktree… for an app's own folder, not for a worktree", async () => {
    const gitButtons = () => [...$$('[data-panel="git"] button')].map((b) => b.textContent);
    await show({}, "shop");
    await wait(80);
    expect(gitButtons()).toContain("New Worktree…");
    ($$('[data-panel="git"] button').find((b) => b.textContent === "New Worktree…") as HTMLButtonElement).click();
    await wait();
    expect(sheetOpen()).toBe(true);
    expect($("#ask-label").textContent).toBe("New worktree of shop");
    // Create says it works while git and the copied files take their time; a refusal stays.
    ($("#ask-input") as HTMLInputElement).value = "feature/y";
    w.mockDelay = { worktree_add: 100 };
    w.mockAddFail = "/Users/x/Sites/shop--feature-y already exists";
    $("#ask-form").dispatchEvent(new Event("submit", { cancelable: true }));
    await wait(30);
    expect($("#ask-ok").textContent).toBe("Creating…");
    await wait(120);
    expect($("#ask-ok").textContent).toBe("Create");
    expect($("#ask-error").textContent).toContain("already exists");
    w.mockDelay = {};
    delete w.mockAddFail;
    $("#ask-cancel").click();
    expect(sheetOpen()).toBe(false);
    await select(wt);
    await wait(80);
    expect(gitButtons()).not.toContain("New Worktree…");
  });

  test("New Worktree… is offered for a linked app that isn't an alias", async () => {
    w.mockSnap.sites.push(w.mockSite("linked-app", "laravel", { linked: true, path: "/Users/x/elsewhere/linked-app", parked_in: null }));
    try {
      await show({}, "linked-app");
      $$("#list [data-name]")
        .find((r) => r.dataset.name === "linked-app")!
        .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
      await wait(80);
      expect((w.mockMenu as any[]).map((i) => i.text)).toContain("New Worktree…");
    } finally {
      w.mockSnap.sites.pop();
    }
  });

  test("the main app's Overview: a database left from a removed worktree, Delete (confirmed) or Keep", async () => {
    w.mockSnap.leftovers = [{ app: "shop", worktree: "x", service: "mysql", database: "blog" }];
    try {
      await show({}, "shop");
      await wait(80);
      const row = $$('[data-panel="overview"] .leftover')[0];
      expect(row.firstChild!.textContent).toBe("Database left from x (1.0 MB)");
      const [del, keep] = [...row.querySelectorAll("button")] as HTMLButtonElement[];
      expect([del.textContent, keep.textContent]).toEqual(["Delete", "Keep"]);
      del.click();
      await wait();
      expect($(".pop-confirm").textContent).toContain("Delete blog?");
      expect(calls("leftover_drop").length).toBe(0);
      ($(".pop-confirm .primary") as HTMLButtonElement).click();
      await wait(60);
      expect(calls("leftover_drop").at(-1)![1]).toEqual({ service: "mysql", database: "blog" });
      keep.click();
      await wait(60);
      expect(calls("leftover_keep").at(-1)![1]).toEqual({ app: "shop", worktree: "x" });
    } finally {
      delete w.mockSnap.leftovers;
    }
  });

  const menuItem = (items: any[], text: string): any => {
    for (const i of items) {
      if (i.text === text) return i;
      const found = i.items && menuItem(i.items, text);
      if (found) return found;
    }
  };
  const openMenu = async (text: string) => {
    $('#list [data-name="shop"]').dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait(80);
    menuItem(w.mockMenu, text).action();
    await wait();
  };
  const openNew = async () => {
    await show({}, "shop");
    await openMenu("New Worktree…");
  };
  const checks = () => $$("#ask-checks input") as HTMLInputElement[];
  const submit = async () => {
    $("#ask-form").dispatchEvent(new Event("submit"));
    await wait(80);
  };

  test("New Worktree… is a sheet: the branch, said to be existing or new, Copy the database too as the hooks set it", async () => {
    w.mockNewDb = { can_copy: true, copy: true, main_database: "shop" };
    try {
      await openNew();
      expect(sheetOpen()).toBe(true);
      expect($("#ask").classList.contains("sheet")).toBe(true);
      expect($("#ask-label").textContent).toBe("New worktree of shop");
      expect($("#ask-ok").textContent).toBe("Create");
      // No "New branch" box: a name that isn't a branch yet is made.
      expect($$("#ask-checks label").map((l) => l.textContent!.trim())).toEqual(["Copy the database too"]);
      expect(checks().map((c) => c.checked)).toEqual([true]);
      const input = $("#ask-input") as HTMLInputElement;
      input.value = "feature/login";
      input.dispatchEvent(new Event("input"));
      expect($("#ask-hint").textContent).toBe("Existing branch · shop--feature-login.test");
      input.value = "feature-a";
      input.dispatchEvent(new Event("input"));
      expect($("#ask-hint").textContent).toBe("New branch · shop--feature-a.test");
      input.value = "feature/login";
      checks()[0].checked = false;
      // Refused: said in the sheet, which stays open.
      w.mockAddFail = "/Users/x/Sites/shop--feature-login already exists";
      await submit();
      expect(sheetOpen()).toBe(true);
      expect($("#ask-error").hidden).toBe(false);
      expect($("#ask-error").textContent).toContain("already exists");
      delete w.mockAddFail;
      await submit();
      expect(calls("worktree_add").at(-1)![1]).toEqual({ site: "shop", branch: "feature/login", new: false, copyDb: false });
      expect(sheetOpen()).toBe(false);
    } finally {
      delete w.mockNewDb;
      delete w.mockAddFail;
      w.mockSnap.sites = w.mockSnap.sites.filter((x) => x.name !== "shop--feature-login");
    }
  });

  test("New Worktree… shows the database checkbox only when main's can be copied", async () => {
    try {
      w.mockNewDb = { can_copy: false, copy: false, main_database: "shop" };
      await openNew();
      expect(checks().length).toBe(0);
      $("#ask-cancel").click();
      w.mockNewDb = { can_copy: true, copy: false, main_database: null };
      await openNew();
      expect(checks().length).toBe(0);
      ($("#ask-input") as HTMLInputElement).value = "x";
      await submit();
      expect(calls("worktree_add").at(-1)![1]).toEqual({ site: "shop", branch: "x", new: true, copyDb: null });
    } finally {
      delete w.mockNewDb;
      w.mockSnap.sites = w.mockSnap.sites.filter((x) => x.name !== "shop--x");
    }
  });

  test("after New Worktree, its after-create commands are offered and run only on Run", async () => {
    w.mockNewDb = { can_copy: true, copy: false, main_database: "shop" };
    w.mockAfter = ["composer install", "php artisan migrate"];
    try {
      await openNew();
      ($("#ask-input") as HTMLInputElement).value = "hooks";
      await submit();
      expect(sheetOpen()).toBe(true);
      expect($("#ask-label").textContent).toBe("shop--hooks is ready");
      expect($$("#ask-body li").map((l) => l.textContent)).toEqual(["composer install", "php artisan migrate"]);
      expect($("#ask-field").hidden).toBe(true);
      expect(calls("worktree_after_create").length).toBe(0);
      expect($("#ask-ok").textContent).toBe("Run");
      // It starts on Cancel: a held Return runs nothing.
      expect(document.activeElement).toBe($("#ask-cancel"));
      expect(calls("worktree_after_create_list").at(-1)![1]).toEqual({ site: "shop--hooks" });
      await submit();
      expect(calls("worktree_after_create").at(-1)![1]).toEqual({
        site: "shop--hooks",
        commands: ["composer install", "php artisan migrate"],
      });
      expect(sheetOpen()).toBe(false);
    } finally {
      delete w.mockNewDb;
      delete w.mockAfter;
      w.mockSnap.sites = w.mockSnap.sites.filter((x) => x.name !== "shop--hooks");
    }
  });

  test("after New Worktree, a failed after-create list shows in the sheet, and Try again lists them", async () => {
    w.mockNewDb = { can_copy: true, copy: false, main_database: "shop" };
    w.mockAfterListFail = "this app: keys isn't a hooks field";
    const ran = calls("worktree_after_create").length;
    try {
      await openNew();
      ($("#ask-input") as HTMLInputElement).value = "broken";
      await submit();
      expect(sheetOpen()).toBe(true);
      expect($("#ask-label").textContent).toBe("shop--broken is ready");
      expect($("#ask-error").hidden).toBe(false);
      expect($("#ask-error").textContent).toBe("this app: keys isn't a hooks field");
      expect($("#ask-ok").textContent).toBe("Try again");
      // Still failing: the error stays, the sheet too.
      await submit();
      expect(sheetOpen()).toBe(true);
      delete w.mockAfterListFail;
      w.mockAfter = ["composer install"];
      await submit();
      expect(sheetOpen()).toBe(true);
      expect($("#ask-error").hidden).toBe(true);
      expect($$("#ask-body li").map((l) => l.textContent)).toEqual(["composer install"]);
      expect($("#ask-ok").textContent).toBe("Run");
      expect(calls("worktree_after_create").length).toBe(ran);
    } finally {
      delete w.mockNewDb;
      delete w.mockAfter;
      delete w.mockAfterListFail;
      $("#ask-cancel").click();
      w.mockSnap.sites = w.mockSnap.sites.filter((x) => x.name !== "shop--broken");
    }
  });

  test("Add Alias and New Group are the same sheet, with their errors inline", async () => {
    await show({}, "shop");
    const cases: [string, string, string, string][] = [
      ["Add Alias…", "New domain for shop", "add_alias", "alias"],
      ["New Group…", "Group for shop", "set_group", "group"],
    ];
    for (const [item, title, cmd, arg] of cases) {
      await openMenu(item);
      expect(sheetOpen()).toBe(true);
      expect([$("#ask-label").textContent, $("#ask-ok").textContent]).toEqual([title, "Add"]);
      expect(checks().length).toBe(0);
      ($("#ask-input") as HTMLInputElement).value = "admin.shop";
      w.mockAskFail = "That name is taken";
      await submit();
      expect(sheetOpen()).toBe(true);
      expect($("#ask-error").textContent).toBe("That name is taken");
      delete w.mockAskFail;
      await submit();
      expect(calls(cmd).at(-1)![1]).toEqual({ site: "shop", [arg]: "admin.shop" });
      expect(sheetOpen()).toBe(false);
    }
  });

  test("the Settings tab's Worktrees section: each hook, where it comes from, Use preset", async () => {
    w.mockProblems = ["this app: copyy isn't a hooks field, or its value isn't the right kind"];
    await show({}, "shop");
    await wait(80);
    const box = $('[data-panel="settings"] .wt-hooks');
    expect(box.hidden).toBe(false);
    expect(box.querySelector(".error-text:not([hidden])")!.textContent).toBe(
      "this app: copyy isn't a hooks field, or its value isn't the right kind",
    );
    delete w.mockProblems;
    // Closed: what a new worktree gets, in sentences, and where it comes from, once.
    const btn = (t: string) => [...box.querySelectorAll("button")].find((b) => b.textContent === t) as HTMLButtonElement;
    expect([...box.querySelectorAll(".wt-summary > div:not(.muted):not(.wt-actions)")].map((d) => d.textContent)).toEqual([
      "Copies .env",
      "Its own database, empty",
      "Sets APP_URL, DB_DATABASE",
      "Then offers to run composer install",
    ]);
    expect(box.querySelector(".wt-summary .muted")!.textContent).toBe(
      "Changed for this app; the rest from the Laravel preset and .anvil.toml",
    );
    expect(box.querySelectorAll(".row").length).toBe(1);
    expect(box.querySelector("textarea")).toBeNull();
    btn("Customize…").click();
    await wait(60);
    const rows = [...box.querySelectorAll(".row")].map((r) => r.querySelector("dt")!.textContent);
    expect(rows).toEqual(["Worktrees", "Copy files", "Database", "Env keys", "After create"]);
    const [copy, db, keys, after] = [...box.querySelectorAll(".hk")] as HTMLElement[];
    // Copy files: a row per path with ×, Add.
    (copy.querySelector(".hk-add input") as HTMLInputElement).value = ".env.local";
    (copy.querySelector(".hk-add .inline-action") as HTMLButtonElement).click();
    await wait(60);
    expect(calls("set_app_hooks").at(-1)![1]).toEqual({ site: "shop", field: "copy", value: [".env", ".env.local"] });
    // Database: Empty | Copy of main's.
    ([...db.querySelectorAll("button")].find((b) => b.textContent === "Copy of main’s") as HTMLButtonElement).click();
    await wait(60);
    expect(calls("set_app_hooks").at(-1)![1]).toEqual({ site: "shop", field: "copy_database", value: true });
    // Env keys: a plain chooser, no placeholders to learn; Custom… takes free text.
    const keysBox = () => [...box.querySelectorAll(".hk")][2] as HTMLElement;
    const sels = [...keys.querySelectorAll("select")] as HTMLSelectElement[];
    expect(sels.map((s) => s.selectedOptions[0].textContent)).toEqual([
      "This worktree’s address",
      "This worktree’s database",
      "This worktree’s database",
    ]);
    sels[0].value = "{prefix}";
    sels[0].dispatchEvent(new Event("change"));
    await wait(60);
    expect(calls("set_app_hooks").at(-1)![1]).toEqual({ site: "shop", field: "keys", value: { APP_URL: "{prefix}", DB_DATABASE: "{db}" } });
    const add = keysBox().querySelector(".hk-add")!;
    (add.querySelector("input.hk-key") as HTMLInputElement).value = "A";
    const sel = add.querySelector("select") as HTMLSelectElement;
    sel.value = "";
    sel.dispatchEvent(new Event("change"));
    const custom = add.querySelector(".hk-custom") as HTMLInputElement;
    expect(custom.hidden).toBe(false);
    custom.value = "{nope}";
    w.mockHooksFail = "A: {nope} isn't one of {db}, {url}, {prefix}, {port}, {name}";
    (add.querySelector(".inline-action") as HTMLButtonElement).click();
    await wait(60);
    expect(calls("set_app_hooks").at(-1)![1]).toEqual({
      site: "shop",
      field: "keys",
      value: { APP_URL: "{url}", DB_DATABASE: "{db}", A: "{nope}" },
    });
    // Refused: said in its row, the typed value kept.
    expect(keysBox().querySelector(".error-text")!.textContent).toContain("isn't one of");
    expect(custom.value).toBe("{nope}");
    delete w.mockHooksFail;
    (keysBox().querySelector(".hk-row .hk-x") as HTMLButtonElement).click();
    await wait(60);
    expect(calls("set_app_hooks").at(-1)![1]).toEqual({ site: "shop", field: "keys", value: { DB_DATABASE: "{db}" } });
    // After create: {install} offered as Install packages (the lock file's command).
    expect(after.textContent).not.toContain("{install}");
    ([...after.querySelectorAll("button")].find((b) => b.textContent === "Add Package Install") as HTMLButtonElement).click();
    await wait(60);
    expect(calls("set_app_hooks").at(-1)![1]).toEqual({ site: "shop", field: "after_create", value: ["composer install", "{install}"] });
    btn("Use preset").click();
    await wait(60);
    expect(calls("set_app_hooks").at(-1)![1]).toEqual({ site: "shop", field: null, value: null });
    btn("Done").click();
    await wait(60);
    expect(box.querySelectorAll(".row").length).toBe(1);
  });

  test("hooks said plainly: {install} as the lock file's command, left out without one", async () => {
    const { summary, command } = await import("../src/hooksform");
    expect(command("{install}", "pnpm install")).toBe("Install packages (pnpm install)");
    expect(command("{install}")).toBe("Install packages (the lock file’s install)");
    const h = { copy: [], copy_database: true, keys: {}, after_create: ["{install}", "php artisan migrate"] };
    expect(summary(h, null)).toEqual([
      "Copies no files",
      "Its own database, a copy of main’s",
      "Sets no env keys",
      "Then offers to run php artisan migrate",
    ]);
    expect(summary(h, "npm ci").at(-1)).toBe("Then offers to run Install packages (npm ci), php artisan migrate");
  });

  test("Remove Worktree… says what's uncommitted, offers to delete its database, and removes anyway", async () => {
    await show();
    $(`#list [data-name="${wt}"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait(80);
    const texts = (w.mockMenu as any[]).map((i) => i.text);
    expect(texts).not.toContain("New Worktree…");
    (w.mockMenu as any[]).find((i) => i.text === "Remove Worktree…").action();
    await wait(60);
    const pop = $(".pop-confirm");
    expect(pop.textContent).toContain("3 uncommitted changes");
    expect(pop.textContent).toContain("Also delete shop_feature_x");
    const yes = $(".pop-confirm .primary") as HTMLButtonElement;
    expect(yes.textContent).toBe("Remove anyway");
    expect(calls("worktree_remove").length).toBe(0);
    ($(".pop-confirm input[type=checkbox]") as HTMLInputElement).checked = true;
    w.mockDelay = { worktree_remove: 120 };
    yes.click();
    await wait(40);
    // While it works the row says so, not only once the folder is gone.
    expect($(`#list [data-name="${wt}"] .removing`).textContent).toBe("Removing…");
    await wait(150);
    w.mockDelay = {};
    expect(calls("worktree_remove").at(-1)![1]).toEqual({ site: wt, dropDb: true, force: true });
    expect($(`#list [data-name="${wt}"] .removing`)).toBeNull();
  });

  test("Remove Worktree… names the ignored data that goes with the folder", async () => {
    w.mockGitClean = true;
    w.mockLosses = ["database/database.sqlite", ".env"];
    try {
      await show({ copies: [] });
      $(`#list [data-name="${wt}"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
      await wait(80);
      (w.mockMenu as any[]).find((i) => i.text === "Remove Worktree…").action();
      await wait(60);
      expect(calls("worktree_losses").at(-1)![1]).toEqual({ site: wt });
      const said = [...document.querySelectorAll(".pop-confirm p")].map((p) => p.textContent);
      expect(said[1]).toBe("These will be deleted with the folder: database/database.sqlite, .env.");
      expect($(".pop-confirm .primary").textContent).toBe("Remove");
    } finally {
      delete w.mockGitClean;
      delete w.mockLosses;
    }
  });

  test("Remove Worktree… on a clean worktree without a copy: just Remove", async () => {
    w.mockGitClean = true;
    try {
      await show({ copies: [] });
      $(`#list [data-name="${wt}"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
      await wait(80);
      (w.mockMenu as any[]).find((i) => i.text === "Remove Worktree…").action();
      await wait(60);
      expect($(".pop-confirm .primary").textContent).toBe("Remove");
      expect($(".pop-confirm input[type=checkbox]")).toBeNull();
      ($(".pop-confirm .primary") as HTMLButtonElement).click();
      await wait(60);
      expect(calls("worktree_remove").at(-1)![1]).toEqual({ site: wt, dropDb: false, force: false });
    } finally {
      delete w.mockGitClean;
    }
  });

  test("Remove Worktree… refused (commits on no branch): says why in place, then Remove anyway", async () => {
    w.mockGitClean = true;
    w.mockRemoveFail = `${wt} has commits on no branch, lost with the worktree`;
    try {
      await show({ copies: [] });
      $(`#list [data-name="${wt}"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
      await wait(80);
      (w.mockMenu as any[]).find((i) => i.text === "Remove Worktree…").action();
      await wait(60);
      ($(".pop-confirm .primary") as HTMLButtonElement).click();
      await wait(60);
      expect($(".pop-confirm").textContent).toContain("has commits on no branch, lost with the worktree");
      const yes = $(".pop-confirm .primary") as HTMLButtonElement;
      expect(yes.textContent).toBe("Remove anyway");
      yes.click();
      await wait(60);
      expect(calls("worktree_remove").at(-1)![1]).toEqual({ site: wt, dropDb: false, force: true });
    } finally {
      delete w.mockGitClean;
      delete w.mockRemoveFail;
    }
  });
});

describe("sidebar worktrees", () => {
  const mk = (slug: string, branch: string) =>
    w.mockSite(`shop--${slug}`, "laravel", {
      worktree_of: "shop",
      branch,
      path: `/Users/x/Sites/shop--${slug}`,
      aliases: [`admin--${slug}.test`],
    });
  let saved: any[];
  const refresh = async () => {
    await (await import("../src/app")).call("state");
    await wait(60);
  };
  beforeAll(async () => {
    saved = w.mockSnap.sites;
    w.mockSnap.sites = [...saved.filter((s) => !s.worktree_of), mk("feature-login", "feature/login"), mk("agent-1a2b", "agent/1a2b")];
    w.mockSnap.app_states = { "shop--feature-login": "running" };
    await refresh();
  });

  test("aliases leave the list; worktrees nest under their app with a branch mark", () => {
    // An alias is no row: its app's header lists it.
    expect($(`#list [data-name="admin.shop"]`)).toBeNull();
    expect($$("#list .kids .item").map((r) => r.dataset.name)).toEqual(["shop--agent-1a2b", "shop--feature-login"]);
    const kids = $$("#list .kids .item.child");
    expect(kids.map((k) => k.querySelector(".name")!.textContent)).toEqual(["agent/1a2b", "feature/login"]);
    // A bare branch glyph, no mark of an app's; green while it runs.
    expect(kids.map((k) => !!k.querySelector(".mark"))).toEqual([false, false]);
    expect(kids.map((k) => k.querySelector(".wt-glyph")!.classList.contains("on"))).toEqual([false, true]);
    const disc = $(`#list [data-name="shop"] .disclosure`);
    expect(disc.querySelector("svg")).toBeTruthy();
    expect(disc.querySelector(".count")!.textContent).toBe("2");
  });

  test("the header chip still lists an app's aliases, and a worktree's derived hosts", async () => {
    await select("shop");
    const chip = $("#site .alias-chip");
    expect(chip.textContent).toBe("+2");
    chip.click();
    expect($$(".alias-pop .alias-url").map((e) => e.textContent!.replace(/^https?:\/\//, ""))).toEqual([
      "shop.test",
      "admin.shop.test",
      "api.shop.test",
    ]);
    document.querySelector(".pop")?.remove();
    await select("shop--feature-login");
    $("#site .alias-chip").click();
    expect($$(".alias-pop .alias-url").map((e) => e.textContent)).toContain("admin--feature-login.test");
    document.querySelector(".pop")?.remove();
  });

  test("a folder or Compose project shows a grey folder or stack where an address's lock goes", async () => {
    expect($(`#list [data-name="notes"] .name .lock`)!.className).toBe("lock none");
    expect($(`#list [data-name="stack"] .name .lock`)!.className).toBe("lock none");
  });

  test("Logs and Packages are hidden while there's nothing in them", async () => {
    const [logs, pkgs] = [w.mockLogSources, w.mockPackages];
    try {
      w.mockLogSources = [];
      w.mockPackages = { ...pkgs, ecosystems: [] };
      await select("notes");
      await select("shop");
      await wait(60);
      expect($('#site .tabs [data-tab="logs"]').hidden).toBe(true);
      expect($('#site .tabs [data-tab="packages"]').hidden).toBe(true);
    } finally {
      w.mockLogSources = logs;
      w.mockPackages = pkgs;
      await select("notes");
      await select("shop");
      await wait(60);
    }
    expect($('#site .tabs [data-tab="logs"]').hidden).toBe(false);
    expect($('#site .tabs [data-tab="packages"]').hidden).toBe(false);
  });

  test("Overview › Aliases: a group of its own, a row per address that opens it, Remove confirmed in the row; a worktree's without Remove", async () => {
    const before = w.mockSnap.sites;
    try {
      await select("shop");
      const box = $('[data-panel="overview"] .aliases');
      expect(box.hidden).toBe(false);
      const rows = [...box.querySelectorAll<HTMLElement>(".row")];
      expect(rows.map((r) => r.querySelector(".alias-go")!.textContent)).toEqual(["admin.shop.test", "api.shop.test"]);
      // A green closed lock for HTTPS, a red open one for plain HTTP (api.shop).
      expect(rows.map((r) => r.querySelector(".lock")!.className)).toEqual(["lock on", "lock off"]);
      (rows[0].querySelector(".alias-go") as HTMLElement).click();
      expect(calls("open_url").at(-1)![1]).toEqual({ url: "https://admin.shop.test" });
      expect((box.querySelector(".aliases-add") as HTMLElement).hidden).toBe(false);
      (rows[0].querySelector(".alias-remove") as HTMLElement).click();
      expect(calls("remove_site").length).toBe(0);
      expect(rows[0].querySelector("dd")!.textContent).toBe("Remove this address? The folder stays.CancelRemove");
      expect($(".pop")).toBeNull();
      (rows[0].querySelectorAll(".alias-remove")[0] as HTMLElement).click();
      expect(rows[0].querySelector("dd")!.textContent).toBe("Remove");
      (rows[0].querySelector(".alias-remove") as HTMLElement).click();
      (rows[0].querySelector(".alias-remove.danger") as HTMLElement).click();
      await wait(60);
      expect(calls("remove_site").at(-1)![1]).toEqual({ site: "admin.shop" });
      await select("shop--feature-login");
      const wt = $('[data-panel="overview"] .aliases');
      expect(wt.querySelectorAll(".alias-remove").length).toBe(0);
      expect((wt.querySelector(".aliases-add") as HTMLElement).hidden).toBe(true);
      expect(wt.querySelector(".aliases-none")!.textContent).toBe("Added and removed on shop");
      (wt.querySelector(".aliases-none .app-link") as HTMLElement).click();
      await wait(60);
      expect($(`#list [data-name="shop"]`).getAttribute("aria-current")).toBe("true");
    } finally {
      w.mockSnap.sites = before;
      await refresh();
    }
  });

  test("a linked app keeps its worktrees under it, and its alias is no row of its own", async () => {
    const before = w.mockSnap.sites;
    const p = "/Users/x/Code/api";
    w.mockSnap.sites = [
      ...before,
      w.mockSite("api", "laravel", { linked: true, parked_in: null, path: p }),
      w.mockSite("backend", "laravel", { linked: true, parked_in: null, path: p }),
      w.mockSite("api--x", "laravel", {
        worktree_of: "api",
        branch: "x",
        parked_in: null,
        path: "/Users/x/Code/api-wt/x",
        aliases: ["backend--x.test"],
      }),
    ];
    try {
      await refresh();
      expect($(`#list [data-name="backend"]`)).toBeNull();
      const kids = $(`#list [data-name="api"]`).nextElementSibling!;
      expect([...kids.querySelectorAll<HTMLElement>(".item")].map((r) => r.dataset.name)).toEqual(["api--x"]);
      await select("api");
      expect($("#site .alias-chip").textContent).toBe("+1");
    } finally {
      w.mockSnap.sites = before;
      await refresh();
    }
  });

  test("a worktree whose app is gone still shows, as a plain row with its branch", async () => {
    w.mockSnap.sites = [
      ...w.mockSnap.sites,
      w.mockSite("lost--fix", "laravel", { worktree_of: "lost", branch: "fix/it", path: "/Users/x/Sites/lost--fix" }),
    ];
    await refresh();
    const r = $(`#list [data-name="lost--fix"]`);
    expect(r.classList.contains("child")).toBe(false);
    expect(r.querySelector(".name")!.textContent).toBe("fix/it");
    w.mockSnap.sites = w.mockSnap.sites.filter((s: any) => s.name !== "lost--fix");
  });

  test("more than five worktrees start collapsed", async () => {
    localStorage.removeItem("wt-folded");
    localStorage.removeItem("collapsed");
    w.mockSnap.sites = [...w.mockSnap.sites, ...[1, 2, 3, 4].map((n) => mk(`b${n}`, `b${n}`))];
    await refresh();
    expect($(`#list [data-name="shop"] + .kids`).classList.contains("closed")).toBe(true);
    expect($(`#list [data-name="shop"] .count`).textContent).toBe("6");
  });
});

describe("agent activity", () => {
  const at = (h: number, m: number) => new Date(2026, 9, 19, h, m).getTime() / 1000;
  const act = (what: string, h: number, m: number, more = {}) => ({
    at: at(h, m),
    client: "Claude Code",
    site: "vuefinder",
    what,
    ok: true,
    detail: null,
    ...more,
  });
  const lines = () => $$("#site .agents .row").map((r) => r.querySelector(".agent-line")!.textContent);
  const refreshed = async () => {
    await wait(550); // refresh reads the state again only half a second after the last read
    w.refresh();
    await wait(80);
  };

  test("overview_lists_agent_actions: that app's newest, through the existing refresh", async () => {
    await select("vuefinder");
    w.mockAgents = [act("restarted api", 14, 2), act("started queue", 13, 58), act("stopped dev", 13, 40, { client: "Terminal" })];
    await refreshed();
    expect($("#site .agents").hidden).toBe(false);
    expect($("#site .agents .res-sub").textContent).toBe("Agents");
    expect(lines()).toEqual(["Claude Code restarted api · 14:02", "Claude Code started queue · 13:58", "Terminal stopped dev · 13:40"]);
    expect(calls("agent_activity").at(-1)![1]).toEqual({ site: "vuefinder" });
    w.mockAgents = [...w.mockAgents, ...[1, 2, 3].map((n) => act(`started p${n}`, 12, n))];
    await refreshed();
    expect(lines()).toHaveLength(5);
  });

  test("overview_hides_agents_when_empty", async () => {
    w.mockAgents = [];
    await refreshed();
    expect($("#site .agents").hidden).toBe(true);
  });

  test("failed_action_shows_detail: inline, in the error colour", async () => {
    w.mockAgents = [act("restarted api", 14, 2, { ok: false, detail: "No process api" })];
    await refreshed();
    const d = $("#site .agents .row .agent-detail");
    expect([d.textContent, d.classList.contains("error-text")]).toEqual(["No process api", true]);
    w.mockAgents = [];
  });
});

describe("browser errors", () => {
  const box = () => $("#app-browser") as HTMLInputElement;
  const line = () => $('[data-panel="overview"] .browser-line');

  test("a PHP app's and a dev server's Settings have the switch, off; a Compose app's and a folder's none", async () => {
    for (const name of ["stack", "notes"]) {
      await select(name);
      expect($("#app-browser")).toBeNull();
    }
    await select("vuefinder");
    expect(box().checked).toBe(false);
    await select("shop");
    expect(box().checked).toBe(false);
    expect(line().hidden).toBe(true);
    expect($(".browser-blocked").hidden).toBe(true);
  });

  test("on: the Overview's Browser line counts the errors since the first", async () => {
    w.mockBrowser = { on: true, errors: 3, since: 1792504953, blocked: false };
    box().checked = true;
    box().dispatchEvent(new Event("change"));
    await wait(80);
    expect(calls("set_browser").at(-1)![1]).toEqual({ site: "shop", on: true });
    expect(box().checked).toBe(true);
    const hm = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit" }).format(new Date(1792504953000));
    expect(line().hidden).toBe(false);
    expect($('[data-panel="overview"] .browser-line dd').textContent).toBe(`3 errors since ${hm}`);
  });

  test("an app whose CSP keeps the script out says so in Settings and on the Overview", async () => {
    w.mockBrowser = { on: true, errors: 0, since: null, blocked: true };
    for (const on of [false, true]) {
      box().checked = on;
      box().dispatchEvent(new Event("change"));
      await wait(80);
    }
    expect(line().hidden).toBe(false);
    expect($(".browser-blocked").hidden).toBe(false);
    expect($('[data-panel="overview"] .browser-line dd').textContent).toBe("Blocked by the app’s Content-Security-Policy");
    w.mockBrowser = undefined;
  });
});

describe("hide", () => {
  test("Hide from Anvil leaves a slim row in place with Undo, no toast; Undo brings the app back", async () => {
    $('.modes [data-mode="apps"]').click();
    await wait(40);
    $(`#list [data-name="notes"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait(80);
    (w.mockMenu as any[]).find((i) => i.text === "Hide from Anvil").action();
    await wait(80);
    expect(calls("remove_site").at(-1)![1]).toEqual({ site: "notes" });
    const row = $('#list .hid-row[data-name="notes"]');
    expect(row.textContent).toBe("notes hidden·Undo");
    expect($('#list .item[data-name="notes"]')).toBeNull();
    expect($(".toast")).toBeNull();
    (row.querySelector("button") as HTMLElement).click();
    await wait(80);
    expect(calls("unhide_site").at(-1)![1]).toEqual({ site: "notes" });
    expect($("#list .hid-row")).toBeNull();
    expect($('#list .item[data-name="notes"]')).not.toBeNull();
  });
});

describe("errors where they happen", () => {
  test("an app's failed action shows in its header, a service's on its page; the banner only for the rest", async () => {
    $('.modes [data-mode="apps"]').click();
    await select("shop");
    w.mockFail = { set_secure: "Caddy didn't reload" };
    $('#list [data-name="shop"]').dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait(80);
    (w.mockMenu as any[]).find((i) => i.text === "Use HTTPS").action();
    await wait(120);
    expect($("#site .run-error").textContent).toBe("Caddy didn't reload");
    expect($("#site .run-error").hidden).toBe(false);
    expect($("#error").hidden).toBe(true);
    // Another app: its own header, without shop's error.
    await select("vuefinder");
    expect($("#site .run-error").hidden).toBe(true);

    w.mockFail = { service_run: "Port 5432 is in use" };
    $('.modes [data-mode="services"]').click();
    await wait(40);
    $('#list [data-name="postgres"]').click();
    await wait(80);
    ($('#site [data-svc="start"]') as HTMLElement).click();
    await wait(80);
    expect($("#site .run-error").textContent).toBe("Port 5432 is in use");
    expect($("#error").hidden).toBe(true);
    w.mockFail = null;
    ($('#site [data-svc="start"]') as HTMLElement).click();
    await wait(80);
    expect($("#site .run-error").hidden).toBe(true);
    $('.modes [data-mode="apps"]').click();
    await wait(40);

    // An error with no place: the banner, dismissible.
    const app = await import("../src/app");
    app.showError("Something else");
    expect($("#error").hidden).toBe(false);
    $("#error-close").click();
    expect($("#error").hidden).toBe(true);
  });
});
