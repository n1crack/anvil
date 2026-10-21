/* The in-row confirm, in every window: Settings, an app's Resources, an inbox's Clear;
 * and the keys of a small bubble. */

/** A bubble's keys: Tab and Shift+Tab go round its controls, Esc calls `close`. */
export function bubbleKeys(pop: HTMLElement, close: () => void) {
  pop.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      return close();
    }
    if (e.key !== "Tab") return;
    const all = [...pop.querySelectorAll<HTMLElement>("button, a[href], input, select, textarea, [tabindex]")].filter(
      (x) => x.tabIndex >= 0 && !(x as HTMLButtonElement).disabled && x.getClientRects().length > 0,
    );
    const [first, last] = [all[0], all[all.length - 1]];
    if (first && document.activeElement === (e.shiftKey ? first : last)) {
      e.preventDefault();
      (e.shiftKey ? last : first).focus();
    }
  });
}

/** A quiet text action at a row's end: no button frame, brighter on hover. */
export function textAct(label: string, onClick: () => void) {
  const b = Object.assign(document.createElement("button"), { className: "text-act", textContent: label, type: "button" });
  b.onclick = onClick;
  return b;
}

/** Asks in the row itself, no dialog: the question, Cancel, and the action in red text. */
export function confirmInRow(box: HTMLElement, question: string, label: string, onYes: () => void, restore: () => void) {
  const yes = textAct(label, () => {
    yes.disabled = no.disabled = true;
    onYes();
  });
  yes.classList.add("danger");
  const no = textAct("Cancel", restore);
  box.classList.add("asking");
  box.replaceChildren(
    Object.assign(document.createElement("span"), { className: "muted", textContent: question, title: question }),
    no,
    yes,
  );
  // Back to the row's actions once the mouse is elsewhere and nothing was chosen.
  box
    .closest(".row")
    ?.addEventListener("mouseleave", () => no.isConnected && !no.disabled && (box.classList.remove("asking"), restore()), { once: true });
}
