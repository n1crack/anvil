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
  test("one button sets everything up, then trusts the certificate if macOS didn't", async () => {
    const buttons = $$("#setup button").map((b) => b.textContent);
    expect(buttons).toEqual(["Set up Anvil"]);
    w.mockSetup.trusted = false;
    $("#setup .setup-go").click();
    await wait(80);
    expect(calls("setup_fix").map(([, a]) => a.step)).toEqual(["all", "trust"]);
    expect($("#setup").hidden).toBe(true);
  });
});

describe("commands", () => {
  test("a row click doesn't run; ▶ asks first, Run runs", async () => {
    await select("vuefinder");
    const row = $$(".proc.cmd").find((r) => r.querySelector(".proc-name")?.textContent === "build")!;
    row.click();
    await wait();
    expect(calls("command_run")).toHaveLength(0);
    (row.querySelector('button[aria-label^="Run"]') as HTMLElement).click();
    const pop = $(".pop-confirm");
    expect(pop.textContent).toContain("Run npm run build?");
    (pop.querySelector(".primary") as HTMLElement).click();
    await wait();
    expect(calls("command_run").map(([, a]) => a.script)).toEqual(["build"]);
  });
});

describe("processes", () => {
  test("a crashed row offers Restart and ■ to dismiss it", async () => {
    await select("shop");
    const logs = $$(".proc").find((r) => r.querySelector(".proc-name")?.textContent === "logs")!;
    expect(logs.classList.contains("crashed")).toBe(true);
    const labels = [...logs.querySelectorAll(".proc-actions button")].map((b) => b.getAttribute("aria-label"));
    expect(labels).toContain("Restart logs");
    expect(labels).toContain("Dismiss logs");
  });

  test("open in the browser is an outline globe, not an emoji", async () => {
    await select("shop");
    const b = $('[aria-label="Open shop.test in the browser"]');
    expect(b.querySelector("svg")).not.toBeNull();
    expect(b.textContent).toBe("");
    expect(b.title).toBe("https://shop.test");
  });
});

describe("https", () => {
  test("the row shows a spinner while HTTPS is turned on or off", async () => {
    w.mockDelay = { set_secure: 200 };
    $(`#list [data-name="shop"]`).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait(80);
    const item = (w.mockMenu as any[]).find((i) => i.text === "HTTPS");
    item.action();
    await wait(40);
    expect($(`#list [data-name="shop"] .spin-dot`)).toBeTruthy();
    await wait(300);
    expect($(`#list [data-name="shop"] .spin-dot`)).toBeNull();
    w.mockDelay = {};
  });
});

describe("watch", () => {
  const row = (name: string) => $$(".procs .proc").find((r) => r.querySelector(".proc-name")?.textContent === name)!;
  const watchTab = () => $('.tabs [data-tab="watch"]');
  const chosen = () => $(".watch-item[aria-selected=true]")?.dataset.key;

  test("a command row click still shows its last output in the console", async () => {
    await select("vuefinder");
    row("test").click();
    await wait(80);
    expect($(".console").hidden).toBe(false);
    expect($(".console-out span.ansi2")).not.toBeNull();
    ($(".console-close") as HTMLElement).click();
  });

  test("a stopped row click doesn't open the console nor read its output", async () => {
    const before = calls("log_text").length;
    row("docs:site").click();
    await wait();
    expect(calls("log_text")).toHaveLength(before);
    expect($(".console").hidden).toBe(true);
    expect(row("docs:site").querySelector('[aria-label^="Watch"]')).toBeNull();
  });

  test("All is the default and merges the processes' lines, each tagged with its name", async () => {
    w.mockLogs = { app: "server up\n", "proc:docs-site": "" };
    expect(watchTab().hidden).toBe(false);
    watchTab().click();
    await wait(80);
    expect($('[data-panel="watch"]').hidden).toBe(false);
    expect(chosen()).toBe("all");
    const lines = $$(".watch-line").map((l) => l.textContent);
    expect(lines).toContain("dev       │ server up");
    expect(lines.some((l) => l!.startsWith("docs:dev  │ ✓ built in"))).toBe(true);
    expect($(".watch-line .watch-tag.ansi4")).not.toBeNull();
  });

  test("Watch on a running row opens the tab on that process, with its colored output", async () => {
    $('.tabs [data-tab="overview"]').click();
    const watch = row("docs:dev").querySelector('[aria-label="Watch docs:dev"]') as HTMLElement;
    const actions = [...row("docs:dev").querySelectorAll(".proc-actions button")].map((b) => b.textContent);
    expect(actions.indexOf("Watch")).toBe(actions.indexOf("Restart") - 1);
    watch.click();
    await wait(80);
    expect(watchTab().getAttribute("aria-selected")).toBe("true");
    expect(chosen()).toBe("proc:docs-dev");
    expect($(".watch-head .console-title").textContent).toBe("docs:dev");
    expect($(".watch-head .watch-cmd").textContent).toBe("npm run docs:dev");
    expect($(".watch-out span.ansi2")).not.toBeNull();
    expect($(".watch-out").textContent).toContain("built in 1.2s");
  });

  test("Restart in the pane restarts that process", async () => {
    const before = calls("process_run").length;
    ($('.watch-head [aria-label="Restart docs:dev"]') as HTMLElement).click();
    await wait();
    expect(
      calls("process_run")
        .slice(before)
        .map(([, a]) => a),
    ).toEqual([{ site: "vuefinder", name: "docs-dev", action: "restart" }]);
  });

  test("↓ moves the selection; a stopped process without output says so", async () => {
    $(".watch-list").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await wait(80);
    expect(chosen()).toBe("proc:docs-site");
    expect($(".watch-out").dataset.empty).toBe("Not running");
    expect($('.watch-head [aria-label="Start docs:site"]')).not.toBeNull();
  });

  test("an app without processes has no Watch tab", async () => {
    await select("notes");
    expect(watchTab().hidden).toBe(true);
    expect($('.tabs [data-tab="overview"]').getAttribute("aria-selected")).toBe("true");
    w.mockLogs = null;
  });
});

describe("audit fixes", () => {
  test("a folder shows no lock nor 'Runs on', even with an old HTTPS flag", async () => {
    await select("notes");
    expect($('#list [data-name="notes"] .lock')).toBeNull();
    expect($(`#list [data-name="vuefinder"] .lock`)).not.toBeNull();
    expect($('[data-field="runtime"]')).toBeNull();
    expect($('[data-field="path"]').textContent).toBe("~/Sites/notes");
  });

  test("a running command has ■ Stop, which calls once_stop without opening the console", async () => {
    await select("vuefinder");
    const row = $$(".proc.cmd").find((r) => r.querySelector(".proc-name")?.textContent === "test")!;
    const stop = row.querySelector('[aria-label="Stop npm run test"]') as HTMLElement;
    expect(stop.textContent).toBe("■ Stop");
    expect(row.querySelector('[aria-label="Run test"]')).toBeNull();
    stop.click();
    await wait();
    expect(calls("once_stop").at(-1)![1]).toEqual({ site: "vuefinder", id: "test" });
    expect($(".console").hidden).toBe(true);
  });

  test("adding a folder or creating an app selects it without a toast", async () => {
    const open = w.__TAURI__.dialog.open;
    w.__TAURI__.dialog.open = async () => "/Users/x/Sites/notes";
    await select("shop");
    ($("#new-existing") as HTMLElement).click();
    await wait(80);
    w.__TAURI__.dialog.open = open;
    expect($('#list [data-name="notes"]').getAttribute("aria-current")).toBe("true");
    expect($("#toast").hidden).toBe(true);

    await select("shop");
    ($("#new-name") as HTMLInputElement).value = "notes";
    const parent = $("#new-parent") as HTMLSelectElement;
    parent.append(new w.Option("~/Sites", "/Users/x/Sites"));
    parent.value = "/Users/x/Sites";
    $("#new-form").dispatchEvent(new Event("submit", { cancelable: true }));
    await wait(80);
    expect(calls("create_app")).toHaveLength(1);
    expect($('#list [data-name="notes"]').getAttribute("aria-current")).toBe("true");
    expect($("#toast").hidden).toBe(true);
  });

  test("Copy says Copied on the button for a moment, not in a toast", async () => {
    $('.modes [data-mode="services"]').click();
    await wait(80);
    const copy = $(".svc-copy");
    copy.click();
    await wait();
    expect(copy.textContent).toBe("Copied");
    expect($("#toast").hidden).toBe(true);
    await wait(1600);
    expect(copy.textContent).toBe("Copy");
    $('.modes [data-mode="apps"]').click();
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

describe("packages", () => {
  test("All until a check exists, no check on opening; then Outdated with a tab per manager", async () => {
    $('.tabs [data-tab="packages"]').click();
    await wait(80);
    expect(calls("package_updates")).toHaveLength(0);
    expect($('.pkg-tools .seg [aria-pressed="true"]').dataset.id).toBe("all");
    expect($$(".pkg-ecos button").map((b) => b.textContent)).toEqual(["Composer 16", "pnpm 8"]);
    expect($(".pkg-status").textContent).toBe("");
    $('.pkg-tools .seg [data-id="outdated"]').click();
    expect($(".pkg-none").textContent).toBe("Not checked yet");
    expect($$(".pkg-check")).toHaveLength(1);
    expect($$(".pkg-ecos button").map((b) => b.textContent)).toEqual(["Composer 16", "pnpm 8"]);
    ($(".pkg-none .pkg-check") as HTMLElement).click();
    await wait(80);
    expect(calls("package_updates")).toHaveLength(1);
    expect($(".pkg-status").textContent).toBe("checked just now");
    expect(JSON.parse(localStorage.getItem("pkgUpdates:shop")!).list).toHaveLength(7);
    expect($$(".pkg-ecos button").map((b) => b.textContent)).toEqual(["Composer 4", "pnpm 3"]);
    expect($('.pkg-tools .seg [data-id="outdated"]').textContent).toBe("Outdated 4");
    const rows = $$(".pkg-list .row");
    expect(rows).toHaveLength(4);
    const fw = rows.find((r) => r.querySelector("a")?.textContent === "laravel/framework")!;
    expect(fw.querySelector(".pkg-ver")?.textContent).toBe("11.36.1→12.4.0");
    expect(fw.querySelector(".pkg-new")?.classList.contains("major")).toBe(true);
    ($('.pkg-ecos [data-id="npm"]') as HTMLElement).click();
    expect($$(".pkg-list .row")).toHaveLength(3);
    $('.tabs [data-tab="logs"]').click();
    $('.tabs [data-tab="packages"]').click();
    await wait(80);
    expect(calls("package_updates")).toHaveLength(1);
  });

  test("a saved check makes Outdated the default after a restart", async () => {
    const { pkgUpdates, pkgView, drawPackages } = await import("../src/packages");
    pkgUpdates.clear();
    pkgView.clear();
    drawPackages("shop");
    expect($('.pkg-tools .seg [aria-pressed="true"]').dataset.id).toBe("outdated");
    expect($(".pkg-status").textContent).toBe("checked just now");
    expect(calls("package_updates")).toHaveLength(1);
  });
});
