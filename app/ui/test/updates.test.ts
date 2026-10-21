// The Software Update window driven through its DOM, with a mocked backend (test/mock.js).
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeAll, expect, test } from "vite-plus/test";

const w = window as any;
const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));
const $ = (sel: string) => document.querySelector(sel) as HTMLElement;
let recheck: () => void;

beforeAll(async () => {
  const html = readFileSync(resolve(import.meta.dirname, "../updates.html"), "utf8");
  document.documentElement.innerHTML = html.replace(/<!doctype html>/i, "").replace(/<script[\s\S]*?<\/script>|<link[^>]*>/g, "");
  await import("./mock.js");
  w.__TAURI__.app = { getVersion: async () => "0.2.1" };
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
  const notes = $("#upd-notes");
  expect(notes.hidden).toBe(false);
  expect([...notes.children].map((c) => c.tagName)).toEqual(["P", "H2", "UL", "H2", "UL"]);
  expect(notes.querySelector("h2")!.textContent).toBe("Added");
  expect(notes.querySelector("li code")!.textContent).toBe("anvil");
  expect(notes.querySelectorAll("li")).toHaveLength(3);
  expect(notes.querySelectorAll("h2")[1].textContent).toBe("Fixed");
});
