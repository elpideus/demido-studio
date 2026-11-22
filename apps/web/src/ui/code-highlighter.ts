/* What each piece of code is called, for highlighting: one list for the editor and for code in an answer, so
   the two can never differ. A piece gets a class (`tok-keyword`, `tok-string`); the colours are the brand's
   code tokens (`--d-color-code-*`, DECISIONS Q7), set in app.css. A tag not named here is the text's own
   colour: variables, properties, plain text. */
import { tagHighlighter, tags as t } from '@lezer/highlight'

export const codeHighlighter = tagHighlighter([
  { tag: [t.keyword, t.controlKeyword, t.operatorKeyword, t.definitionKeyword, t.moduleKeyword, t.modifier, t.self], class: 'tok-keyword' },
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], class: 'tok-comment' },
  { tag: [t.string, t.special(t.string), t.character, t.attributeValue], class: 'tok-string' },
  { tag: [t.regexp, t.escape], class: 'tok-string tok-special' },
  { tag: [t.number, t.integer, t.float, t.bool, t.null, t.atom, t.unit], class: 'tok-number' },
  { tag: [t.typeName, t.className, t.namespace, t.attributeName], class: 'tok-type' },
  { tag: [t.definition(t.variableName), t.function(t.definition(t.variableName)), t.definition(t.typeName), t.definition(t.className)], class: 'tok-definition' },
  { tag: [t.punctuation, t.bracket, t.operator, t.separator, t.derefOperator], class: 'tok-punctuation' },
  { tag: [t.meta, t.processingInstruction, t.annotation], class: 'tok-meta' },
  // Pages and other markup
  { tag: t.tagName, class: 'tok-keyword' },
  // Markdown, as it reads
  { tag: [t.heading, t.heading1, t.heading2, t.heading3], class: 'tok-heading' },
  { tag: [t.heading4, t.heading5, t.heading6], class: 'tok-heading tok-minor' },
  { tag: t.strong, class: 'tok-strong' },
  { tag: t.emphasis, class: 'tok-emphasis' },
  { tag: t.strikethrough, class: 'tok-strikethrough' },
  { tag: [t.link, t.url], class: 'tok-link' },
  { tag: t.quote, class: 'tok-quote' },
  { tag: t.monospace, class: 'tok-string' },
  { tag: t.contentSeparator, class: 'tok-punctuation' },
  { tag: t.invalid, class: 'tok-invalid' },
])
