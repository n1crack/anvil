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

  test("the header says what runs and opens Dev; Dev's Start leaves out what Dev start does", async () => {
    await select("vuefinder");
    expect($("#site [data-all]")).toBeNull();
    // Something in Dev start is stopped: Start (green), right of Configure.
    expect($$(".dev-bar button").map((b) => b.className)).toEqual(["dev-run", "dev-configure", "dev-all dev-stop"]);
    const ov = w.mockOv.vuefinder;
    const before = structuredClone(ov.rows);
    for (const r of ov.rows) r.running = false;
    await select("shop");
    await select("vuefinder");
    expect($(".dev-bar .dev-all").textContent).toBe("Start");
    expect($(".dev-bar .dev-all").className).toBe("dev-all dev-start");
    for (const r of ov.rows) r.running = r.id !== "docs-site";
    ov.rows.find((r) => r.id === "docs-site").manual = true;
    await select("shop");
    await select("vuefinder");
    expect($(".dev-bar .dev-all").textContent).toBe("Stop");
    expect($("#site .state-text").textContent).toBe("2 running");
    ($("#site .state-link") as HTMLElement).click();
    await wait(80);
    expect($('.tabs [data-tab="dev"]').getAttribute("aria-selected")).toBe("true");
    // Stop: "Stopping…" with a spinner while it works, then Dev stop for everything.
    const n = calls("process_run").length;
    w.mockDelay = { process_run: 60 };
    ($(".dev-bar .dev-all") as HTMLElement).click();
    await wait(10);
    expect($(".dev-bar .dev-all").textContent).toBe("Stopping…");
    expect($(".dev-bar .dev-all .spin-dot")).not.toBeNull();
    await wait(120);
    w.mockDelay = {};
    expect(calls("process_run").slice(n)[0][1]).toEqual({ site: "vuefinder", name: null, action: "stop" });
    expect($(".dev-bar .dev-all").textContent).toBe("Stop");
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
    expect($(".dev-list .dev-divider")).not.toBeNull();
    // A run's status sits in the dot column: a spinner, then ✓ or ✕ (the code in its tooltip).
    expect(item("once:test").querySelector(".dev-status .spin-dot")).not.toBeNull();
    expect(item("once:preview").firstElementChild!.className).toBe("dev-status ok");
    expect(item("once:preview").firstElementChild!.textContent).toBe("✓");
    const bad = item("once:format").firstElementChild as HTMLElement;
    expect([bad.textContent, bad.className, bad.title]).toEqual(["✕", "dev-status bad", "Exited with code 1"]);
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
    const runs = (from: number) =>
      calls("process_run")
        .slice(from)
        .map(([, a]) => [a.name, a.action]);
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
    expect(line.querySelector(".dev-summary-state")!.textContent).toBe("3 of 4 running");
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
    // Then only what came since the last line read.
    const n = calls("container_log").length;
    await wait(1100);
    const since = calls("container_log")
      .slice(n)
      .find(([, a]) => a.service === "web")![1];
    expect(since.since).toBe(w.mockContainerLogs.web.at(-1).split(" ")[0]);
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
