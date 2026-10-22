// The Software Update window driven through its DOM, with a mocked backend (test/mock.js).
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, expect, test } from "vite-plus/test";

const w = window as any;
const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));
const $ = (sel: string) => document.querySelector(sel) as HTMLElement;
let recheck: () => void;
const fits = () => (w.mockCalls as [string, any][]).filter(([c]) => c === "updates_fit").map(([, a]) => a.tall);

beforeAll(async () => {
  const html = readFileSync(resolve(import.meta.dirname, "../updates.html"), "utf8");
  document.documentElement.innerHTML = html.replace(/<!doctype html>/i, "").replace(/<script[\s\S]*?<\/script>|<link[^>]*>/g, "");
  await import("./mock.js");
  w.__TAURI__.app = { getVersion: async () => "0.2.1" };
  w.__TAURI__.window = { getCurrentWindow: () => ({ close: () => (w.closed = (w.closed ?? 0) + 1) }) };
  w.__TAURI__.event.listen = async (name: string, f: () => void) => {
    if (name === "update-recheck") recheck = f;
    return () => {};
  };
  await import("../src/updates");
  await wait();
});

test("up to date: the version, Close, no Restart", () => {
  expect($("#upd-title").textContent).toBe("Anvil is up to date");
  expect($("#upd-sub").textContent).toBe("Version 0.2.1");
  expect($("#upd-close").textContent).toBe("Close");
  expect($("#upd-go").hidden).toBe(true);
  expect($("#upd-notes").hidden).toBe(true);
  expect(document.activeElement).toBe($("#upd-close"));
  // Short: nothing to read.
  expect(fits().at(-1)).toBe(false);
});

test("a newer release: what's new as headings and bullets, Later and Restart to update", async () => {
  w.mockUpdate = {
    version: "0.2.2",
    notes: "Faster starts.\n\nAdded\n\n• A window for `anvil` updates.\n• Tray item\n\n### Fixed\n\n- Notes read well.",
  };
  recheck();
  await wait();
  expect($("#upd-title").textContent).toBe("Anvil 0.2.2 is available");
  expect($("#upd-close").textContent).toBe("Later");
  expect($("#upd-go").hidden).toBe(false);
  expect(document.activeElement).toBe($("#upd-go"));
  const notes = $("#upd-notes");
  expect(notes.hidden).toBe(false);
  expect([...notes.children].map((c) => c.tagName)).toEqual(["P", "H2", "UL", "H2", "UL"]);
  expect(notes.querySelector("h2")!.textContent).toBe("Added");
  expect(notes.querySelector("li code")!.textContent).toBe("anvil");
  expect(notes.querySelectorAll("li")).toHaveLength(3);
  expect(notes.querySelectorAll("h2")[1].textContent).toBe("Fixed");
  // Tall for the notes.
  expect(fits().at(-1)).toBe(true);
});

test("a failed check: the reason under the title, Try Again checks again", async () => {
  const err = "error sending request for url (https://github.com/n1crack/anvil/releases/latest/download/latest.json)";
  Object.defineProperty(w, "mockUpdate", {
    configurable: true,
    get: () => {
      throw err;
    },
  });
  recheck();
  await wait();
  expect($("#upd-title").textContent).toBe("Couldn't check for updates");
  expect($("#upd-sub").textContent).toBe(err);
  expect($("#upd-status").textContent).toBe("");
  expect($("#upd-retry").hidden).toBe(false);
  expect($("#upd-go").hidden).toBe(true);
  expect(document.activeElement).toBe($("#upd-retry"));
  Object.defineProperty(w, "mockUpdate", { configurable: true, writable: true, value: null });
  $("#upd-retry").click();
  await wait();
  expect($("#upd-title").textContent).toBe("Anvil is up to date");
  expect($("#upd-sub").classList.contains("error-text")).toBe(false);
  expect($("#upd-retry").hidden).toBe(true);
});

test("Esc closes the window, but not while the update downloads", async () => {
  const esc = () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  w.closed = 0;
  w.mockUpdate = { version: "0.2.2", notes: null };
  recheck();
  await wait();
  w.mockDelay = { update_install: 80 };
  $("#upd-go").click();
  await wait(10);
  esc();
  expect(w.closed).toBe(0);
  await wait(120);
  w.mockDelay = {};
  esc();
  expect(w.closed).toBe(1);
});
