import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { json } from "@codemirror/lang-json";
import { HighlightStyle, bracketMatching, syntaxHighlighting } from "@codemirror/language";
import { type Diagnostic, linter, lintGutter } from "@codemirror/lint";
import { tags } from "@lezer/highlight";
import { type Problem } from "./confjson";

/* Configure's JSON editor (CodeMirror 6), loaded only when JSON is chosen. Its colors are
 * the app's, so it follows the light and dark themes. */

const theme = EditorView.theme({
  "&": { height: "100%", background: "var(--steel-raised)", color: "var(--text)", fontSize: "12px" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace', lineHeight: "1.55" },
  ".cm-content": { padding: "8px 0", caretColor: "var(--text)" },
  ".cm-cursor": { borderLeftColor: "var(--text)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { background: "var(--selected) !important" },
  ".cm-activeLine": { background: "color-mix(in srgb, var(--text) 4%, transparent)" },
  ".cm-gutters": { background: "transparent", color: "var(--muted)", border: "none", borderRight: "1px solid var(--line)" },
  ".cm-activeLineGutter": { background: "transparent", color: "var(--text)" },
  ".cm-matchingBracket": { background: "color-mix(in srgb, var(--text) 12%, transparent)", outline: "none" },
  ".cm-tooltip": { background: "var(--steel-raised)", color: "var(--text)", border: "1px solid var(--line)", borderRadius: "6px" },
  ".cm-diagnostic": { padding: "4px 8px", fontSize: "12px" },
  ".cm-diagnostic-error": { borderLeftColor: "var(--danger)" },
  ".cm-lintRange-error": { backgroundImage: "none", textDecoration: "underline wavy var(--danger)", textUnderlineOffset: "3px" },
});

// Calm, neutral colors that read in both themes.
const highlight = HighlightStyle.define([
  { tag: tags.propertyName, color: "var(--text)", fontWeight: "500" },
  { tag: tags.string, color: "color-mix(in srgb, var(--text) 62%, #3b82f6)" },
  { tag: [tags.bool, tags.null, tags.number], color: "color-mix(in srgb, var(--text) 62%, #a855f7)" },
  { tag: [tags.brace, tags.squareBracket, tags.separator, tags.punctuation], color: "var(--muted)" },
]);

/** An editor in `parent` holding `text`; `problems` checks it as it changes (markers in
 * the gutter, underlines, the message on hover) and `changed` hears every edit. */
export function jsonEditor(parent: HTMLElement, text: string, problems: (text: string) => Problem[], changed: (text: string) => void) {
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: text,
      extensions: [
        lineNumbers(),
        highlightActiveLine(),
        history(),
        bracketMatching(),
        keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
        json(),
        syntaxHighlighting(highlight),
        lintGutter(),
        linter((v) => problems(v.state.doc.toString()).map((p): Diagnostic => ({ ...p, severity: "error" })), { delay: 150 }),
        EditorView.updateListener.of((u) => u.docChanged && changed(u.state.doc.toString())),
        EditorView.contentAttributes.of({ "aria-label": "Processes as JSON", spellcheck: "false" }),
        theme,
      ],
    }),
  });
  return {
    view,
    get text() {
      return view.state.doc.toString();
    },
    /** Replace the whole text (Format, Cancel, a change from elsewhere). */
    set(next: string) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next } });
    },
  };
}
