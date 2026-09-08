import ReactMarkdown from 'react-markdown'
import remarkBreaks from 'remark-breaks'
import remarkGfm from 'remark-gfm'

import styles from './Markdown.module.css'

/**
 * An answer, rendered.
 *
 * Brief B16: "Markdown, LaTeX and Mermaid supported in the chat bubbles."
 *
 * Markdown and fenced code, and neither of the other two. #34 puts them out of
 * this slice's scope in as many words: an answer is unreadable without headings,
 * lists and code, and LaTeX and Mermaid have no decision ticket at all, so they
 * are not this slice's to invent.
 *
 * ## Raw HTML is not rendered, and that is a security boundary
 *
 * `react-markdown` does not parse embedded HTML unless a plugin is added, and
 * none is. What arrives in this component is text a language model generated,
 * which is the least trustworthy string in the application, and the window it
 * would be running inside can call every command in `src-tauri/src/lib.rs`.
 *
 * A link is drawn and does not navigate, for the same reason: this window is
 * the application, and following a link in it would replace the desk with a web
 * page. The embedded browser is where a link goes, and it is its own ticket.
 *
 * ## A newline is a line break
 *
 * `remark-breaks` is the one departure from CommonMark here, and it is the
 * difference between an answer and a wall. Markdown folds a single newline into
 * a space, which is right for a document somebody wrote in a file and wrong for
 * a model asked to put one item per line: it answers with newlines and the page
 * renders one paragraph. Measured on the development tier the first time
 * anything was driven through this window, on an answer that counted.
 */
export function Markdown({ children }: { children: string }) {
  return (
    <div className={styles.prose}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkBreaks]}
        components={{
          // A fence is a recess in the bubble, and the recess is the `pre`
          // rather than the `code` inside it, so a block does not draw two
          // nested wells (docs/rules/surfaces.md, rule 3).
          pre: ({ children }) => <pre className={styles.fence}>{children}</pre>,
          code: ({ className, children, ...rest }) => (
            <code {...rest} className={[styles.code, className].filter(Boolean).join(' ')}>
              {children}
            </code>
          ),
          a: ({ children, href }) => (
            <a
              className={styles.link}
              href={href}
              title={href}
              onClick={(event) => event.preventDefault()}
            >
              {children}
            </a>
          ),
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  )
}
