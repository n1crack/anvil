// Each window may call only the app commands its own code uses (app/capabilities/).
// A command counts as used when its name is a string literal anywhere in the window's
// module graph, so helpers taking a command name (`call("state")`, `pmAction(…, "x")`)
// are covered; a word that is also a command name (a tab called "services") over-allows.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { describe, expect, test } from "vite-plus/test";

const ui = resolve(import.meta.dirname, "..");
const app = resolve(ui, "..");
const read = (...p: string[]) => readFileSync(resolve(...p), "utf8");
const names = (list: string) => [...list.matchAll(/"?([a-z_]+)"?\s*,?/g)].map((m) => m[1]);

const mainRs = read(app, "src/main.rs");
const handler = names(mainRs.match(/generate_handler!\[([^\]]*)\]/)![1]);
const manifest = names(read(app, "build.rs").match(/COMMANDS: &\[&str\] = &\[([^\]]*)\]/)![1]);

// Window label → its page: the config's windows open index.html, the rest are built in main.rs.
const pages = new Map<string, string>(JSON.parse(read(app, "tauri.conf.json")).app.windows.map((w) => [w.label, "index.html"]));
for (const [, label, page] of mainRs.matchAll(/WebviewWindowBuilder::new\(\s*\w+,\s*"(\w+)",\s*WebviewUrl::App\("([^"]+)"/g))
  pages.set(label, page);

function graph(file: string, seen = new Set<string>()) {
  if (seen.has(file)) return seen;
  seen.add(file);
  for (const [, p] of read(file).matchAll(/(?:from|import)\s*\(?\s*"(\.[^"]+)"/g)) {
    const t = resolve(dirname(file), p);
    graph(existsSync(t) ? t : `${t}.ts`, seen);
  }
  return seen;
}

function used(page: string) {
  const entry = read(ui, page).match(/<script type="module" src="\/([^"]+)"/)![1];
  const found = new Set<string>();
  for (const f of graph(resolve(ui, entry)))
    for (const [, s] of read(f).matchAll(/["'`]([a-z_]+)["'`]/g)) if (handler.includes(s)) found.add(s);
  return [...found].sort();
}

const caps = readdirSync(resolve(app, "capabilities")).map((f) => JSON.parse(read(app, "capabilities", f)));

test("the app manifest lists every registered command", () => {
  expect(handler.length).toBeGreaterThan(100);
  expect([...manifest].sort()).toEqual([...handler].sort());
});

test("every window has exactly one capability", () => {
  expect(caps.flatMap((c) => c.windows).sort()).toEqual([...pages.keys()].sort());
  for (const c of caps) expect(c.windows).toHaveLength(1);
});

describe.each(caps.map((c) => [c.windows[0], c]))("window %s", (label, cap) => {
  test("allows exactly the commands its code calls", () => {
    const allowed = cap.permissions
      .filter((p) => typeof p === "string" && p.startsWith("allow-"))
      .map((p) => p.slice(6).replaceAll("-", "_"))
      .sort();
    expect(allowed).toEqual(used(pages.get(label)!));
  });
});
