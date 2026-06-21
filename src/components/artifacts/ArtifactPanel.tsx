import { useState, useEffect, useMemo, useRef, useCallback } from 'react'
import { Document, Page, pdfjs } from 'react-pdf'
import 'react-pdf/dist/Page/AnnotationLayer.css'
import 'react-pdf/dist/Page/TextLayer.css'

pdfjs.GlobalWorkerOptions.workerSrc = new URL('pdfjs-dist/build/pdf.worker.min.mjs', import.meta.url).toString()
import hljs from 'highlight.js'
import { X, Copy, Download, Eye, Code, ChevronLeft, ChevronRight, Pencil, Check, List, GitFork } from 'lucide-react'
import * as fileIconsJs from 'file-icons-js'
import 'file-icons-js/css/style.css'
import { JsonTreeViewer } from './JsonTreeViewer'
import { JsonGraphViewer } from './JsonGraphViewer'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import { Button } from '@/components/ui/button'
import { fs } from '../../lib/tauri'
import { useArtifacts } from '../../stores/artifacts'
import { useMessages } from '../../stores/messages'
import { getExtension, parseArtifacts } from '../../lib/parseArtifacts'
import { cn } from '../../lib/utils'
import { TOOL_CALLS_CONTENT_PREFIX } from '../../lib/constants'
import type { Artifact } from '../../types'

export function ArtifactPanel({ width, isDragging }: { width: number; isDragging?: boolean }) {
  const { activeArtifact, setActive } = useArtifacts()
  const messages = useMessages(s => s.messages)
  const updateMessage = useMessages(s => s.updateMessage)
  const [preview, setPreview] = useState(true)
  const [jsonMode, setJsonMode] = useState<'tree' | 'graph' | 'source'>('tree')
  const [editing, setEditing] = useState(false)
  const [editedContent, setEditedContent] = useState('')
  const [numPages, setNumPages] = useState(0)
  const preRef = useRef<HTMLPreElement>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const onDocumentLoad = useCallback(({ numPages }: { numPages: number }) => setNumPages(numPages), [])

  // Collect all artifacts from messages in order
  const allArtifacts = useMemo((): Artifact[] => {
    const result: Artifact[] = []
    for (const msg of messages) {
      if (msg.role !== 'assistant' || msg.content.startsWith(TOOL_CALLS_CONTENT_PREFIX)) continue
      for (const seg of parseArtifacts(msg.content, msg.id)) {
        if (seg.artifact) result.push(seg.artifact)
      }
    }
    return result
  }, [messages])

  // All versions of the active artifact (matched by identifier or title)
  const versions = useMemo((): Artifact[] => {
    if (!activeArtifact) return []
    const key = activeArtifact.identifier ?? activeArtifact.title.toLowerCase().trim()
    return allArtifacts.filter(a => (a.identifier ?? a.title.toLowerCase().trim()) === key)
  }, [activeArtifact, allArtifacts])

  const versionIndex = useMemo(
    () => versions.findIndex(a => a.id === activeArtifact?.id),
    [versions, activeArtifact]
  )

  useEffect(() => { setPreview(true); setEditing(false); setEditedContent(''); setJsonMode('tree') }, [activeArtifact?.id])

  // Auto-switch to newest version when a new one arrives
  useEffect(() => {
    const newest = versions[versions.length - 1]
    if (newest && newest.id !== activeArtifact?.id) {
      setActive(newest)
    }
  }, [versions.length])

  if (!activeArtifact) return null

  const { type, title, content: originalContent } = activeArtifact
  const content = editedContent || originalContent
  const isHtml = type === 'html'
  const isMd = type === 'markdown' || type === 'md'
  const isPdf = type === 'pdf'
  const isJson = type === 'json' || type === 'jsonc' || type === 'json5' || type.startsWith('json')
  const canPreview = isHtml || isMd

  const handleCopy = () => {
    if (isPdf) {
      const b64 = content.split(',')[1]
      if (b64) fs.copyFileToClipboard(b64, title.endsWith('.pdf') ? title : `${title}.pdf`)
    } else {
      navigator.clipboard.writeText(content)
    }
  }

  const handleDownload = () => {
    const ext = isPdf ? '.pdf' : getExtension(type)
    const slug = title.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '')
    const filename = `${slug || 'artifact'}${ext}`
    if (isPdf) {
      const b64 = content.split(',')[1]
      if (b64) fs.saveFileBase64(filename, b64)
    } else {
      const blob = new Blob([content], { type: 'text/plain' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = filename
      a.click()
      URL.revokeObjectURL(url)
    }
  }

  const highlighted = useMemo(() => {
    if (!editing) return ''
    try {
      return hljs.highlight(editedContent, { language: type }).value
    } catch {
      return hljs.highlightAuto(editedContent).value
    }
  }, [editing, editedContent, type])

  const hasVersions = versions.length > 1

  return (
    <div className="flex flex-col bg-background overflow-hidden" style={{ flex: `0 0 ${width}px`, minWidth: 0 }}>
      {/* Header */}
      <div className="flex items-center gap-2 px-3 py-2.5 border-b border-border shrink-0">
        {(() => { const cls = fileIconsJs.getClassWithColor(`artifact${getExtension(type)}`); return cls ? <span className={cls} style={{ fontSize: 13, lineHeight: 1, display: 'inline-block', width: 13 }} /> : null })()}
        <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-secondary text-muted-foreground border border-border">
          {type}
        </span>
        <span className="flex-1 text-sm font-medium truncate">{title}</span>
        <div className="flex items-center gap-1">
          {hasVersions && (
            <div className="flex items-center gap-0.5 mr-1">
              <Button
                variant="ghost"
                size="icon-xs"
                title="Previous version"
                onClick={() => setActive(versions[versionIndex - 1])}
                disabled={versionIndex <= 0}
                className="text-muted-foreground disabled:opacity-30"
              >
                <ChevronLeft size={13} />
              </Button>
              <span className="text-[10px] text-muted-foreground tabular-nums select-none">
                v{versionIndex + 1}/{versions.length}
              </span>
              <Button
                variant="ghost"
                size="icon-xs"
                title="Next version"
                onClick={() => setActive(versions[versionIndex + 1])}
                disabled={versionIndex >= versions.length - 1}
                className="text-muted-foreground disabled:opacity-30"
              >
                <ChevronRight size={13} />
              </Button>
            </div>
          )}
          {!isPdf && <Button
            variant="ghost"
            size="icon-xs"
            title={editing ? 'Save' : 'Edit'}
            onClick={() => {
              if (editing && editedContent && editedContent !== originalContent) {
                const msg = messages.find(m => m.id === activeArtifact?.messageId)
                if (msg) updateMessage(msg.id, msg.content.replace(originalContent, editedContent))
              }
              if (!editing) setEditedContent(editedContent || originalContent)
              setEditing(e => !e)
            }}
            className={cn('text-muted-foreground', editing && 'text-foreground')}
          >
            {editing ? <Check size={13} /> : <Pencil size={13} />}
          </Button>}
          {isJson && !editing && (
            <div className="flex items-center gap-0.5 border border-border rounded px-0.5">
              <Button
                variant="ghost"
                size="icon-xs"
                title="Tree view"
                onClick={() => setJsonMode('tree')}
                className={cn('text-muted-foreground', jsonMode === 'tree' && 'text-foreground bg-secondary')}
              >
                <List size={13} />
              </Button>
              <Button
                variant="ghost"
                size="icon-xs"
                title="Graph view"
                onClick={() => setJsonMode('graph')}
                className={cn('text-muted-foreground', jsonMode === 'graph' && 'text-foreground bg-secondary')}
              >
                <GitFork size={13} />
              </Button>
              <Button
                variant="ghost"
                size="icon-xs"
                title="Source"
                onClick={() => setJsonMode('source')}
                className={cn('text-muted-foreground', jsonMode === 'source' && 'text-foreground bg-secondary')}
              >
                <Code size={13} />
              </Button>
            </div>
          )}
          {canPreview && !editing && (
            <Button
              variant="ghost"
              size="icon-xs"
              title={preview ? 'Show source' : 'Preview'}
              onClick={() => setPreview(p => !p)}
              className={cn('text-muted-foreground', preview && 'text-foreground')}
            >
              {preview ? <Code size={13} /> : <Eye size={13} />}
            </Button>
          )}
          <Button variant="ghost" size="icon-xs" title="Copy" onClick={handleCopy} className="text-muted-foreground">
            <Copy size={13} />
          </Button>
          {versions.length > 0 && <Button variant="ghost" size="icon-xs" title="Download" onClick={handleDownload} className="text-muted-foreground">
            <Download size={13} />
          </Button>}
          <Button variant="ghost" size="icon-xs" title="Close" onClick={() => setActive(null)} className="text-muted-foreground">
            <X size={13} />
          </Button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto relative">
        {isPdf ? (
          <Document file={content} onLoadSuccess={onDocumentLoad} className="flex flex-col items-center gap-2 p-4">
            {Array.from({ length: numPages }, (_, i) => (
              <Page key={i + 1} pageNumber={i + 1} width={Math.max(width - 32, 200)} renderTextLayer renderAnnotationLayer />
            ))}
          </Document>
        ) : editing ? (
          <div className="relative w-full h-full">
            <pre
              ref={preRef}
              className="hljs absolute inset-0 m-0 p-4 text-sm font-mono whitespace-pre-wrap break-words overflow-hidden pointer-events-none rounded-none bg-background"
              dangerouslySetInnerHTML={{ __html: highlighted + '\n' }}
            />
            <textarea
              ref={textareaRef}
              className="absolute inset-0 w-full h-full p-4 text-sm font-mono bg-transparent text-transparent caret-white resize-none outline-none border-0"
              value={editedContent}
              onChange={e => setEditedContent(e.target.value)}
              onScroll={e => {
                if (preRef.current) preRef.current.scrollTop = e.currentTarget.scrollTop
              }}
              spellCheck={false}
            />
          </div>
        ) : isHtml && preview ? (
          <div className="relative w-full h-full">
            <iframe
              srcDoc={content}
              className="w-full h-full border-0"
              sandbox="allow-scripts"
              title={title}
            />
            {isDragging && <div className="absolute inset-0" />}
          </div>
        ) : isMd && preview ? (
          <div className="p-4 prose prose-invert prose-sm max-w-none">
            <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
              {content}
            </ReactMarkdown>
          </div>
        ) : isMd ? (
          <div className="p-4 text-sm font-mono">
            <pre className="whitespace-pre-wrap break-words text-foreground/80">{content}</pre>
          </div>
        ) : isJson && !editing && jsonMode === 'tree' ? (
          <JsonTreeViewer content={content} />
        ) : isJson && !editing && jsonMode === 'graph' ? (
          <JsonGraphViewer content={content} />
        ) : (
          <div className="p-4 text-sm">
            <div className="prose prose-invert prose-sm max-w-none [&_pre]:!m-0 [&_pre]:!rounded-md [&_pre]:!bg-secondary/50">
              <ReactMarkdown rehypePlugins={[rehypeHighlight]}>
                {`\`\`\`${type}\n${content}\n\`\`\``}
              </ReactMarkdown>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
