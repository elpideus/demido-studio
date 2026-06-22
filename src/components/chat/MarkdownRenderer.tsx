import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remarkMath from 'remark-math'
import rehypeHighlight from 'rehype-highlight'
import rehypeKatex from 'rehype-katex'
import katex from 'katex'
import DOMPurify from 'dompurify'
import 'katex/dist/katex.min.css'
import { MermaidBlock } from './MermaidBlock'

function LatexBlock({ code }: { code: string }) {
  // Split on comment lines, render comments as text and math as KaTeX
  const parts = code.split('\n')
  const segments: { type: 'comment' | 'math'; text: string }[] = []
  let mathLines: string[] = []

  for (const line of parts) {
    const commentMatch = line.match(/^\s*%+\s*(.*)$/)
    if (commentMatch) {
      if (mathLines.length) { segments.push({ type: 'math', text: mathLines.join('\n') }); mathLines = [] }
      if (commentMatch[1].trim()) segments.push({ type: 'comment', text: commentMatch[1].trim() })
    } else {
      mathLines.push(line)
    }
  }
  if (mathLines.join('\n').trim()) segments.push({ type: 'math', text: mathLines.join('\n') })

  return (
    <div className="my-2 space-y-1">
      {segments.map((seg, i) =>
        seg.type === 'comment' ? (
          <p key={i} className="text-sm text-muted-foreground italic px-2">{seg.text}</p>
        ) : (
          <div key={i} className="flex justify-center overflow-x-auto" dangerouslySetInnerHTML={{
            __html: DOMPurify.sanitize(katex.renderToString(seg.text.trim(), { throwOnError: false, displayMode: true }))
          }} />
        )
      )}
    </div>
  )
}

const components = {
  code({ className, children, ...props }: React.HTMLAttributes<HTMLElement> & { inline?: boolean }) {
    const lang = /language-(\w+)/.exec(className ?? '')?.[1]
    const code = String(children).replace(/\n$/, '')
    if (lang === 'mermaid') return <MermaidBlock code={code} />
    if (lang === 'latex' || lang === 'tex') return <LatexBlock code={code} />
    const isInline = !lang && !code.includes('\n')
    if (isInline) return (
      <code
        className="px-1.5 py-0.5 rounded text-[0.8em] font-mono"
        style={{ background: 'rgba(0,0,0,0.25)' }}
        {...props}
      >{children}</code>
    )
    return <code className={className} {...props}>{children}</code>
  },
}

export function MarkdownRenderer({ children }: { children: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeHighlight, rehypeKatex]}
      components={components as never}
    >
      {children}
    </ReactMarkdown>
  )
}
