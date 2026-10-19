import type { ProcessRow } from "./types";

/* Configure's processes as plain items, and the calls that save them. No editor here, so
 * the table uses it without loading CodeMirror. */

export type Item = { name: string; command: string; devStart: boolean; withAnvil: boolean };
export type Call = [cmd: string, args: Record<string, unknown>];

/** The rows as Configure's table shows them. */
export const items = (rows: ProcessRow[]) =>
  rows.map((r): Item => ({ name: r.name, command: r.command, devStart: r.main || !r.manual, withAnvil: r.autostart }));

/** The rows as JSON, as the JSON mode shows them. */
export const toJson = (rows: ProcessRow[]) => JSON.stringify(items(rows), null, 2);

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
