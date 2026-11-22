/* A file's text, in an editor: CodeMirror 6 (MIT). Its language is found from its name and loaded only when
   a file of it is opened; a name it does not know is plain text.

   Highlighting has colours (DECISIONS Q7), the brand's code tokens, given by the same classes as code in an
   answer (ui/code-highlighter.ts; the colours are in app.css). */
import { useEffect, useRef, type RefObject } from 'react'
import { Compartment, EditorState } from '@codemirror/state'
import { drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, highlightSpecialChars, keymap, lineNumbers } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { highlightSelectionMatches, search, searchKeymap } from '@codemirror/search'
import { bracketMatching, indentOnInput, LanguageDescription, syntaxHighlighting } from '@codemirror/language'
import { languages } from '@codemirror/language-data'
import { codeHighlighter } from '../../ui/code-highlighter'

const look = EditorView.theme({
  '&': { height: '100%', color: 'var(--d-color-text-primary)', backgroundColor: 'var(--d-color-bg-sunken)', fontSize: 'var(--d-type-code-size)' },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { fontFamily: 'var(--d-font-family-mono)', fontWeight: 'var(--d-font-weight-mono)', lineHeight: 'var(--d-type-code-leading)' },
  '.cm-content': { padding: 'var(--d-space-16) 0', caretColor: 'var(--d-color-text-primary)' },
  '.cm-line': { padding: '0 var(--d-space-16) 0 var(--d-space-8)' },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--d-color-text-primary)', borderLeftWidth: '2px' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': { backgroundColor: 'var(--d-color-fill-selected)' },
  '.cm-gutters': { backgroundColor: 'transparent', color: 'var(--d-color-text-tertiary)', border: 'none', paddingInlineStart: 'var(--d-space-8)' },
  '.cm-activeLine': { backgroundColor: 'var(--d-color-fill-hover)' },
  '.cm-activeLineGutter': { backgroundColor: 'transparent', color: 'var(--d-color-text-secondary)' },
  '.cm-matchingBracket, &.cm-focused .cm-matchingBracket': { backgroundColor: 'var(--d-color-fill-selected)', outline: 'var(--d-stroke-hairline) solid var(--d-color-border-strong)' },
  '.cm-nonmatchingBracket, &.cm-focused .cm-nonmatchingBracket': { color: 'var(--d-color-danger-base)' },
  '.cm-selectionMatch': { backgroundColor: 'var(--d-color-fill-hover)' },
  '.cm-searchMatch': { backgroundColor: 'var(--d-color-fill-selected)', outline: 'var(--d-stroke-hairline) solid var(--d-color-border-strong)' },
  '.cm-searchMatch.cm-searchMatch-selected': { backgroundColor: 'var(--d-color-caution-tint)' },
  '.cm-panels': { backgroundColor: 'var(--d-color-bg-raised)', color: 'var(--d-color-text-primary)' },
  '.cm-panels.cm-panels-bottom': { borderTop: 'var(--d-stroke-hairline) solid var(--d-color-border-hairline)' },
  '.cm-panel.cm-search': { padding: 'var(--d-space-8) var(--d-space-12)', fontFamily: 'var(--d-font-family-sans)' },
  '.cm-panel.cm-search input, .cm-panel.cm-search button': { font: 'inherit', color: 'inherit' },
  '.cm-textfield': { backgroundColor: 'var(--d-color-fill-control)', border: 'var(--d-stroke-hairline) solid var(--d-color-border-default)', borderRadius: 'var(--d-radius-sm)' },
  '.cm-button': { backgroundImage: 'none', backgroundColor: 'var(--d-color-fill-control)', border: 'var(--d-stroke-hairline) solid var(--d-color-border-default)', borderRadius: 'var(--d-radius-sm)' },
})

/** How long typing rests before the rest of the window hears of it again: a long file is not read out at
    every key. The first key of a burst is told at once, so that an edit is known from its start. */
const REST_MS = 120

export interface CodeProps {
  /** The file's name, which says its language. */
  name: string
  /** The text it opens with. The editor keeps its own from then on: give it a new `key` to start again. */
  text: string
  readOnly: boolean
  /** Long lines are wrapped: for prose (Markdown and plain text), not for code. */
  wrap: boolean
  onChange: (text: string) => void
  onSave: () => void
  /** Set to a call that tells what was typed and not yet told, at once: a save makes it first. */
  flush?: RefObject<(() => void) | null>
}

export default function Code({ name, text, readOnly, wrap, onChange, onSave, flush }: CodeProps) {
  const holder = useRef<HTMLDivElement>(null)
  const told = useRef({ onChange, onSave })
  told.current = { onChange, onSave }
  useEffect(() => {
    const place = holder.current
    if (!place) return
    const language = new Compartment()
    let rest: ReturnType<typeof setTimeout> | undefined
    // Typed since the window last heard.
    let more = false
    const tell = () => {
      clearTimeout(rest)
      rest = undefined
      if (more) { more = false; told.current.onChange(view.state.doc.toString()) }
    }
    const view = new EditorView({
      parent: place,
      state: EditorState.create({
        doc: text,
        extensions: [
          lineNumbers(),
          highlightActiveLineGutter(),
          highlightSpecialChars(),
          history(),
          drawSelection(),
          indentOnInput(),
          bracketMatching(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          search({ top: false }),
          syntaxHighlighting(codeHighlighter),
          look,
          language.of([]),
          wrap ? EditorView.lineWrapping : [],
          EditorState.readOnly.of(readOnly),
          EditorState.tabSize.of(2),
          EditorView.contentAttributes.of({ 'aria-label': `${name}, its text${readOnly ? ', not editable here' : ''}` }),
          keymap.of([
            { key: 'Mod-s', preventDefault: true, run: () => { tell(); told.current.onSave(); return true } },
            ...searchKeymap,
            ...historyKeymap,
            // Tab indents; Escape, then Tab, leaves the editor (CodeMirror's own way out for the keyboard).
            indentWithTab,
            ...defaultKeymap,
          ]),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged) return
            if (rest === undefined) told.current.onChange(u.view.state.doc.toString())
            else more = true
            clearTimeout(rest)
            rest = setTimeout(tell, REST_MS)
          }),
        ],
      }),
    })
    if (flush) flush.current = tell
    const found = LanguageDescription.matchFilename(languages, name)
    let alive = true
    found?.load().then((support) => { if (alive) view.dispatch({ effects: language.reconfigure(support) }) }, () => {})
    return () => {
      alive = false
      // What was typed last is not lost to the rest.
      tell()
      if (flush?.current === tell) flush.current = null
      view.destroy()
    }
    // The editor is made once for a file; a new file, or starting again, is a new `key`.
  }, [])
  return <div ref={holder} className="files__edit files__code" />
}
