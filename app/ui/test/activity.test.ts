// The Processes window driven through its DOM, with a mocked backend.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, describe, expect, test } from "vite-plus/test";

const w = window as any;
const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));
const $ = (sel: string) => document.querySelector(sel) as HTMLElement;
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

beforeAll(async () => {
  localStorage.clear();
  localStorage.setItem("act-folded", '["vuefinder"]');
  const html = readFileSync(resolve(import.meta.dirname, "../processes.html"), "utf8");
  document.documentElement.innerHTML = html.replace(/<!doctype html>/i, "").replace(/<script[\s\S]*?<\/script>|<link[^>]*>/g, "");
  const activity = {
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
    core: { invoke: async (cmd: string) => (cmd === "activity" ? structuredClone(activity) : { entries: [], end_offset: 0 }) },
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
});

describe("processes tabs", () => {
  const tabBtn = (t: string) => $(`#tabs [data-tab="${t}"]`);
  test("each tab counts its rows and shows only them; the tab is remembered", async () => {
    expect(tabBtn("apps").textContent).toBe("Apps4");
    expect(tabBtn("web").textContent).toBe("Web server");
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
    tabBtn("web").click();
    expect($("#pane").hidden).toBe(false);
    tabBtn("apps").click();
  });
});
