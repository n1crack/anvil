import type { ProcessRow } from "./types";
import { jsonLanguage } from "@codemirror/lang-json";

/* Configure's JSON mode: the Dev processes as JSON, checked as they are typed, and the
 * calls that make the table so (the ones the table makes). Loaded with the editor. */

export type Item = { name: string; command: string; devStart: boolean; withAnvil: boolean };
export type Problem = { from: number; to: number; message: string };
export type Call = [cmd: string, args: Record<string, unknown>];

const KEYS = ["name", "command", "devStart", "withAnvil"];

/** The rows as Configure's table shows them, as JSON. */
export const toJson = (rows: ProcessRow[]) =>
  JSON.stringify(
    rows.map((r): Item => ({ name: r.name, command: r.command, devStart: r.main || !r.manual, withAnvil: r.autostart })),
    null,
    2,
  );

/** What `text` gets wrong, each with where; and the processes when nothing is. `main` is
 * the app's own row: it stays and always starts with Dev. */
export function check(text: string, main?: string) {
  const problems: Problem[] = [];
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (e) {
    // Where: the parser's first error node (WebKit's message has no position).
    let at = text.length;
    jsonLanguage.parser.parse(text).iterate({
      enter: (n) => {
        if (n.type.isError && at === text.length) at = n.from;
        return at === text.length;
      },
    });
    const msg = String((e as Error).message ?? e).replace(/^JSON Parse error: /, "");
    problems.push({ from: Math.min(at, text.length), to: Math.min(at + 1, text.length), message: `Not valid JSON: ${msg}` });
    return { problems, items: null };
  }
  // The nodes of the array's objects and their properties, for where each problem is.
  const top = jsonLanguage.parser.parse(text).topNode.firstChild;
  const kids = (n: typeof top) => {
    const out: NonNullable<typeof top>[] = [];
    for (let c = n?.firstChild; c; c = c.nextSibling) if (!/^[[\]{},:]$/.test(c.name)) out.push(c);
    return out;
  };
  const whole = { from: top?.from ?? 0, to: top?.to ?? text.length };
  if (!Array.isArray(value)) {
    problems.push({ ...whole, message: 'Expected an array: [{ "name", "command", "devStart", "withAnvil" }]' });
    return { problems, items: null };
  }
  const objects = kids(top);
  const seen = new Map<string, number>();
  value.forEach((v, i) => {
    const node = objects[i];
    const at = { from: node?.from ?? whole.from, to: node?.to ?? whole.to };
    if (!v || typeof v !== "object" || Array.isArray(v)) {
      problems.push({ ...at, message: "Each process is an object" });
      return;
    }
    const props = new Map(kids(node).map((p) => [JSON.parse(text.slice(p.firstChild!.from, p.firstChild!.to)), p]));
    const where = (k: string) => {
      const p = props.get(k);
      return p ? { from: p.from, to: p.to } : { from: at.from, to: Math.min(at.from + 1, at.to) };
    };
    for (const k of Object.keys(v))
      if (!KEYS.includes(k)) problems.push({ ...where(k), message: `Unknown key "${k}": use ${KEYS.join(", ")}` });
    const o = v as Record<string, unknown>;
    if (typeof o.name !== "string" || !o.name.trim()) problems.push({ ...where("name"), message: "name: a name is needed" });
    else {
      const n = o.name.trim();
      if (seen.has(n)) problems.push({ ...where("name"), message: `name: "${n}" is already process ${seen.get(n)! + 1}` });
      else seen.set(n, i);
    }
    if (typeof o.command !== "string" || !o.command.trim()) problems.push({ ...where("command"), message: "command: a command is needed" });
    for (const k of ["devStart", "withAnvil"])
      if (k in o && typeof o[k] !== "boolean") problems.push({ ...where(k), message: `${k}: true or false` });
    if (main && o.name === main && o.devStart === false)
      problems.push({ ...where("devStart"), message: `${main} is the app itself: it always starts with Dev` });
  });
  if (main && !problems.length && !seen.has(main)) problems.push({ ...whole, message: `${main} is the app itself: it stays` });
  if (problems.length) return { problems, items: null };
  const items = (value as Record<string, any>[]).map((o) => ({
    name: o.name.trim(),
    command: o.command.trim(),
    devStart: o.devStart ?? true,
    withAnvil: o.withAnvil ?? false,
  }));
  return { problems, items };
}

/** The calls that turn `rows` into `items`: a process left out is removed (as the
 * table's ×), a changed one set, a new one added; set_process only sends what changed,
 * as a new command restarts the process. New processes' switches come after (`later`),
 * once their ids are known. */
export function plan(rows: ProcessRow[], items: Item[]) {
  const calls: Call[] = [];
  const later: Item[] = [];
  const byName = new Map(items.map((x) => [x.name, x]));
  for (const r of rows) if (!r.main && !byName.has(r.name)) calls.push(["set_process", { id: r.id, hidden: true }]);
  for (const x of items) {
    const r = rows.find((r) => r.name === x.name);
    if (!r) {
      calls.push(["process_add", { name: x.name, command: x.command }]);
      later.push(x);
      continue;
    }
    const id = r.main ? "" : r.id;
    const args: Record<string, unknown> = {};
    if (x.command !== r.command) args.command = x.command;
    if (!r.main && x.devStart === r.manual) args.manual = !x.devStart;
    if (x.withAnvil !== r.autostart) args.autostart = x.withAnvil;
    if (Object.keys(args).length) calls.push(["set_process", { id, ...args }]);
  }
  return { calls, later };
}

/** Run `plan`'s calls in order through `ask`, then set the new processes' switches by
 * the ids `rows()` then has. Stops at the first failure (thrown). */
export async function apply(p: ReturnType<typeof plan>, ask: (c: Call) => Promise<unknown>, rows: () => ProcessRow[]) {
  for (const c of p.calls) await ask(c);
  for (const x of p.later) {
    const r = rows().find((r) => !r.main && r.name === x.name);
    if (!r) continue;
    const args: Record<string, unknown> = {};
    if (x.devStart === r.manual) args.manual = !x.devStart;
    if (x.withAnvil !== r.autostart) args.autostart = x.withAnvil;
    if (Object.keys(args).length) await ask(["set_process", { id: r.id, ...args }]);
  }
}
