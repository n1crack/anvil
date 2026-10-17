import { describe, expect, test } from "vite-plus/test";
import { ansi, clean } from "../src/term";

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
