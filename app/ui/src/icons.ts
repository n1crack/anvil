// Outline icons on a 16 box, drawn in the label's color.
const ICONS = {
  run: '<path d="M2.5 4.5 6 8l-3.5 3.5M7.5 12h6"/>',
  configure: '<path d="M2 4.5h7M12 4.5h2M2 11.5h2M7 11.5h7"/><circle cx="10.5" cy="4.5" r="1.5"/><circle cx="5.5" cy="11.5" r="1.5"/>',
  start: '<path d="M4.5 3v10l8.5-5z"/>',
  stop: '<rect x="4" y="4" width="8" height="8" rx="1"/>',
  chevron: '<path d="M4.5 6.5 8 10l3.5-3.5"/>',
  again: '<path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v3h-3"/>',
  plus: '<path d="M8 3v10M3 8h10"/>',
  x: '<path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/>',
  check: '<path d="M3.5 8.5l3 3 6-7"/>',
  more: '<circle cx="3.5" cy="8" r=".75"/><circle cx="8" cy="8" r=".75"/><circle cx="12.5" cy="8" r=".75"/>',
  keys: '<rect x="1.75" y="4" width="12.5" height="8" rx="1.5"/><path d="M4.5 6.75h0M7 6.75h0M9.5 6.75h0M12 6.75h0M5 9.5h6"/>',
};
/** An outline icon; a place's CSS may size or fill it, else it is 12px in the text's color. */
export function icon(name: keyof typeof ICONS) {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 16 16");
  s.setAttribute("aria-hidden", "true");
  const attrs = {
    width: 12,
    height: 12,
    fill: name === "start" ? "currentColor" : "none",
    stroke: "currentColor",
    "stroke-width": 1.5,
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
  };
  for (const [k, v] of Object.entries(attrs)) s.setAttribute(k, String(v));
  s.innerHTML = ICONS[name];
  return s;
}
