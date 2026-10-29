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
  // An app's kind on its tile until its own icon loads: abstract marks, not the logos.
  folder: '<path d="M2 4.5a1 1 0 0 1 1-1h3l1.5 1.5H13a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z"/>',
  compose: '<path d="M8 2.25 14 5.25 8 8.25 2 5.25z"/><path d="m2 8.25 6 3 6-3M2 11.25l6 3 6-3"/>',
  laravel: '<path d="M8 1.75 2.75 4.75v6.5L8 14.25l5.25-3v-6.5z"/><path d="M2.75 4.75 8 7.75l5.25-3M8 7.75v6.5"/>',
  php: '<ellipse cx="8" cy="8" rx="6.5" ry="4.75"/><path d="M6.25 6.5 4.75 8l1.5 1.5M9.75 6.5 11.25 8l-1.5 1.5"/>',
  vite: '<path d="M9.25 1.75 3.5 9H8l-1.25 5.25L12.5 7H8z"/>',
  node: '<path d="M4.75 2.5h6.5L14.5 8l-3.25 5.5h-6.5L1.5 8z"/><circle cx="8" cy="8" r="1.5"/>',
  box: '<rect x="2.25" y="2.75" width="11.5" height="10.5" rx="2"/><path d="M2.25 6h11.5"/>',
};
// Kinds drawn alike: PHP frameworks as PHP, the JavaScript servers as Node; the rest a box.
const KIND_GLYPHS: Record<string, keyof typeof ICONS> = {
  folder: "folder",
  compose: "compose",
  laravel: "laravel",
  statamic: "php",
  symfony: "php",
  wordpress: "php",
  php: "php",
  vite: "vite",
  node: "node",
  next: "node",
  nuxt: "node",
};
/** An app kind's monochrome glyph, for its tile while it has no icon of its own. */
export const kindGlyph = (kind: string) => icon(KIND_GLYPHS[kind] ?? "box");
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
