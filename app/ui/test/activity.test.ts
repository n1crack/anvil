// The Processes window driven through its DOM, with a mocked backend.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, describe, expect, test } from "vite-plus/test";

const w = window as any;
const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));
const $ = (sel: string) => document.querySelector(sel) as HTMLElement;
const LOG = "2026-10-17 10:00:01 local.ERROR: boom\n\x1b[37;44m INFO \x1b[39;49m Server running\n";
const row = (key: string, name: string, state: string) => ({
  key,
  name,
  pid: 1,
  port: null,
  cpu: 0,
  memory: 0,
  state,
  detail: null,
  actions: true,
});

let activity: any;

beforeAll(async () => {
  localStorage.clear();
  localStorage.setItem("act-folded", '["vuefinder"]');
  const html = readFileSync(resolve(import.meta.dirname, "../processes.html"), "utf8");
  document.documentElement.innerHTML = html.replace(/<!doctype html>/i, "").replace(/<script[\s\S]*?<\/script>|<link[^>]*>/g, "");
  activity = {
    web: [],
    apps: [
      { site: "shop", rows: [row("shop@queue", "queue", "running")] },
      {
        site: "vuefinder",
        rows: [
          row("vuefinder@dev", "dev", "running"),
          row("vuefinder@docs-dev", "docs:dev", "running"),
          row("vuefinder@x", "x", "crashed"),
        ],
      },
    ],
    services: [],
  };
  w.__TAURI__ = {
    event: { listen: async (name: string, cb: any) => ((w.listeners ??= {})[name] = cb) },
    core: {
      invoke: async (cmd: string, a: any) => {
        (w.actCalls ??= []).push([cmd, a]);
        if (cmd === "activity") return structuredClone(activity);
        if (cmd === "agent_activity") return structuredClone(w.agents ?? []);
        if (cmd === "activity_log_clear") return void (w.logCleared = true);
        return w.logCleared ? { text: "", end_offset: 0 } : { text: LOG, end_offset: LOG.length };
      },
    },
    menu: {
      Menu: { new: async ({ items }) => ({ popup: () => (w.mockMenu = items) }) },
      MenuItem: { new: async (o) => o },
      PredefinedMenuItem: { new: async (o) => o },
    },
  };
  await import("../src/activity");
  await wait(80);
});

describe("processes window", () => {
  test("right-click opens the menu without selecting the row", async () => {
    const tr = () => $('tr[data-key="shop@queue"]');
    tr().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await wait();
    expect(w.mockMenu.map((i) => i.text)).toContain("Stop");
    expect(tr().getAttribute("aria-selected")).toBe("false");
    expect($("#pane").hidden).toBe(true);
    tr().click();
    await wait();
    expect(tr().getAttribute("aria-selected")).toBe("true");
  });

  test("a folded group says what it hides on its header line", async () => {
    const head = [...document.querySelectorAll(".act-app")].find((h) => h.textContent?.includes("vuefinder")) as HTMLElement;
    expect(head.getAttribute("aria-expanded")).toBe("false");
    expect($(".act-app[aria-expanded=false] .act-sum").textContent).toBe("2 running, 1 crashed · dev, docs:dev, x");
    expect($(".act-sum-crashed").textContent).toBe("1 crashed");
    expect(document.querySelector('tr[data-key="vuefinder@dev"]')).toBeNull();
    head.click();
    await wait();
    expect(document.querySelector('tr[data-key="vuefinder@dev"]')).not.toBeNull();
  });

  test("the keyboard: one row takes Tab, arrows move, Enter opens the log, Shift+F10 the menu", async () => {
    const key = (k: string, o = {}) =>
      document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...o }));
    const tabbable = () => [...document.querySelectorAll<HTMLElement>("#rows tr")].filter((t) => t.tabIndex === 0);
    expect(tabbable().map((t) => t.dataset.key)).toEqual(["shop@queue"]);
    $("#log-close").click();
    await wait();
    $('tr[data-key="shop@queue"]').focus();
    key("ArrowDown"); // the vuefinder header
    key("ArrowDown");
    expect(document.activeElement).toBe($('tr[data-key="vuefinder@dev"]'));
    expect($("#pane").hidden).toBe(true);
    key("Enter");
    await wait();
    expect($('tr[data-key="vuefinder@dev"]').getAttribute("aria-selected")).toBe("true");
    expect(document.activeElement).toBe($('tr[data-key="vuefinder@dev"]'));
    key("ArrowDown"); // with the log open, it follows
    await wait();
    expect($('tr[data-key="vuefinder@docs-dev"]').getAttribute("aria-selected")).toBe("true");
    expect(document.activeElement).toBe($('tr[data-key="vuefinder@docs-dev"]'));
    expect(tabbable().map((t) => t.dataset.key)).toEqual(["vuefinder@docs-dev"]);
    w.mockMenu = null;
    key("F10", { shiftKey: true });
    await wait();
    expect(w.mockMenu.map((i) => i.text)).toContain("Restart");
    $("#log-close").click();
    await wait();
  });
});

describe("processes tabs", () => {
  test("Apps by default; a stored tab comes back, the old web one as Anvil", async () => {
    const { storedTab } = await import("../src/activity");
    expect(storedTab(null)).toBe("apps");
    expect(storedTab("nonsense")).toBe("apps");
    expect(storedTab("services")).toBe("services");
    expect(storedTab("web")).toBe("anvil");
  });

  test("the log keeps a process's colors and tints plain lines", async () => {
    $('tr[data-key="shop@queue"]').click();
    await wait();
    expect($("#log").textContent).toBe("2026-10-17 10:00:01 local.ERROR: boom\n INFO  Server running\n");
    expect($("#log .term-time").textContent).toBe("2026-10-17 10:00:01");
    expect($("#log .ansi1").textContent).toBe("ERROR");
    expect($("#log .ansi-bg4.ansi7").textContent).toBe(" INFO ");
  });

  test("the log’s Clear, left of ×, asks in place, then empties the file and the view", async () => {
    $('tr[data-key="shop@queue"]').click();
    await wait();
    expect($("#log").textContent).not.toBe("");
    const clear = $("#log-clear");
    expect(clear.hidden).toBe(false);
    expect(clear.nextElementSibling).toBe($("#log-close"));
    // It asks in the pane's head first; Cancel leaves the file alone.
    clear.click();
    expect($("#log-ask").hidden).toBe(false);
    expect($("#log-title").hidden).toBe(true);
    $("#log-no").click();
    await wait(60);
    expect(w.actCalls.some((c: any) => c[0] === "activity_log_clear")).toBe(false);
    expect($("#log-ask").hidden).toBe(true);
    expect(clear.hidden).toBe(false);
    clear.click();
    $("#log-yes").click();
    await wait(60);
    expect(w.actCalls.filter((c: any) => c[0] === "activity_log_clear").at(-1)[1]).toEqual({ key: "shop@queue" });
    expect($("#log").textContent).toBe("");
    w.logCleared = false;
  });

  const tabBtn = (t: string) => $(`#tabs [data-tab="${t}"]`);
  test("each tab counts its rows and shows only them; the tab is remembered", async () => {
    expect(tabBtn("apps").textContent).toBe("Apps4");
    expect(tabBtn("anvil").textContent).toBe("Anvil");
    expect([...document.querySelectorAll("#tabs [role=tab]")].map((b) => (b as HTMLElement).dataset.tab)).toEqual([
      "apps",
      "services",
      "anvil",
      "agents",
    ]);
    expect(tabBtn("apps").getAttribute("aria-selected")).toBe("true");
    tabBtn("services").click();
    expect(tabBtn("services").getAttribute("aria-selected")).toBe("true");
    expect(document.querySelectorAll("#rows tr")).toHaveLength(0);
    expect($("#empty").hidden).toBe(false);
    expect($("#empty").textContent).toBe("Nothing running");
    expect(localStorage.getItem("act-tab")).toBe("services");
    tabBtn("apps").click();
    expect($("#empty").hidden).toBe(true);
    expect(document.querySelector('tr[data-key="shop@queue"]')).not.toBeNull();
  });

  test("the selected row's log stays open on another tab", async () => {
    $('tr[data-key="shop@queue"]').click();
    await wait();
    tabBtn("anvil").click();
    expect($("#pane").hidden).toBe(false);
    tabBtn("apps").click();
  });

  test("agents_tab_lists_all_newest_first, from the activity poll", async () => {
    const { load } = await import("../src/activity");
    const at = (h: number, m: number) => new Date(2026, 9, 19, h, m).getTime() / 1000;
    // The backend sends them newest first.
    w.agents = [
      { at: at(14, 2), client: "Claude Code", site: "shop", what: "restarted queue", ok: false, detail: "No process queue" },
      { at: at(9, 5), client: "Terminal", site: null, what: "started mysql", ok: true, detail: null },
    ];
    tabBtn("agents").click();
    expect($("#empty").textContent).toBe("No actions from agents or the terminal yet");
    await load();
    expect(w.actCalls.filter((c: any) => c[0] === "agent_activity").at(-1)[1]).toEqual({ site: null });
    expect($("#empty").hidden).toBe(true);
    expect($("#rows").closest("table")!.hidden).toBe(true);
    const rows = [...document.querySelectorAll("#agent-rows tr")].map((tr) => [...tr.children].map((c) => c.firstChild?.textContent ?? ""));
    expect(rows).toEqual([
      ["Claude Code restarted queue", "shop", "14:02"],
      ["Terminal started mysql", "", "09:05"],
    ]);
    const d = $("#agent-rows .agent-detail");
    expect([d.textContent, d.classList.contains("error-text")]).toEqual(["No process queue", true]);
    w.agents = [];
    tabBtn("apps").click();
    expect($("#agents").hidden).toBe(true);
  });
});

describe("asked to show a row", () => {
  test("the service page's log button: its tab, the row selected, its log open", async () => {
    activity.services.push(row("service:cache", "Redis", "running"));
    w.listeners["activity-select"]({ payload: "service:cache" });
    await wait(60);
    expect($('#tabs [data-tab="services"]').getAttribute("aria-selected")).toBe("true");
    expect($('tr[data-key="service:cache"]').getAttribute("aria-selected")).toBe("true");
    expect($("#pane").hidden).toBe(false);
    expect($("#log-title").textContent).toBe("Redis · log");
    activity.services.pop();
    $('#tabs [data-tab="apps"]').click();
  });
});

describe("long-open window", () => {
  test("usage changes are written into the row; the table isn't rebuilt", async () => {
    const { load } = await import("../src/activity");
    await load();
    const tr = $('tr[data-key="shop@queue"]');
    activity.apps[0].rows[0].cpu = 12.5;
    activity.apps[0].rows[0].memory = 50 * 1048576;
    await load();
    expect($('tr[data-key="shop@queue"]')).toBe(tr);
    expect(tr.children[3].textContent).toBe("12.5%");
    expect(tr.children[4].textContent).toBe("50 MB");
    activity.apps[0].rows[0].state = "crashed";
    await load();
    expect($('tr[data-key="shop@queue"]')).not.toBe(tr);
    activity.apps[0].rows[0].state = "running";
    await load();
  });

  test("the log keeps about LOG_LINES of the newest pages, the oldest go in one removal", async () => {
    const { trimLog, LOG_LINES } = await import("../src/activity");
    const log = document.createElement("pre");
    for (let i = 0; i < 10; i++) {
      const p = document.createElement("span") as any;
      p.textContent = `page ${i}\n`.repeat(1000);
      p.lines = 1000;
      log.append(p);
    }
    trimLog(log);
    expect(log.children).toHaveLength(LOG_LINES / 1000);
    expect(log.firstElementChild!.textContent).toMatch(/^page 7\n/);
  });

  test("an error stays until its owner clears it: a poll that works keeps an action's", async () => {
    const { load, showError } = await import("../src/activity");
    showError("Stop failed", "act");
    await load();
    expect($("#error").hidden).toBe(false);
    expect($("#error").textContent).toBe("Stop failed");
    showError(null, "act");
    expect($("#error").hidden).toBe(true);
  });

  test("Docker's error shows until a poll lists the containers again", async () => {
    const { load } = await import("../src/activity");
    activity.docker_error = "Docker isn't running";
    await load();
    expect($("#error").hidden).toBe(false);
    expect($("#error").textContent).toBe("Docker isn't running");
    activity.docker_error = null;
    await load();
    expect($("#error").hidden).toBe(true);
  });
});
