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

describe("overview", () => {
  test("one Dev line with a dot per process, and no process or command lists", async () => {
    await select("vuefinder");
    const line = $(".procs .dev-summary");
    expect(line.textContent).toContain("Dev");
    expect(line.querySelector(".dev-summary-state")!.textContent).toBe("1 of 3 running");
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

  test("the header's Start is Dev start: processes left out of it don't keep it shown", async () => {
    await select("vuefinder");
    const ov = w.mockOv.vuefinder;
    const before = structuredClone(ov.rows);
    for (const r of ov.rows) r.running = r.id !== "docs-site";
    ov.rows.find((r) => r.id === "docs-site").manual = true;
    await select("shop");
    await select("vuefinder");
    expect($('#site [data-all="start"]').hidden).toBe(true);
    expect($("#site .state-text").textContent).toBe("2 running");
    ov.rows = before;
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
    expect($(".dev-list .dev-divider")).not.toBeNull();
    expect(item("once:test").querySelector(".dot.on")).not.toBeNull();
    expect(item("once:preview").querySelector(".dev-exit")!.textContent).toBe("✓");
    const bad = item("once:format").querySelector(".dev-exit")!;
    expect([bad.textContent, bad.className]).toEqual(["1", "dev-exit bad"]);
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

  test("a right-click offers Stop, Restart, Open in Browser and Clear; Clear empties it", async () => {
    item("proc:docs-dev").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    await wait();
    const menu = w.mockMenu as any[];
    expect(menu.map((i) => i.text ?? i.item)).toEqual(["Stop", "Restart", "Open in Browser", "Separator", "Clear"]);
    menu.find((i) => i.text === "Clear").action();
    await wait();
    expect(calls("log_clear").at(-1)![1]).toEqual({ site: "vuefinder", source: "proc:docs-dev" });
    expect(lines()).toEqual([]);
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

  test("Run ▾ lists the one-off scripts only, and runs one after a confirm", async () => {
    $(".dev-bar .dev-run").click();
    const names = $$(".dev-run-menu .dev-run-name").map((n) => n.textContent);
    expect(names).toEqual(w.mockOv.vuefinder.commands.map((c) => c.name));
    expect(names).not.toContain("docs:dev");
    const build = $$(".dev-run-item").find((b) => b.querySelector(".dev-run-name")!.textContent === "build")!;
    expect(build.querySelector(".dev-run-cmd")!.textContent).toBe("vite build");
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

  test("Configure: edit a command, leave one out of Dev start, add a process, reset", async () => {
    $(".dev-bar .dev-configure").click();
    await wait();
    expect($(".dev-main").hidden).toBe(true);
    const names = () => $$(".dev-conf-table .dev-conf-name").map((n) => n.textContent);
    expect(names()).toEqual(["dev", "docs:dev", "docs:site"]);
    const box = (aria: string) => $(`.dev-conf [aria-label="${aria}"]`) as HTMLInputElement;
    expect(box("dev in Dev start").disabled).toBe(true);
    expect($('.dev-conf [aria-label="Remove dev"]')).toBeNull();

    const cmd = box("Command for docs:dev");
    cmd.value = "npm run docs:dev -- --open";
    cmd.dispatchEvent(new Event("change"));
    await wait();
    expect(calls("set_process").at(-1)![1]).toEqual({ site: "vuefinder", id: "docs-dev", command: "npm run docs:dev -- --open" });

    box("docs:site in Dev start").click();
    await wait();
    expect(calls("set_process").at(-1)![1]).toEqual({ site: "vuefinder", id: "docs-site", manual: true });
    expect(box("docs:site in Dev start").checked).toBe(false);

    // An empty command is refused inline, not in a toast.
    const empty = box("Command for docs:site");
    empty.value = " ";
    empty.dispatchEvent(new Event("change"));
    await wait();
    expect($(".dev-conf-error").textContent).toBe("The command can't be empty");
    expect($("#toast").hidden).toBe(true);

    $$(".dev-conf-foot button")
      .find((b) => b.textContent === "+ Add process")!
      .click();
    const n = box("Name of the new process");
    const c = box("Command of the new process");
    n.value = "worker";
    n.dispatchEvent(new Event("input"));
    c.value = "node worker.js";
    c.dispatchEvent(new Event("input"));
    c.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await wait();
    expect(calls("process_add").at(-1)![1]).toEqual({ site: "vuefinder", name: "worker", command: "node worker.js" });
    expect(names()).toEqual(["dev", "docs:dev", "docs:site", "worker"]);
    expect($(".dev-conf-draft")).toBeNull();

    const reset = $$(".dev-conf-foot button").find((b) => b.textContent === "Reset to detected")!;
    reset.click();
    expect(calls("processes_reset")).toHaveLength(0);
    ($(".pop-confirm .primary") as HTMLElement).click();
    await wait();
    expect(calls("processes_reset")).toHaveLength(1);
    expect(names()).toEqual(["dev", "docs:dev", "docs:site"]);
    expect(box("docs:site in Dev start").checked).toBe(true);

    $$(".dev-conf-foot button")
      .find((b) => b.textContent === "Done")!
      .click();
    await wait();
    expect($(".dev-main").hidden).toBe(false);
    expect($(".dev-conf").hidden).toBe(true);
  });

  test("keys act with the focus in the list or output, never in a field nor with ⌘", async () => {
    const key = (at: HTMLElement, k: string, o = {}) => at.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, ...o }));
    const runs = (from: number) => calls("process_run").slice(from).map(([, a]) => [a.name, a.action]);
    const list = $(".dev-list");
    const out = $(".dev-out");
    expect($(".dev-keys").textContent).toBe("↑↓ select · s start · x stop · r restart · q stop all · c clear");
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
    await select("shop");
    expect(item("proc:logs").querySelector(".dot.bad")).not.toBeNull();
    await select("notes");
    expect(devTab().hidden).toBe(true);
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
