import { readFileSync } from "node:fs";
import { resolve } from "node:path";
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
    expect(clean("a\r\n\x1b[?25lb\x1b[3;1Hc")).toBe("a\nbc");
  });
});

describe("clean follows cursor moves on a line", () => {
  // Symfony's ProgressBar (composer install): Cursor::moveToColumn(1), then clearLine().
  test("composer's progress bar is one bar, as it ends", () => {
    const bar = (n: number, fill: string, pct: string) => ` ${String(n).padStart(3)}/109 [${fill}]  ${pct}%`;
    const raw =
      "Installing dependencies from lock file\n" +
      bar(0, ">" + "-".repeat(27), "  0") +
      "\x1b[1G\x1b[2K" +
      bar(26, "======>" + "-".repeat(21), " 23") +
      "\x1b[1G\x1b[2K" +
      bar(109, "=".repeat(28), "100") +
      "\n";
    expect(clean(raw)).toBe(`Installing dependencies from lock file\n${bar(109, "=".repeat(28), "100")}\n`);
    // Unfinished, as the Dev tab draws its live line.
    expect(clean(raw.slice(0, raw.lastIndexOf("\x1b[1G")))).toBe(
      `Installing dependencies from lock file\n${bar(26, "======>" + "-".repeat(21), " 23")}`,
    );
  });
  // npm 10's spinner: readline's cursorTo(0) (ESC[1G) and clearLine(1) (ESC[0K).
  test("npm's spinner leaves only its last frame, and the line after it", () => {
    expect(clean("⠋\x1b[1G\x1b[0K⠙\x1b[1G\x1b[0K⠹\x1b[1G\x1b[0K\nadded 3 packages")).toBe("\nadded 3 packages");
    expect(clean("⠋\x1b[1G\x1b[0K⠙")).toBe("⠙");
  });
  // pnpm (log-update, ansi-escapes' eraseLines(2)): ESC[2K ESC[1A ESC[2K ESC[G.
  test("pnpm's two-line redraw goes up a line", () => {
    const raw = "Progress: resolved 1\nPackages: +3\x1b[2K\x1b[1A\x1b[2K\x1b[GProgress: resolved 9\nPackages: +9";
    expect(clean(raw)).toBe("Progress: resolved 9\nPackages: +9");
    // On one line (the Dev tab draws line by line) up stays on it.
    expect(clean("abc\x1b[1A\x1b[2K\x1b[Gxy")).toBe("xy");
  });
  test("columns, forward and back, partial erases", () => {
    expect(clean("abcdef\x1b[3Gx")).toBe("abxdef");
    expect(clean("abcdef\x1b[3G\x1b[K")).toBe("ab");
    expect(clean("abcdef\x1b[3G\x1b[1K")).toBe("   def");
    expect(clean("ab\x1b[2Cx")).toBe("ab  x");
    expect(clean("abcd\x1b[2Dx")).toBe("abxd");
    expect(clean("\x1b[5Gx")).toBe("    x");
  });
  test("\\r starts over only once something follows", () => {
    expect(clean("long line\rshort")).toBe("short");
    expect(clean("kept\r")).toBe("kept");
  });
  test("an erased color's reset is kept, so the color doesn't run on", () => {
    expect(clean("\x1b[32m 50%\x1b[0m\x1b[1G\x1b[2K100%", true)).toBe("\x1b[0m100%");
    const div = document.createElement("div");
    div.append(colorize(clean("\x1b[32mgo\x1b[0m\x1b[1G\x1b[2Kdone\nnext", true)));
    expect(div.querySelector(".ansi2")).toBeNull();
    expect(div.textContent).toBe("done\nnext");
  });
  test("redraws don't pile codes up", () => {
    const raw = Array.from({ length: 500 }, (_, i) => `\x1b[1G\x1b[2K\x1b[36m${i}\x1b[0m`).join("");
    expect(clean(raw)).toBe("499");
    expect(clean(raw, true).length).toBeLessThan(40);
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

describe("progress and badges", () => {
  const render = (text: string) => {
    const div = document.createElement("div");
    div.append(colorize(clean(text, true)));
    return div;
  };

  test("backspaces between color resets overwrite, as docker logs of a counter show them", () => {
    // The real bytes of elpida-sphere's counter: ESC[0m pairs around every \b.
    const r = "\x1b[0m\x1b[0m";
    let raw = `${r}0%`;
    for (let n = 1; n <= 100; n++) raw += `${r}\b`.repeat(String(n - 1).length + 1) + `${r}${n}%`;
    raw += "\n";
    expect(clean(raw)).toBe("100%\n");
    const div = render(raw);
    expect(div.textContent).toBe("100%\n");
    expect(div.textContent).not.toContain("[0");
    // The same with \r.
    expect(render("0%\r\x1b[0m1%\r\x1b[32m100%\x1b[0m\n").textContent).toBe("100%\n");
  });

  test("badges keep their contrast: black on green, white on blue, black on cyan and yellow", () => {
    const css = document.createElement("style");
    css.textContent = readFileSync(resolve(import.meta.dirname, "../style.css"), "utf8");
    document.head.append(css);
    const badge = (text: string) => {
      const div = render(text);
      document.body.append(div);
      const span = div.querySelector("span")!;
      return [span.textContent, getComputedStyle(span).color, getComputedStyle(span).backgroundColor];
    };
    // Pint's PASS: bright black on green, bold.
    const [pass, fg] = badge("  \x1b[90;42;1m  PASS  \x1b[39;49;22m 1 file");
    expect(pass).toBe("  PASS  ");
    expect(["#000", "#000000", "rgb(0, 0, 0)"]).toContain(fg);
    for (const t of ["\x1b[30;42m PASS \x1b[39;49m", "\x1b[30;46m INFO \x1b[0m", "\x1b[30;43m WARN \x1b[0m"])
      expect(["#000", "#000000", "rgb(0, 0, 0)"]).toContain(badge(t)[1]);
    expect(["#fff", "#ffffff", "rgb(255, 255, 255)"]).toContain(badge("\x1b[37;44m INFO \x1b[0m")[1]);
    css.remove();
  });

  test("inverse swaps the colors, and the default ones too", () => {
    const span = render("\x1b[7m sel \x1b[27m").querySelector("span")!;
    expect(span.className).toBe("ansi0 ansi-bg7");
    expect(render("\x1b[32;7mok\x1b[0m").querySelector("span")!.className).toBe("ansi0 ansi-bg2");
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
    const div = render(
      "  ➜  Local:   \x1b[36mhttp://localhost:\x1b[1m5174\x1b[22m/\x1b[39m\nsee https://laravel.com/docs, then http://shop.test.",
    );
    expect(links(div).map((a) => a.dataset.url)).toEqual(["http://localhost:5174/", "https://laravel.com/docs", "http://shop.test"]);
    expect(links(div)[0].textContent).toBe("http://localhost:5174/");
    expect(links(div)[0].querySelector(".ansi-b")?.textContent).toBe("5174");
    expect(links(div)[0].title).toBe("⌘-click to open");
    expect(div.textContent).toBe("  ➜  Local:   http://localhost:5174/\nsee https://laravel.com/docs, then http://shop.test.");
  });

  test("many URLs cost one pass: 5000 lines, each with a colored URL, well under the old minutes", () => {
    const text = Array.from({ length: 5000 }, (_, i) => `\x1b[32m10:00:0${i % 10}\x1b[39m GET http://localhost:5173/f${i}.ts 200`).join(
      "\n",
    );
    const t = performance.now();
    const div = render(text);
    expect(performance.now() - t).toBeLessThan(5000);
    expect(links(div).length).toBe(5000);
    expect(links(div).at(-1)!.dataset.url).toBe("http://localhost:5173/f4999.ts");
    expect(div.textContent).toBe(clean(text));
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
