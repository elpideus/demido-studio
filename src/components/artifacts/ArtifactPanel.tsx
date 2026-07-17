import { useState, useEffect, useMemo, useRef, useCallback } from 'react'
import { Document, Page, pdfjs } from 'react-pdf'
import 'react-pdf/dist/Page/AnnotationLayer.css'
import 'react-pdf/dist/Page/TextLayer.css'

pdfjs.GlobalWorkerOptions.workerSrc = new URL('pdfjs-dist/build/pdf.worker.min.mjs', import.meta.url).toString()
import hljs from 'highlight.js'
import { X, Copy, Download, Eye, Code, ChevronLeft, ChevronRight, Pencil, Check, List, GitFork, ExternalLink, Columns2 } from 'lucide-react'
import * as fileIconsJs from 'file-icons-js'
import 'file-icons-js/css/style.css'
import { JsonTreeViewer } from './JsonTreeViewer'
import { JsonGraphViewer } from './JsonGraphViewer'
import { MarkdownRenderer } from '../chat/MarkdownRenderer'
import { MermaidBlock } from '../chat/MermaidBlock'
import { Button } from '@/components/ui/button'
import { fs } from '../../lib/tauri'
import { useArtifacts } from '../../stores/artifacts'
import { useWindowManager } from '../../stores/windowManager'
import { useMessages } from '../../stores/messages'
import { getExtension, parseArtifacts } from '../../lib/parseArtifacts'
import { cn } from '../../lib/utils'
import { TOOL_CALLS_CONTENT_PREFIX } from '../../lib/constants'
import type { Artifact } from '../../types'

export function ArtifactPanel({ width, isDragging, windowed }: { width?: number; isDragging?: boolean; windowed?: boolean }) {
  const { activeArtifact, setActive, setPoppedOut, skillSession, selectSkillFile, saveSkillFile } = useArtifacts()
  const { openWindow } = useWindowManager()
  const messages = useMessages(s => s.messages)
  const updateMessage = useMessages(s => s.updateMessage)
  const [preview, setPreview] = useState(true)
  const [jsonMode, setJsonMode] = useState<'tree' | 'graph' | 'source'>('tree')
  const [editing, setEditing] = useState(false)
  const [split, setSplit] = useState(false)
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

  // Markdown/HTML open split in the detached window — the pane reads `editedContent`, so it
  // has to start seeded rather than empty.
  useEffect(() => {
    const type = activeArtifact?.type ?? ''
    const splittable = !!windowed && (type === 'html' || type === 'markdown' || type === 'md')
    setPreview(true)
    setEditing(false)
    setSplit(splittable)
    setEditedContent(splittable ? activeArtifact?.content ?? '' : '')
    setJsonMode('tree')
  }, [activeArtifact?.id, windowed])

  // Auto-switch to newest version when a new one arrives
  useEffect(() => {
    const newest = versions[versions.length - 1]
    if (newest && newest.id !== activeArtifact?.id) {
      setActive(newest)
    }
  }, [versions.length])

  if (!activeArtifact) return null

  const { type, title, content: originalContent } = activeArtifact
  // While a pane is open the textarea is the source of truth, so clearing it clears the preview.
  const content = editing || split ? editedContent : editedContent || originalContent
  const isHtml = type === 'html'
  const isMd = type === 'markdown' || type === 'md'
  const isPdf = type === 'pdf'
  const isJson = type === 'json' || type === 'jsonc' || type === 'json5' || type.startsWith('json')
  const isMermaid = type === 'mermaid'
  const isLatex = type === 'latex' || type === 'tex'
  const canPreview = isHtml || isMd || isMermaid || isLatex

  /** Write edits back where the artifact came from: a skill file on disk, or the message. */
  const persist = (next: string) => {
    if (!next || next === originalContent) return
    if (skillSession) {
      void saveSkillFile(title, next)
      return
    }
    const msg = messages.find(m => m.id === activeArtifact?.messageId)
    if (msg) updateMessage(msg.id, msg.content.replace(originalContent, next))
  }

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
    if (!editing && !split) return ''
    try {
      return hljs.highlight(editedContent, { language: type }).value
    } catch {
      return hljs.highlightAuto(editedContent).value
    }
  }, [editing, split, editedContent, type])

  const hasVersions = versions.length > 1
  const canSplit = windowed && (isHtml || isMd)

  const editorPane = (
    <div className="relative w-full h-full">
      <pre
        ref={preRef}
        className={cn(
          'hljs absolute inset-0 m-0 p-4 text-sm font-mono whitespace-pre-wrap break-words overflow-hidden pointer-events-none rounded-none',
          // index.css forces `.hljs` to --secondary with !important, so this has to shout back.
          split ? '!bg-[#171717]' : 'bg-background',
        )}
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
  )

  const previewPane = isHtml ? (
    <div className="relative w-full h-full">
      <iframe srcDoc={content} className="w-full h-full border-0" sandbox="allow-scripts" title={title} />
      {isDragging && <div className="absolute inset-0" />}
    </div>
  ) : (
    <div className="p-4 prose prose-invert prose-sm max-w-none">
      <MarkdownRenderer>{content}</MarkdownRenderer>
    </div>
  )

  return (
    <div
      className="flex bg-background overflow-hidden relative h-full"
      style={windowed ? { width: '100%', minWidth: 0 } : { flex: `0 0 ${width}px`, minWidth: 0 }}
    >
      {windowed && skillSession && skillSession.files.length > 1 && (
        <div className="w-44 flex flex-col py-2 gap-0.5 border-r border-border/50 bg-[#1b1b1b] shrink-0 overflow-y-auto">
          <p className="px-3 pb-1.5 text-[10px] uppercase tracking-wide text-muted-foreground/70 truncate" title={skillSession.skillName}>
            {skillSession.skillName}
          </p>
          {skillSession.files.map(f => (
            <button
              key={f.name}
              onClick={() => selectSkillFile(f.name)}
              title={f.name}
              className={`mx-1.5 px-2 py-1.5 rounded-md text-left text-xs truncate transition-colors ${
                f.name === skillSession.activeFile
                  ? 'text-foreground bg-accent'
                  : 'text-muted-foreground hover:text-foreground hover:bg-accent/60'
              }`}
            >
              {f.name}
            </button>
          ))}
        </div>
      )}
      {windowed && !skillSession && hasVersions && (
        <div className="w-12 flex flex-col items-center py-2 gap-1.5 border-r border-border/50 bg-[#1b1b1b] shrink-0 overflow-y-auto">
          {versions.map((v, i) => (
            <button
              key={v.id}
              onClick={() => setActive(v)}
              title={`Version ${i + 1}`}
              className={`w-9 h-9 shrink-0 flex items-center justify-center rounded-lg text-xs font-medium transition-colors ${
                v.id === activeArtifact.id
                  ? 'text-foreground bg-accent'
                  : 'text-muted-foreground hover:text-foreground hover:bg-accent/60'
              }`}
            >
              {i + 1}
            </button>
          ))}
        </div>
      )}
      <div className="flex flex-col flex-1 min-w-0 relative">
      {/* Docked only — the detached window shows this in its title bar instead. */}
      {!windowed && (
        <div className="flex items-center gap-2 px-3 py-2.5 border-b border-border shrink-0 pr-12">
          {(() => { const cls = fileIconsJs.getClassWithColor(`artifact${getExtension(type)}`); return cls ? <span className={cls} style={{ fontSize: 13, lineHeight: 1, display: 'inline-block', width: 13 }} /> : null })()}
          <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-secondary text-muted-foreground border border-border">
            {type}
          </span>
          <span className="flex-1 text-sm font-medium truncate">{title}</span>
        </div>
      )}

      {/* Floating action buttons, positioned below window controls */}
      <div className="absolute right-2 top-12 z-20 flex flex-col items-center gap-1 p-1.5 rounded-lg" style={{ background: '#171717', border: '1px solid rgba(255,255,255,0.06)' }}>
        {hasVersions && !windowed && (
          <div className="flex flex-col items-center gap-0.5 pb-1 mb-0.5 border-b border-white/10">
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
            <span className="text-[9px] text-muted-foreground tabular-nums select-none">
              {versionIndex + 1}/{versions.length}
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
        {!isPdf && !split && <Button
          variant="ghost"
          size="icon-xs"
          title={editing ? 'Save' : 'Edit'}
          onClick={() => {
            if (editing) persist(editedContent)
            else setEditedContent(editedContent || originalContent)
            setEditing(e => !e)
          }}
          className={cn('text-muted-foreground', editing && 'text-foreground')}
        >
          {editing ? <Check size={13} /> : <Pencil size={13} />}
        </Button>}
        {canSplit && (
          <>
            <Button
              variant="ghost"
              size="icon-xs"
              title={split ? 'Leave split view' : 'Split view: edit left, preview right'}
              onClick={() => {
                if (split) persist(editedContent)
                else { setEditedContent(editedContent || originalContent); setEditing(false) }
                setSplit(s => !s)
              }}
              className={cn('text-muted-foreground', split && 'text-foreground')}
            >
              <Columns2 size={13} />
            </Button>
            {split && (
              <Button
                variant="ghost"
                size="icon-xs"
                title="Save"
                onClick={() => persist(editedContent)}
                className="text-muted-foreground"
              >
                <Check size={13} />
              </Button>
            )}
          </>
        )}
        {isJson && !editing && (
          <>
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
          </>
        )}
        {canPreview && !editing && !split && (
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
        {!windowed && <Button
          variant="ghost"
          size="icon-xs"
          title="Open in window"
          onClick={() => {
            openWindow('artifact-viewer', 'artifact-viewer', title, {
              initialSize: { width: Math.round(window.innerWidth * 0.92), height: Math.round(window.innerHeight * 0.92) },
            })
            setPoppedOut(true)
          }}
          className="text-muted-foreground"
        >
          <ExternalLink size={13} />
        </Button>}
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

      {/* Content */}
      <div className="flex-1 overflow-auto relative">
        {isPdf ? (
          <Document file={content} onLoadSuccess={onDocumentLoad} className="flex flex-col items-center gap-2 p-4">
            {Array.from({ length: numPages }, (_, i) => (
              <Page key={i + 1} pageNumber={i + 1} width={Math.max((width ?? 600) - 32, 200)} renderTextLayer renderAnnotationLayer />
            ))}
          </Document>
        ) : split ? (
          <div className="absolute inset-0 flex">
            <div className="flex-1 min-w-0 border-r border-border">{editorPane}</div>
            {/* Preview reads `content`, which follows the textarea — so it tracks every keystroke. */}
            <div className="flex-1 min-w-0 overflow-auto">{previewPane}</div>
          </div>
        ) : editing ? (
          editorPane
        ) : isHtml && preview ? (
          previewPane
        ) : isMermaid && preview ? (
          <div className="w-full h-full" style={{ backgroundImage: 'radial-gradient(circle, #ffffff18 1px, transparent 1px)', backgroundSize: '32px 32px' }}>
            <MermaidBlock code={content} className="relative w-full h-full overflow-hidden group select-none" />
          </div>
        ) : isLatex && preview ? (
          <div className="p-4 prose prose-invert prose-sm max-w-none">
            <MarkdownRenderer>{`$$\n${content}\n$$`}</MarkdownRenderer>
          </div>
        ) : isMd && preview ? (
          previewPane
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
              <MarkdownRenderer>{`\`\`\`${type}\n${content}\n\`\`\``}</MarkdownRenderer>
            </div>
          </div>
        )}
      </div>
      </div>
    </div>
  )
}
