import { describe, expect, test } from "vite-plus/test";
import { ansi, clean, colorize } from "../src/term";

describe("clean", () => {
  test("keeps what a terminal would show after \\r and backspaces", () => {
    expect(clean("12%\r50%\r100%")).toBe("100%");
    expect(clean("abX\bc")).toBe("abc");
    expect(clean("a\x01b\tc")).toBe("ab\tc");
  });
  test("drops escape codes", () => {
    expect(clean("\x1b[31mred\x1b[0m ok")).toBe("red ok");
    expect(clean("\x1b[2Kline")).toBe("line");
  });
});

describe("ansi", () => {
  const render = (text: string) => {
    const div = document.createElement("div");
    div.append(ansi(text));
    return div;
  };
  test("colored runs become spans; text stays text", () => {
    const div = render("plain \x1b[91mbright <red>\x1b[0m done");
    expect(div.textContent).toBe("plain bright <red> done");
    const span = div.querySelector("span.ansi9");
    expect(span?.textContent).toBe("bright <red>");
    expect(div.innerHTML).not.toContain("\x1b");
  });
  test("bold, and reset ends it", () => {
    const div = render("\x1b[1mB\x1b[0m n");
    expect(div.querySelector("span.ansi-b")?.textContent).toBe("B");
    expect(div.lastChild?.textContent).toBe(" n");
  });
});

describe("ansi colors beyond the 16", () => {
  const render = (text: string) => {
    const div = document.createElement("div");
    div.append(ansi(text));
    return div.firstElementChild as HTMLElement;
  };
  test("backgrounds, 256 and RGB colors", () => {
    expect(render("\x1b[37;44m INFO \x1b[39;49m").className).toBe("ansi7 ansi-bg4");
    expect(render("\x1b[38;2;147;197;253mserver\x1b[0m").style.color).toBe("rgb(147, 197, 253)");
    expect(render("\x1b[38;5;196mred\x1b[0m").style.color).toBe("rgb(255, 0, 0)");
    expect(render("\x1b[38;5;9mred\x1b[0m").className).toBe("ansi9");
    expect(render("\x1b[2;3;4mx\x1b[22;23;24m").className).toBe("ansi-dim ansi-i ansi-u");
  });
});

describe("colorize", () => {
  const render = (text: string) => {
    const div = document.createElement("div");
    div.append(colorize(text));
    return div;
  };
  test("a plain line's leading time is muted and its level words colored", () => {
    const text = "[2026-10-17 10:00:01] local.WARNING: slow\n[17-Oct-2026 10:00:02] NOTICE: fpm is running";
    const div = render(text);
    expect([...div.querySelectorAll(".term-time")].map((s) => s.textContent)).toEqual(["[2026-10-17 10:00:01]", "[17-Oct-2026 10:00:02]"]);
    expect(div.querySelector(".ansi3")?.textContent).toBe("WARNING");
    expect(div.querySelector(".ansi6")?.textContent).toBe("NOTICE");
    expect(div.textContent).toBe(text);
  });
  test("lines with escape codes keep theirs, carried to the next line", () => {
    const div = render("\x1b[31mERROR one\ntwo\x1b[0m\nINFORMATION at 10:00:00");
    expect([...div.querySelectorAll(".ansi1")].map((s) => s.textContent)).toEqual(["ERROR one", "two"]);
    // Not a whole word, and no leading time: left alone.
    expect(div.querySelector(".ansi4, .term-time")).toBeNull();
  });
});

describe("links", () => {
  const render = (text: string) => {
    const div = document.createElement("div");
    div.append(colorize(clean(text, true)));
    document.body.append(div);
    return div;
  };
  const links = (div: HTMLElement) => [...div.querySelectorAll<HTMLElement>(".term-link")];

  test("URLs are found, also when a color splits one; text stays the same", () => {
    const div = render("  ➜  Local:   \x1b[36mhttp://localhost:\x1b[1m5174\x1b[22m/\x1b[39m\nsee https://laravel.com/docs, then http://shop.test.");
    expect(links(div).map((a) => a.dataset.url)).toEqual(["http://localhost:5174/", "https://laravel.com/docs", "http://shop.test"]);
    expect(links(div)[0].textContent).toBe("http://localhost:5174/");
    expect(links(div)[0].querySelector(".ansi-b")?.textContent).toBe("5174");
    expect(links(div)[0].title).toBe("⌘-click to open");
    expect(div.textContent).toBe("  ➜  Local:   http://localhost:5174/\nsee https://laravel.com/docs, then http://shop.test.");
  });

  test("⌘-click opens through open_url; a plain click opens nothing", async () => {
    const calls: [string, any][] = [];
    (window as any).__TAURI__ = { core: { invoke: async (cmd: string, args: any) => void calls.push([cmd, args]) } };
    const a = links(render("at http://localhost:5173/ now"))[0];
    a.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(calls).toEqual([]);
    a.dispatchEvent(new MouseEvent("click", { bubbles: true, metaKey: true }));
    expect(calls).toEqual([["open_url", { url: "http://localhost:5173/" }]]);
  });

  test("⌘ held marks the page so a hovered link underlines", () => {
    dispatchEvent(new KeyboardEvent("keydown", { key: "Meta", metaKey: true }));
    expect(document.documentElement.classList.contains("link-mod")).toBe(true);
    dispatchEvent(new KeyboardEvent("keyup", { key: "Meta" }));
    expect(document.documentElement.classList.contains("link-mod")).toBe(false);
  });
});
