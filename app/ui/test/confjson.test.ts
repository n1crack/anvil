import { describe, expect, test } from "vite-plus/test";
import { labelSpan } from "../src/confplan";
import { type Call, apply, check, plan, toJson } from "../src/confjson";

const row = (o: Record<string, unknown>): any => ({ id: "", name: "", command: "", main: false, manual: false, autostart: false, ...o });
const rows = [
  row({ name: "dev", command: "vite", main: true }),
  row({ id: "queue", name: "queue", command: "php artisan queue:listen" }),
  row({ id: "logs", name: "logs", command: "php artisan pail", manual: true, autostart: true }),
];

describe("check", () => {
  const messages = (text: string) => check(text, "dev").problems.map((p) => `${text.slice(0, p.from).split("\n").length}: ${p.message}`);

  test("the rows' own JSON is valid and reads back as they are", () => {
    const text = toJson(rows);
    expect(JSON.parse(text)[2]).toEqual({ name: "logs", command: "php artisan pail", devStart: false, withAnvil: true });
    expect(check(text, "dev")).toEqual({
      problems: [],
      items: [
        { name: "dev", command: "vite", devStart: true, withAnvil: false },
        { name: "queue", command: "php artisan queue:listen", devStart: true, withAnvil: false },
        { name: "logs", command: "php artisan pail", devStart: false, withAnvil: true },
      ],
    });
  });

  test("JSON that doesn't parse: one problem, on the line where it breaks", () => {
    const p = messages('[\n  { "name": "dev", "command": "vite" },\n  { "name": oops }\n]');
    expect(p).toHaveLength(1);
    expect(p[0]).toMatch(/^3: Not valid JSON/);
  });

  test("an array of objects", () => {
    expect(messages('{ "name": "dev" }')).toEqual(['1: Expected an array: [{ "name", "command", "devStart", "withAnvil" }]']);
    expect(messages('[{ "name": "dev", "command": "vite" },\n 3]')).toEqual(["2: Each process is an object"]);
  });

  test("names given and unique, commands given, booleans, no unknown keys; each on its line", () => {
    const text = `[
  { "name": "dev", "command": "vite" },
  { "name": " ", "command": "a" },
  { "name": "dev", "command": "" },
  { "name": "w", "command": "b", "devStart": "yes" },
  { "name": "x", "command": "c", "colour": 1 }
]`;
    expect(messages(text)).toEqual([
      "3: name: a name is needed",
      '4: name: "dev" is already process 1',
      "4: command: a command is needed",
      "5: devStart: true or false",
      '6: Unknown key "colour": use name, command, devStart, withAnvil',
    ]);
  });

  test("the app itself stays, and always starts with Dev", () => {
    expect(messages('[{ "name": "q", "command": "x" }]')).toEqual(["1: dev is the app itself: it stays"]);
    expect(messages('[{ "name": "dev", "command": "vite", "devStart": false }]')).toEqual([
      "1: dev is the app itself: it always starts with Dev",
    ]);
  });
});

describe("plan and apply", () => {
  test("left out is removed, changed is set with only what changed, new is added; the app's id is empty", () => {
    const items = [
      { name: "dev", command: "vite --host", devStart: true, withAnvil: true },
      { name: "logs", command: "php artisan pail", devStart: true, withAnvil: true },
      { name: "horizon", command: "php artisan horizon", devStart: false, withAnvil: true },
    ];
    const p = plan(rows, items);
    expect(p.calls).toEqual([
      ["set_process", { id: "queue", hidden: true }],
      ["set_process", { id: "", command: "vite --host", autostart: true }],
      ["set_process", { id: "logs", manual: false }],
      ["process_add", { name: "horizon", command: "php artisan horizon" }],
    ]);
    expect(p.later.map((x) => x.name)).toEqual(["horizon"]);
    expect(plan(rows, check(toJson(rows), "dev").items!).calls).toEqual([]);
  });

  test("a new process's switches are set by the id it got; a failure stops the rest", async () => {
    const seen: Call[] = [];
    let now = rows;
    const ask = async (c: Call) => {
      seen.push(c);
      if (c[0] === "process_add") now = [...now, row({ id: "horizon-1", name: "horizon", command: "php artisan horizon" })];
    };
    const items = [
      ...check(toJson(rows), "dev").items!,
      { name: "horizon", command: "php artisan horizon", devStart: false, withAnvil: true },
    ];
    await apply(plan(rows, items), ask, () => now);
    expect(seen).toEqual([
      ["process_add", { name: "horizon", command: "php artisan horizon" }],
      ["set_process", { id: "horizon-1", manual: true, autostart: true }],
    ]);
    const failing = async (c: Call) => {
      seen.push(c);
      throw "The command can't be empty";
    };
    seen.length = 0;
    await expect(apply(plan(rows, items.slice(0, 1)), failing, () => now)).rejects.toBe("The command can't be empty");
    expect(seen).toHaveLength(1);
  });
});

describe("labelSpan", () => {
  test("finds the label after the exact <app>-- prefix", () => {
    for (const [app, host] of [
      ["vuefinder", "vuefinder--docs.test"],
      ["my--app", "my--app--docs.test"],
      ["my.app", "my.app--docs.test"],
    ]) {
      const [from, to] = labelSpan(app, host);
      expect(host.slice(from, to)).toBe("docs");
      expect(host.slice(to)).toBe(".test");
    }
  });
});
