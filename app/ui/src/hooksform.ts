import { icon } from "./icons";
import type { Hooks } from "./types";

/* The worktree hooks as plain editors, shared by Settings › Worktrees (a preset) and an
 * app's Settings tab (its own layer): rows with ×, an Add line, choices instead of
 * `{db}`-style placeholders. Each change is saved at once; a refusal is said under it.
 * `check` has no editor: keys set to its database or a prefix are checked by themselves
 * (worktrees::resolve), an explicit `check` in a config file still works. */

export type Field = "copy" | "copy_database" | "keys" | "after_create";

export const NAMES: Record<Field, string> = {
  copy: "Copy files",
  copy_database: "Database",
  keys: "Env keys",
  after_create: "After create",
};

/** What an env key can be set to, by plain name; "" is Custom… (free text). */
export const CHOICES: [string, string][] = [
  ["{db}", "This worktree’s database"],
  ["{url}", "This worktree’s address"],
  ["{prefix}", "A prefix of its own"],
  ["{port}", "Its port"],
  ["{name}", "Its name"],
  ["", "Custom…"],
];

export interface Opts {
  /** Store `field`'s new value; fails with the reason it was refused. */
  save: (field: Field, value: unknown) => Promise<unknown>;
  /** What `{install}` runs here: the lock file's install (undefined: not known, a preset). */
  install?: string | null;
  /** The app's folder: Copy files offers Choose… in it. */
  root?: string;
}

function el(tag: string, cls?: string | null, text?: string | null): any {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

const input = (placeholder: string, cls?: string) =>
  Object.assign(el("input", cls), { type: "text", placeholder, spellcheck: false, autocomplete: "off" }) as HTMLInputElement;

function remove(label: string, onclick: () => void) {
  const b = el("button", "icon-btn hk-x");
  b.type = "button";
  b.title = `Remove ${label}`;
  b.setAttribute("aria-label", b.title);
  b.append(icon("x"));
  b.onclick = onclick;
  return b;
}

function action(text: string, onclick: () => void) {
  const b = el("button", "inline-action", text);
  b.type = "button";
  b.onclick = onclick;
  return b;
}

/** `{install}` in a command, said plainly. */
export function command(c: string, install?: string | null): string {
  if (c.trim() === "{install}") return `Install packages (${install ?? "the lock file’s install"})`;
  return c.replaceAll("{install}", install ?? "the lock file’s install");
}

/** One line of the summary; `title` names the env keys behind it. */
export interface Line {
  text: string;
  title?: string;
}

/** What each env key value gives a worktree, said as a thing of its own. */
const OWN: Record<string, string> = { "{url}": "address", "{prefix}": "cache prefix", "{port}": "port", "{name}": "name" };

const and = (xs: string[]) => (xs.length > 1 ? `${xs.slice(0, -1).join(", ")} and ${xs.at(-1)}` : xs[0]);

/** What a new worktree gets, a short line each: files, database, the keys set to its own
 * things (names on hover), and the commands it then offers. */
export function summary(h: Hooks, install?: string | null): { gets: Line[]; offers: string | null } {
  const keys = Object.entries(h.keys ?? {});
  const dbKeys = keys.filter(([, v]) => v === "{db}").map(([k]) => k);
  const rest = keys.filter(([, v]) => v !== "{db}");
  const things = [...new Set(rest.map(([, v]) => OWN[v] ?? "other values"))];
  // Without package.json a command with {install} is left out (worktrees::commands).
  const cmds = (h.after_create ?? []).filter((c) => install !== null || !c.includes("{install}"));
  const gets: Line[] = [
    { text: h.copy?.length ? `${and(h.copy)} copied` : "No files copied" },
    {
      text: h.copy_database ? "Its own database, a copy of main’s" : "Its own empty database",
      ...(dbKeys.length ? { title: `Set in ${dbKeys.join(", ")}` } : {}),
    },
  ];
  if (rest.length) gets.push({ text: `Its own ${and(things)}`, title: `Set in ${rest.map(([k]) => k).join(", ")}` });
  const offer = (c: string) => (c.trim() === "{install}" ? (install ?? "Install packages") : command(c, install));
  return { gets, offers: cmds.length ? `Then offers: ${cmds.map(offer).join(" · ")}` : null };
}

/** The editor of `field`, showing `h`'s value. */
export function editor(field: Field, h: Hooks, o: Opts): HTMLElement {
  const box = el("div", "hk");
  const fail = el("small", "error-text");
  fail.hidden = true;
  const run = (value: unknown) => {
    fail.hidden = true;
    // A change made is a change done: the next render may replace what has focus.
    (document.activeElement as HTMLElement | null)?.blur?.();
    o.save(field, value).catch((e) => {
      fail.textContent = String(e);
      fail.hidden = false;
    });
  };
  const add = el("div", "hk-row hk-add");
  const onEnter = (i: HTMLInputElement, go: () => void) =>
    (i.onkeydown = (e) => {
      if (e.key === "Enter") go();
    });

  if (field === "copy_database") {
    const label = el("label", "hk-row hk-switch");
    const sw = Object.assign(el("input"), { type: "checkbox", checked: !!h.copy_database }) as HTMLInputElement;
    sw.setAttribute("role", "switch");
    sw.onchange = () => run(sw.checked);
    label.append(sw, el("span", null, "Start with a copy of main’s data"));
    box.append(label, el("small", "hk-hint", "Each worktree gets a database of its own; off, it starts empty."), fail);
    return box;
  }

  if (field === "copy") {
    const list = h.copy ?? [];
    for (const p of list) {
      const row = el("div", "hk-row");
      row.append(
        el("code", "hk-text", p),
        remove(p, () => run(list.filter((x) => x !== p))),
      );
      box.append(row);
    }
    const path = input(".env.local or config/*.json", "hk-text");
    const go = (p = path.value.trim()) => {
      if (p && !list.includes(p)) run([...list, p]);
    };
    onEnter(path, () => go());
    add.append(path);
    if (o.root) {
      const root = o.root.replaceAll("\\", "/").replace(/\/$/, "");
      add.append(
        action("Choose…", async () => {
          const picked = await window.__TAURI__.dialog.open({ defaultPath: o.root, title: "Choose a file to copy" });
          if (!picked) return;
          const p = String(picked).replaceAll("\\", "/");
          if (!p.startsWith(`${root}/`)) {
            fail.textContent = "Choose a file inside the app’s folder";
            fail.hidden = false;
            return;
          }
          go(p.slice(root.length + 1));
        }),
      );
    }
    // Add last on every Add line, so it lines up with the × above it.
    add.append(action("Add", () => go()));
    box.append(add, el("small", "hk-hint", "Copied from main’s folder into the new one; * matches any characters."), fail);
    return box;
  }

  if (field === "keys") {
    const keys = h.keys ?? {};
    // A value chooser, and the free text Custom… shows.
    const chooser = (value: string, set: (v: string) => void) => {
      const sel = el("select", "hk-choice") as HTMLSelectElement;
      for (const [v, text] of CHOICES) sel.append(Object.assign(el("option", null, text), { value: v }));
      const known = CHOICES.some(([v]) => v && v === value);
      sel.value = known ? value : "";
      const custom = input("Value, e.g. {name}_cache", "hk-custom");
      custom.value = known ? "" : value;
      custom.hidden = known;
      sel.onchange = () => {
        custom.hidden = sel.value !== "";
        if (sel.value) set(sel.value);
        else custom.focus();
      };
      custom.onchange = () => custom.value.trim() && set(custom.value.trim());
      return [sel, custom] as const;
    };
    for (const [k, v] of Object.entries(keys)) {
      const row = el("div", "hk-row");
      const [sel, custom] = chooser(v, (nv) => run({ ...keys, [k]: nv }));
      const rest = { ...keys };
      delete rest[k];
      row.append(
        el("code", "hk-key", k),
        sel,
        custom,
        remove(k, () => run(rest)),
      );
      box.append(row);
    }
    const key = input("KEY", "hk-key");
    const [sel, custom] = chooser("{db}", () => {});
    const go = () => {
      const k = key.value.trim();
      const v = sel.value || custom.value.trim();
      if (k) run({ ...keys, [k]: v });
    };
    onEnter(key, go);
    onEnter(custom, go);
    add.append(key, sel, custom, action("Add", go));
    box.append(add, el("small", "hk-hint", "Set for the worktree’s processes, over its .env, so it doesn’t share main’s."), fail);
    return box;
  }

  // after_create
  const list = h.after_create ?? [];
  list.forEach((c, i) => {
    const row = el("div", "hk-row");
    const text = command(c, o.install);
    row.append(
      el(text === c ? "code" : "span", "hk-text", text),
      remove(text, () => run(list.filter((_, j) => j !== i))),
    );
    box.append(row);
  });
  const cmd = input("php artisan migrate", "hk-text");
  const go = () => {
    const c = cmd.value.trim();
    if (c) run([...list, c]);
  };
  onEnter(cmd, go);
  add.append(cmd);
  if (!list.includes("{install}")) add.append(action("Add Package Install", () => run([...list, "{install}"])));
  add.append(action("Add", go));
  box.append(add, el("small", "hk-hint", "Offered once the worktree is made; each runs only after you say so."), fail);
  return box;
}
