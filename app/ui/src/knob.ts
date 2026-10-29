/* A segmented control's knob: one raised plate under the pressed (or selected) item that
 * slides to the next one. Every window's controls use it (main, Settings, Processes). */

export function knob(box: HTMLElement) {
  const k = document.createElement("span");
  k.className = "knob";
  box.prepend(k);
  // Sized from the item itself: items are as wide as their words. A box shown later
  // (a hidden tab's) is measured when it gets a size.
  let placed = false;
  const place = () => {
    const b = box.querySelector<HTMLElement>(':scope > [aria-pressed="true"], :scope > [aria-selected="true"]');
    k.hidden = !b;
    if (!b) return;
    k.dataset.on = b.dataset.id ?? b.dataset.tab; // not `data-id`: that finds the items
    if (!b.offsetWidth) return; // not laid out yet (built apart or hidden): measured once it is
    // The first place is where it starts: it slides from there, it doesn't grow out of 0.
    if (!placed) k.style.transition = "none";
    k.style.width = `${b.offsetWidth}px`;
    k.style.transform = `translateX(${b.offsetLeft}px)`;
    if (!placed) {
      void k.offsetWidth;
      k.style.transition = "";
      placed = true;
    }
  };
  new MutationObserver(place).observe(box, { subtree: true, attributes: true, attributeFilter: ["aria-pressed", "aria-selected"] });
  new ResizeObserver(place).observe(box);
  place();
}
