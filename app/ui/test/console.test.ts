// The follow rule every log view shares (console.ts `follower`), on an element whose
// scroll geometry is faked: happy-dom lays nothing out.
import { describe, expect, test } from "vite-plus/test";
import { follower } from "../src/console";

function pane(newestFirst = false) {
  const out = document.createElement("pre");
  const g = { height: 1000, top: 0 };
  Object.defineProperty(out, "scrollHeight", { get: () => g.height });
  Object.defineProperty(out, "clientHeight", { get: () => 200 });
  Object.defineProperty(out, "scrollTop", {
    get: () => g.top,
    set: (v) => (g.top = Math.max(0, Math.min(v, g.height - 200))),
  });
  document.body.append(out);
  const f = follower(out, newestFirst);
  /** The reader scrolls to `top`: the position changes, then its event comes. */
  const scroll = (top: number) => {
    out.scrollTop = top;
    out.dispatchEvent(new Event("scroll"));
  };
  const wheel = (deltaY: number) => out.dispatchEvent(new WheelEvent("wheel", { deltaY }));
  const pill = () => out.querySelector<HTMLElement>(".follow-pill");
  return { out, g, f, scroll, wheel, pill };
}

describe("follower", () => {
  test("at the end it follows new output; its own scrolls don't count as the reader's", () => {
    const { out, g, f, pill } = pane();
    f.follow();
    expect(out.scrollTop).toBe(800);
    g.height = 1500;
    f.follow();
    expect(out.scrollTop).toBe(1300);
    out.dispatchEvent(new Event("scroll")); // the event of the scroll made here
    expect(f.atEnd).toBe(true);
    expect(pill()).toBeNull();
  });

  test("a wheel up pauses at once, before its scroll arrives; output then shows the pill", () => {
    const { out, g, f, wheel, pill } = pane();
    f.follow();
    wheel(-3);
    expect(f.atEnd).toBe(false);
    g.height = 1400;
    f.follow();
    expect(out.scrollTop).toBe(800); // not moved
    expect(pill()?.dataset.label).toBe("↓ New output");
    expect(out.lastElementChild).toBe(pill());
    expect(pill()!.textContent).toBe(""); // the label is CSS: the log's text stays its own
  });

  test("scrolled up a little: paused; restore and ResizeObserver-like redraws keep the place", () => {
    const { out, g, f, scroll } = pane();
    f.follow();
    scroll(790);
    expect(f.atEnd).toBe(false);
    g.height = 2000;
    f.follow();
    f.restore();
    expect(out.scrollTop).toBe(790);
  });

  test("within a few px of the end counts as the end; scrolling back resumes and hides the pill", () => {
    const { out, g, f, scroll, wheel, pill } = pane();
    f.follow();
    scroll(797);
    expect(f.atEnd).toBe(true);
    wheel(-5);
    scroll(500);
    g.height = 1200;
    f.follow();
    expect(pill()).not.toBeNull();
    scroll(1000);
    expect(f.atEnd).toBe(true);
    expect(pill()).toBeNull();
    g.height = 1300;
    f.follow();
    expect(out.scrollTop).toBe(1100);
  });

  test("the pill jumps to the end and follows again", () => {
    const { out, g, f, scroll, pill } = pane();
    scroll(100);
    g.height = 1200;
    f.follow();
    pill()!.click();
    expect(f.atEnd).toBe(true);
    expect(out.scrollTop).toBe(1000);
    expect(pill()).toBeNull();
  });

  test("paused, lines trimmed before the view leave what the reader sees in place", () => {
    const { out, g, f, scroll } = pane();
    scroll(500);
    f.keep(() => (g.height = 700)); // 300 px of old lines gone
    expect(out.scrollTop).toBe(200);
    f.end();
    f.keep(() => (g.height = 600)); // following: the end, as before
    f.follow();
    expect(out.scrollTop).toBe(400);
  });

  test("newest first: it follows the top; a wheel down pauses; new rows on top don't push the view", () => {
    const { out, g, f, wheel, pill } = pane(true);
    f.follow();
    expect(out.scrollTop).toBe(0);
    wheel(4);
    out.scrollTop = 300;
    out.dispatchEvent(new Event("scroll"));
    expect(f.atEnd).toBe(false);
    f.keep(() => (g.height = 1100)); // a row of 100 px on top
    f.follow();
    expect(out.scrollTop).toBe(400);
    expect(out.firstElementChild).toBe(pill());
    expect(pill()!.dataset.label).toBe("↑ New entries");
    pill()!.click();
    expect(out.scrollTop).toBe(0);
  });
});
