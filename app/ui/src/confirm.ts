/* The in-row confirm, in every window: Settings, an app's Resources, an inbox's Clear. */

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
