import { useEffect, useRef } from "react";
import { languages } from "@codemirror/language-data";
import { Annotation, Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { LanguageDescription } from "@codemirror/language";
import { basicSetup } from "codemirror";

import { editorTheme } from "./editor-theme";

// marks changes pushed in from props so they aren't reported back as edits
const external = Annotation.define<boolean>();

type CodeEditorProps = { path: string; value: string; onChange: (value: string) => void };

export default function CodeEditor({ path, value, onChange }: CodeEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    if (!host.current) return;
    const language = new Compartment();
    const editor = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          basicSetup,
          editorTheme,
          language.of([]),
          EditorView.contentAttributes.of({ "aria-label": `Edit ${path}` }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !update.transactions.some((transaction) => transaction.annotation(external))) {
              onChangeRef.current(update.state.doc.toString());
            }
          }),
        ],
      }),
    });
    view.current = editor;

    let cancelled = false;
    LanguageDescription.matchFilename(languages, path)
      ?.load()
      .then((support) => !cancelled && editor.dispatch({ effects: language.reconfigure(support) }))
      .catch((error) => console.warn("could not load editor language", error));

    return () => {
      cancelled = true;
      editor.destroy();
      view.current = null;
    };
    // rebuilt per file only; value changes are synced by the effect below
  }, [path]);

  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    const current = editor.state.doc.toString();
    if (current !== value) editor.dispatch({ changes: { from: 0, to: current.length, insert: value }, annotations: external.of(true) });
  }, [value]);

  return <div ref={host} className="h-full min-h-0 overflow-hidden" />;
}
