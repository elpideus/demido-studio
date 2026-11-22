/* Text written by someone else (a model, a model card), drawn as Markdown and never as HTML. */
import { useEffect, useState, type ReactNode } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

/** A block of code in the language it names, highlighted once the highlighter and the language are loaded;
    until then, and in a language Demido does not know, as it is. Inline code has no language, and is as it is. */
function Code({ className, children }: { className?: string; children?: ReactNode }) {
  const language = /language-([\w+#.-]+)/.exec(className ?? '')?.[1]
  const text = typeof children === 'string' ? children : Array.isArray(children) ? children.join('') : ''
  const [pieces, setPieces] = useState<{ text: string; pieces: [string, string][] } | null>(null)
  useEffect(() => {
    if (!language || !text) return
    let alive = true
    import('./highlight').then((m) => m.highlighted(text, language)).then((found) => { if (alive && found) setPieces({ text, pieces: found }) }, () => {})
    return () => { alive = false }
  }, [language, text])
  if (!language || !pieces || pieces.text !== text) return <code className={className}>{children}</code>
  return <code className={className}>{pieces.pieces.map(([piece, classes], i) => (classes ? <span key={i} className={classes}>{piece}</span> : piece))}</code>
}

/** A model's words, as markdown. Never as HTML: whatever a model writes is text to be shown, not code to be run.

    A picture is never loaded from an address in an answer. A browser fetches a picture by itself, and the
    address can carry text: a file that told a model to end its answer with a picture at
    `…/?d=<what it had read>` would have sent that away with no tool used at all. The address is shown instead.
    A link is only followed when the person presses it, and it opens outside Demido. */
export function Prose({ text }: { text: string }) {
  return (
    <div className="d-prose">
      <Markdown
        remarkPlugins={[remarkGfm]}
        skipHtml
        components={{
          img: ({ src, alt }) => <span className="d-type-caption d-text-tertiary">[A picture{alt ? ` (${alt})` : ''} that Demido did not load: <code>{typeof src === 'string' ? src : ''}</code>]</span>,
          a: ({ href, children }) => <a href={href} title={href} target="_blank" rel="noreferrer noopener">{children}</a>,
          code: ({ className, children }) => <Code className={className}>{children}</Code>,
        }}
      >{text}</Markdown>
    </div>
  )
}
