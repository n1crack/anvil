/* A segmented control's knob: one raised plate under the pressed (or selected) item that
 * slides to the next one. Every window's controls use it (main, Settings, Processes). */

export function knob(box: HTMLElement) {
  const k = document.createElement("span");
  k.className = "knob";
  box.prepend(k);
  // Sized from the item itself: items are as wide as their words. A box shown later
  // (a hidden tab's) is measured when it gets a size.
  const place = () => {
    const b = box.querySelector<HTMLElement>(':scope > [aria-pressed="true"], :scope > [aria-selected="true"]');
    k.hidden = !b;
    if (!b) return;
    k.dataset.on = b.dataset.id ?? b.dataset.tab; // not `data-id`: that finds the items
    k.style.width = `${b.offsetWidth}px`;
    k.style.transform = `translateX(${b.offsetLeft}px)`;
  };
  new MutationObserver(place).observe(box, { subtree: true, attributes: true, attributeFilter: ["aria-pressed", "aria-selected"] });
  new ResizeObserver(place).observe(box);
  place();
}
