import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags } from "@lezer/highlight";

// colors come from --syntax-* variables in app.css so the editor follows the light/dark theme without reconfiguring
const highlight = HighlightStyle.define([
  { tag: [tags.keyword, tags.controlKeyword, tags.operatorKeyword, tags.modifier], color: "var(--syntax-keyword)" },
  { tag: [tags.string, tags.special(tags.string), tags.regexp], color: "var(--syntax-string)" },
  { tag: [tags.comment, tags.lineComment, tags.blockComment], color: "var(--syntax-comment)", fontStyle: "italic" },
  { tag: [tags.number, tags.bool, tags.null, tags.atom], color: "var(--syntax-number)" },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName), tags.definition(tags.function(tags.variableName))], color: "var(--syntax-function)" },
  { tag: [tags.typeName, tags.className, tags.namespace], color: "var(--syntax-type)" },
  { tag: [tags.propertyName, tags.attributeName], color: "var(--syntax-property)" },
  { tag: [tags.heading], fontWeight: "600", color: "var(--syntax-keyword)" },
  { tag: [tags.link, tags.url], color: "var(--syntax-string)", textDecoration: "underline" },
]);

const theme = EditorView.theme({
  "&": { height: "100%", backgroundColor: "transparent", color: "var(--color-ink)", fontSize: "12px" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.6" },
  ".cm-content": { caretColor: "var(--color-ink)", padding: "12px 0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-ink)" },
  ".cm-gutters": { backgroundColor: "transparent", color: "var(--color-muted)", border: "none", paddingLeft: "8px" },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--color-subtle) 70%, transparent)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--color-ink)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "color-mix(in srgb, var(--color-accent) 25%, transparent)" },
  ".cm-matchingBracket": { backgroundColor: "var(--color-subtle)", outline: "1px solid var(--color-line)" },
  ".cm-panels": { backgroundColor: "var(--color-surface)", color: "var(--color-ink)", borderColor: "var(--color-line)" },
  ".cm-tooltip": { backgroundColor: "var(--color-surface)", border: "1px solid var(--color-line)", borderRadius: "8px" },
});

export const editorTheme = [theme, syntaxHighlighting(highlight)];
