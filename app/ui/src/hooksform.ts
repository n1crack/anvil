import type { Hooks } from "./types";

/* The worktree hooks as form fields, shared by Settings › Worktrees (a preset) and an
 * app's Settings tab (its own layer): each saved on change, a refusal said under it. */

export type Field = keyof Hooks;

/** Each field: its label and one line about it. */
export const FIELDS: [Field, string, string][] = [
  ["copy", "Copy files", "Copied from the app into a new worktree, one path per line; * matches any characters"],
  ["copy_database", "Copy the database too", "New Worktree's checkbox: off gives an empty database of its own"],
  ["keys", "Env keys", "KEY=value per line with {db} {url} {prefix} {port} {name}; {db} alone points a URL at its database"],
  ["check", "Check keys", "Warned about while a worktree's env files give main's value"],
  ["after_create", "After create", "Offered once a worktree is made, run only on Run; {install} is the lock file's install"],
];

const lines = (t: string) =>
  t
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);

/** `field` as text, one item per line. */
export function asText(field: Field, h: Hooks): string {
  if (field === "keys")
    return Object.entries(h.keys ?? {})
      .map(([k, v]) => `${k}=${v}`)
      .join("\n");
  return ((h[field] as string[] | undefined) ?? []).join("\n");
}

/** The text of a list field as its value: `KEY=value` lines for keys. */
export function parse(field: Field, text: string): unknown {
  if (field !== "keys") return lines(text);
  return Object.fromEntries(
    lines(text).map((l) => {
      const at = l.indexOf("=");
      return at < 0 ? [l, ""] : [l.slice(0, at).trim(), l.slice(at + 1).trim()];
    }),
  );
}

/** The control of `field` showing `h`'s value; `save` stores a new one and fails with the
 * reason it was refused, shown in `fail`. */
export function control(field: Field, h: Hooks, save: (value: unknown) => Promise<unknown>, fail: HTMLElement): HTMLElement {
  const run = (value: unknown) => {
    fail.hidden = true;
    save(value).catch((e) => {
      fail.textContent = String(e);
      fail.hidden = false;
    });
  };
  if (field === "copy_database") {
    const box = Object.assign(document.createElement("input"), { type: "checkbox", checked: !!h.copy_database });
    box.setAttribute("role", "switch");
    box.onchange = () => run(box.checked);
    return box;
  }
  const area = document.createElement("textarea");
  area.rows = Math.min(8, Math.max(2, asText(field, h).split("\n").length));
  area.spellcheck = false;
  area.value = asText(field, h);
  area.onchange = () => run(parse(field, area.value));
  return area;
}
