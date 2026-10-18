import { describe, expect, test } from "vite-plus/test";
import { splitter } from "../src/split";

describe("splitter", () => {
  const drag = (h: HTMLElement, x: number) => {
    h.dispatchEvent(new PointerEvent("pointerdown", { button: 0, clientX: 0 }));
    h.dispatchEvent(new PointerEvent("pointermove", { clientX: x }));
    h.dispatchEvent(new PointerEvent("pointerup", { clientX: x }));
  };
  const make = () => {
    const h = document.createElement("div");
    const seen: (number | null)[] = [];
    splitter(h, { key: "t-width", min: 200, max: () => 500, at: (m) => m.clientX, apply: (w) => seen.push(w) });
    return { h, seen };
  };

  test("a drag resizes within min and max and is kept; a double-click resets", () => {
    localStorage.clear();
    const { h, seen } = make();
    expect(seen).toEqual([]);
    drag(h, 320);
    expect(seen.at(-1)).toBe(320);
    expect(localStorage.getItem("t-width")).toBe("320");
    drag(h, 9000);
    expect(seen.at(-1)).toBe(500);
    drag(h, 10);
    expect(seen.at(-1)).toBe(200);
    // Kept for the next launch.
    expect(make().seen).toEqual([200]);
    h.dispatchEvent(new MouseEvent("dblclick"));
    expect(seen.at(-1)).toBeNull();
    expect(localStorage.getItem("t-width")).toBeNull();
    expect(document.documentElement.classList.contains("col-resizing")).toBe(false);
  });
});
