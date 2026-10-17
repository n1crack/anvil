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
    $("#new-done").click();
    await wait();
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
  test("repeats fold into one row with ×N; open, it lists each time and folds vendor frames", async () => {
    await select("shop");
    $('.tabs [data-tab="logs"]').click();
    await wait(80);
    const first = $$(".logs-list .log")[0];
    expect(first.querySelector(".log-count")?.textContent).toBe("×4");
    expect(first.querySelector(".log-level")?.textContent).toBe("error");
    expect(first.querySelector(".log-dot")).not.toBeNull();
    (first.querySelector(".log-head") as HTMLElement).click();
    expect(first.querySelector(".log-meta")?.textContent).toMatch(/^4 times: /);
    const trace = first.querySelector(".log-trace")!;
    expect(trace.classList.contains("folded")).toBe(true);
    expect([...trace.querySelectorAll(".own .log-file")].map((f) => f.textContent)).toEqual([
      "app/Http/Controllers/CheckoutController.php:42",
      "app/Http/Middleware/EnsureCartNotEmpty.php:23",
      "app/Providers/RouteServiceProvider.php:51",
    ]);
    const more = first.querySelector(".log-vendor") as HTMLElement;
    expect(more.textContent).toBe("+56 vendor frames");
    more.click();
    expect(trace.classList.contains("folded")).toBe(false);
    (trace.querySelector("button.log-file") as HTMLElement).click();
    expect(calls("open_in_editor").at(-1)![1]).toEqual({ site: "shop", path: "app/Http/Controllers/CheckoutController.php", line: 42 });
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
});

describe("packages", () => {
  test("a list per manager; Outdated shows current → newest, checked once an hour", async () => {
    $('.tabs [data-tab="packages"]').click();
    await wait(80);
    expect(calls("package_updates")).toHaveLength(1);
    expect($$(".pkg-head > span:first-child").map((h) => h.textContent)).toEqual(["Composer", "pnpm"]);
    expect($(".pkg-status").textContent).toBe("7 updates · 3 major · checked just now");
    expect($$(".pkg-dev").length).toBe(14);
    $('.pkg-tools .seg [data-id="outdated"]').click();
    const rows = $$(".pkg-list .row");
    expect(rows).toHaveLength(7);
    const fw = rows.find((r) => r.querySelector("a")?.textContent === "laravel/framework")!;
    expect(fw.querySelector(".pkg-ver")?.textContent).toBe("11.36.1→12.4.0");
    expect(fw.querySelector(".pkg-new")?.classList.contains("major")).toBe(true);
    $('.tabs [data-tab="logs"]').click();
    $('.tabs [data-tab="packages"]').click();
    await wait(80);
    expect(calls("package_updates")).toHaveLength(1);
    $(".pkg-check").click();
    await wait(80);
    expect(calls("package_updates")).toHaveLength(2);
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
    expect(calls("create_app").at(-1)![1]).toMatchObject({ kind: "laravel", name: "my-app", parent: "/Users/x/Sites" });
    $("#new-done").click();
  });

  test("the address follows the typed name and the configured TLD", async () => {
    await openSheet();
    expect($("#new-url").textContent).toBe("Its address: my-app.test");
    fill("Shop Front");
    expect(($("#new-name") as HTMLInputElement).value).toBe("shop-front");
    expect($("#new-url").textContent).toBe("Its address: shop-front.test");
    $("#new-cancel").click();
  });

  test("Options show the chosen template's choices, summed up when closed, remembered per template", async () => {
    await openSheet();
    await pick("laravel");
    expect(($("#new-options") as HTMLDetailsElement).open).toBe(false);
    expect($("#new-opts-sum").textContent).toBe("No starter kit · SQLite · npm · PHP 8.4 · Git");
    const keys = () => $$("#new-opts [data-key]").map((x) => x.dataset.key);
    expect(keys()).toEqual(["kit", "db", "js", "php", "git"]);
    const kit = $('#new-opts select[data-key="kit"]') as HTMLSelectElement;
    kit.value = "livewire";
    kit.dispatchEvent(new Event("change"));
    expect($("#new-opts-sum").textContent).toBe("Livewire · SQLite · npm · PHP 8.4 · Git");

    await pick("next");
    expect(keys()).toEqual(["js", "node", "ts", "tailwind", "git"]);
    const tw = $('#new-opts input[data-key="tailwind"]') as HTMLInputElement;
    tw.checked = false;
    tw.dispatchEvent(new Event("change"));
    expect($("#new-opts-sum").textContent).toBe("TypeScript · npm · Node 24 · Git");

    await pick("flask");
    expect(keys()).toEqual(["git"]);

    await pick("laravel");
    expect($("#new-opts-sum").textContent).toContain("Livewire");
    fill("kit-app");
    submit();
    await wait(80);
    expect(calls("create_app").at(-1)![1].options).toEqual({ kit: "livewire", db: "sqlite", js: "npm", php: "8.4", git: "on" });
    $("#new-done").click();
    localStorage.removeItem("newOpts:laravel");
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
    expect($("#new-done").textContent).toBe("Open");
    expect($("#new-reveal").hidden).toBe(true);
    await select("shop");
    $("#new-done").click();
    await wait();
    expect($("#new-app").hasAttribute("open")).toBe(false);
    expect($('#list [data-name="notes"]').getAttribute("aria-current")).toBe("true");
  });

  test("a failure shows ✕, highlights the error in the log, and offers Close and Show in Finder", async () => {
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
});
