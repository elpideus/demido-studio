/* Text written by someone else (a model, a model card), drawn as Markdown and never as HTML. */
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

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
        }}
      >{text}</Markdown>
    </div>
  )
}
