import { $, el } from "./app";

/* A searchable, grouped single-column list to pick one thing from, as the left side of
 * the New application and Add service sheets: a glyph, a name and a line each, a ✓ on
 * the chosen one, the search above filtering by name and line. Arrows move the choice
 * (from the list or the search), Enter submits the form, Esc clears the search before
 * the sheet closes. */

type Choice = { id: string; name: string; about: string; group: string; icon: string };

const CHECK = `<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="m3.5 8.5 3 3 6-7" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

export function picker(o: {
  list: HTMLElement;
  search: HTMLInputElement;
  sheet: HTMLDialogElement;
  form: HTMLFormElement;
  groups: string[];
  choices: () => Choice[];
  /** Called when the choice changes (also on draw). */
  onPick: (id: string) => void;
  none: string;
}) {
  let chosen = "";
  const items = () => [...o.list.querySelectorAll(".kind-item:not([hidden])")] as HTMLElement[];

  function pick(id: string) {
    chosen = id;
    for (const it of o.list.querySelectorAll(".kind-item"))
      it.setAttribute("aria-selected", String((it as HTMLElement).dataset.kind === id));
    const cur = items().find((it) => it.dataset.kind === id);
    if (cur) o.list.setAttribute("aria-activedescendant", cur.id);
    cur?.scrollIntoView?.({ block: "nearest" });
    o.onPick(id);
  }

  function draw(selected: string) {
    o.list.replaceChildren();
    const all = o.choices();
    for (const g of o.groups) {
      const mine = all.filter((c) => c.group === g);
      if (!mine.length) continue;
      const group = el("div", "kind-group");
      group.setAttribute("role", "group");
      group.setAttribute("aria-label", g);
      group.append(el("div", "kind-head", g));
      for (const c of mine) {
        const it = el("div", "kind-item");
        it.id = `${o.list.id}-${c.id}`;
        it.dataset.kind = c.id;
        it.setAttribute("role", "option");
        const glyph = el("span", "kind-icon");
        glyph.innerHTML = c.icon; // constant SVG markup from the caller, never user text
        const text = el("span", "kind-text");
        text.append(el("span", "kind-name", c.name), el("span", "kind-about", c.about));
        const mark = el("span", "kind-check");
        mark.innerHTML = CHECK;
        it.append(glyph, text, mark);
        it.onclick = () => {
          pick(c.id);
          o.list.focus();
        };
        group.append(it);
      }
      o.list.append(group);
    }
    o.list.append(el("p", "kind-none", o.none));
    o.search.value = "";
    filter();
    pick(all.some((c) => c.id === selected) ? selected : (all[0]?.id ?? ""));
  }

  /** Only the choices whose name or line has the search in it; an empty group hides,
   * and a hidden choice moves to the first one shown. */
  function filter() {
    const q = o.search.value.trim().toLowerCase();
    const all = o.choices();
    for (const it of o.list.querySelectorAll(".kind-item") as NodeListOf<HTMLElement>) {
      const c = all.find((x) => x.id === it.dataset.kind)!;
      it.hidden = !!q && !`${c.name} ${c.about}`.toLowerCase().includes(q);
    }
    for (const g of o.list.querySelectorAll(".kind-group") as NodeListOf<HTMLElement>)
      g.hidden = !g.querySelector(".kind-item:not([hidden])");
    const shown = items();
    $(".kind-none", o.list).hidden = shown.length > 0;
    if (shown.length && !shown.some((it) => it.dataset.kind === chosen)) pick(shown[0].dataset.kind!);
  }

  // One column: down and right go to the next, up and left to the previous.
  const move = (e: KeyboardEvent) => {
    const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
    if (!step || (e.target === o.search && (e.key === "ArrowLeft" || e.key === "ArrowRight"))) return false;
    e.preventDefault();
    const all = items();
    const next = all[all.findIndex((it) => it.dataset.kind === chosen) + step];
    if (next) pick(next.dataset.kind!);
    return true;
  };
  o.search.oninput = filter;
  o.search.onkeydown = move;
  o.list.onkeydown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      if (items().length) o.form.requestSubmit();
      return;
    }
    move(e);
  };
  // Esc clears the search first, then closes.
  o.sheet.addEventListener(
    "keydown",
    (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !o.search.value || o.form.hidden) return;
      e.preventDefault();
      e.stopPropagation();
      o.search.value = "";
      filter();
      o.search.focus();
    },
    true,
  );
  return { draw, pick, filter, chosen: () => chosen, groups: o.groups };
}
