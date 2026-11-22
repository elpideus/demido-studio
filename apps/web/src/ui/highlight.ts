/* Code in an answer, highlighted the way the editor highlights it (the same classes: ui/code-highlighter.ts,
   DECISIONS Q7), in the language its block names. Loaded the first time an answer has a block of code in a
   language; the languages are CodeMirror's, each loaded when a block in it is first seen. The result is pieces
   of text with class names (`tok-keyword`, `tok-comment`), drawn by React as spans: never as HTML. */
import { LanguageDescription, type Language } from '@codemirror/language'
import { languages } from '@codemirror/language-data'
import { highlightCode } from '@lezer/highlight'
import { codeHighlighter } from './code-highlighter'

/** Code longer than this is shown as it is: an answer is not held up for it. */
const MOST_HIGHLIGHTED = 50_000

const loaded = new Map<string, Promise<Language | null>>()

/** The pieces of `code`, each with its classes (`''` for none); `null` when its language is not known. */
export async function highlighted(code: string, name: string): Promise<[string, string][] | null> {
  if (code.length > MOST_HIGHLIGHTED) return null
  let language = loaded.get(name)
  if (!language) {
    const found = LanguageDescription.matchLanguageName(languages, name, true)
    language = found ? found.load().then((support) => support.language, () => null) : Promise.resolve(null)
    loaded.set(name, language)
  }
  const found = await language
  if (!found) return null
  const pieces: [string, string][] = []
  highlightCode(code, found.parser.parse(code), codeHighlighter,(text, classes) => pieces.push([text, classes]), () => pieces.push(['\n', '']))
  return pieces
}
