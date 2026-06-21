import { useState, useRef, useEffect, useMemo } from 'react'
import { MessageSquare, ChevronRight, Copy, Pencil, RotateCcw, ArrowRight, FileText, Trash2, RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import { cn, clampToViewport } from '../../lib/utils'
import type { FileAttachment } from '../../types'
import { parseArtifacts } from '../../lib/parseArtifacts'
import { ArtifactCard } from './ArtifactCard'
import { useArtifacts } from '../../stores/artifacts'

interface Props {
  id: string
  role: 'user' | 'assistant'
  content: string
  thinking?: string
  hasStrip?: boolean
  streaming?: boolean
  modelLabel?: string
  attachments?: FileAttachment[]
  onEdit?: (id: string, newContent: string) => void
  onRegenerate?: (id: string) => void
  onContinue?: (id: string) => void
  onDelete?: (id: string) => void
  onResend?: (id: string) => void
  messageId?: string
  versionOf?: Map<string, number>
}

function ActionButton({ icon, title, onClick }: { icon: React.ReactNode; title: string; onClick: () => void }) {
  return (
    <Button title={title} onClick={onClick} variant="outline" size="icon-xs" className="text-muted-foreground">
      {icon}
    </Button>
  )
}

export function MessageBubble({ id, role, content, thinking, hasStrip, streaming, modelLabel, attachments, onEdit, onRegenerate, onContinue, onDelete, onResend, messageId, versionOf }: Props) {
  const isUser = role === 'user'
  const setActiveArtifact = useArtifacts(s => s.setActive)
  const [thinkingExpanded, setThinkingExpanded] = useState(false)
  const [hovered, setHovered] = useState(false)
  const [editing, setEditing] = useState(false)
  const [editValue, setEditValue] = useState(content)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number } | null>(null)
  const ctxMenuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!ctxMenu) return
    requestAnimationFrame(() => ctxMenuRef.current && clampToViewport(ctxMenuRef.current))
    const handler = (e: MouseEvent) => {
      if (ctxMenuRef.current && !ctxMenuRef.current.contains(e.target as Node)) {
        setCtxMenu(null)
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [ctxMenu])

  const handleCtxCopy = () => { navigator.clipboard.writeText(content); setCtxMenu(null) }
  const handleCtxEdit = () => { setCtxMenu(null); handleEditStart() }
  const handleCtxDelete = () => { setCtxMenu(null); onDelete?.(id) }
  const handleCtxRegenerate = () => { setCtxMenu(null); onRegenerate?.(id) }
  const handleCtxContinue = () => { setCtxMenu(null); onContinue?.(id) }
  const handleCtxResend = () => { setCtxMenu(null); onResend?.(id) }

  // Parse artifacts from assistant messages (not while streaming)
  const segments = useMemo(() => {
    if (isUser || streaming || !messageId) return null
    return parseArtifacts(content, messageId)
  }, [isUser, streaming, messageId, content])

  useEffect(() => {
    if (editing && textareaRef.current) {
      textareaRef.current.focus()
      textareaRef.current.style.height = 'auto'
      textareaRef.current.style.height = textareaRef.current.scrollHeight + 'px'
    }
  }, [editing])

  const handleCopy = () => navigator.clipboard.writeText(content)

  const handleEditStart = () => {
    setEditValue(content)
    setEditing(true)
  }

  const handleEditCancel = () => setEditing(false)

  const handleEditSave = () => {
    const trimmed = editValue.trim()
    if (!trimmed) return
    setEditing(false)
    onEdit?.(id, trimmed)
  }


  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    if (streaming) return
    setCtxMenu({ x: e.clientX, y: e.clientY })
  }

  return (
    <div
      className={cn('flex flex-col', isUser ? 'items-end' : hasStrip ? 'w-full' : 'items-start')}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onContextMenu={handleContextMenu}
    >
      {!isUser && thinking && (
        <div
          className="max-w-[75%] mb-1.5 border border-dashed border-border rounded-[10px_10px_10px_3px] bg-card cursor-pointer"
          onClick={() => setThinkingExpanded(e => !e)}
        >
          <div className="flex items-center gap-2 px-3 py-2">
            <MessageSquare size={12} className="text-muted-foreground/60 shrink-0" />
            <span className="text-muted-foreground/60 text-xs flex-1 truncate">
              {thinkingExpanded ? 'Thinking' : thinking.split('\n')[0]}
            </span>
            <ChevronRight
              size={11}
              className={cn('text-muted-foreground/50 shrink-0 transition-transform', thinkingExpanded && 'rotate-90')}
            />
          </div>
          {thinkingExpanded && (
            <div className="px-3 pb-3 border-t border-border pt-2">
              <div
                className="prose prose-invert prose-xs max-w-none"
                style={{
                  '--tw-prose-body': '#6b6b88',
                  '--tw-prose-bold': '#6b6b88',
                  '--tw-prose-headings': '#6b6b88',
                  '--tw-prose-code': '#6b6b88',
                  '--tw-prose-links': '#6b6b88',
                  '--tw-prose-quotes': '#6b6b88',
                  '--tw-prose-kbd': '#6b6b88',
                  '--tw-prose-pre-code': '#6b6b88',
                  '--tw-prose-pre-bg': '#1a1a24',
                } as React.CSSProperties}
              >
                <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
                  {thinking}
                </ReactMarkdown>
              </div>
            </div>
          )}
        </div>
      )}

      {isUser && attachments && attachments.length > 0 && (
        <div className="flex flex-wrap gap-1.5 mb-1.5 justify-end max-w-[75%]">
          {attachments.map(att => att.mimeType?.startsWith('image/') ? (
            <img
              key={att.name}
              src={att.content}
              alt={att.name}
              className="h-24 max-w-[180px] object-cover rounded-lg border border-border"
            />
          ) : att.mimeType === 'application/pdf' ? (
            <button
              key={att.name}
              onClick={() => setActiveArtifact({ id: att.name, messageId: id, type: 'pdf', title: att.name, content: att.content })}
              className="flex items-center gap-1 px-2 py-1 rounded-md bg-secondary border border-border text-[11px] text-muted-foreground hover:bg-secondary/80 transition-colors"
            >
              <FileText size={10} className="shrink-0 text-red-400" />
              {att.name}
            </button>
          ) : (
            <div key={att.name} className="flex items-center gap-1 px-2 py-1 rounded-md bg-secondary border border-border text-[11px] text-muted-foreground">
              <FileText size={10} className="shrink-0" />
              {att.name}
            </div>
          ))}
        </div>
      )}

      {editing ? (
        <div className="max-w-[75%] w-full flex flex-col gap-2">
          <textarea
            ref={textareaRef}
            value={editValue}
            onChange={e => {
              setEditValue(e.target.value)
              e.target.style.height = 'auto'
              e.target.style.height = e.target.scrollHeight + 'px'
            }}
            rows={1}
            className="w-full bg-secondary border border-[var(--primary)]/50 rounded-xl px-4 py-3 text-sm text-foreground outline-none resize-none leading-relaxed"
          />
          <div className="flex gap-2 justify-end">
            <Button onClick={handleEditCancel} variant="secondary" size="xs">Cancel</Button>
            <Button onClick={handleEditSave} disabled={!editValue.trim()} size="xs">{isUser ? 'Save & Send' : 'Save'}</Button>
          </div>
        </div>
      ) : (
        <div
          className={cn(
            'rounded-xl px-4 py-3 text-sm leading-relaxed',
            hasStrip ? 'w-full' : 'max-w-[75%]',
            isUser
              ? 'bg-primary text-white rounded-br-sm'
              : 'bg-secondary text-foreground rounded-bl-sm border border-border',
            !isUser && (thinking || hasStrip) && 'rounded-tl-sm'
          )}
        >
          {!isUser && modelLabel && (
            <p className="text-[10px] text-muted-foreground/60 mb-1.5">{modelLabel}</p>
          )}
          {isUser ? (
            <p className="whitespace-pre-wrap">{content}</p>
          ) : segments ? (
            <div className="prose prose-invert prose-sm max-w-none">
              {segments.map((seg, i) =>
                seg.artifact ? (
                  <ArtifactCard key={seg.artifact.id} artifact={seg.artifact} version={versionOf?.get(seg.artifact.id)} />
                ) : seg.text ? (
                  <ReactMarkdown key={i} remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
                    {seg.text}
                  </ReactMarkdown>
                ) : null
              )}
            </div>
          ) : (
            <div className="prose prose-invert prose-sm max-w-none">
              <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
                {content}
              </ReactMarkdown>
              {streaming && <span className="inline-block w-2 h-4 bg-primary animate-pulse ml-0.5 align-middle" />}
            </div>
          )}
        </div>
      )}

      {/* Action buttons — shown on hover, hidden while editing or streaming */}
      {!editing && (
        <div className={cn(
          'flex gap-1 mt-1 transition-opacity duration-150',
          hovered && !streaming ? 'opacity-100' : 'opacity-0 pointer-events-none'
        )}>
          {isUser ? (
            <>
              <ActionButton icon={<RefreshCw size={11} />} title="Re-send" onClick={() => onResend?.(id)} />
              <ActionButton icon={<Pencil size={11} />} title="Edit" onClick={handleEditStart} />
              <ActionButton icon={<Copy size={11} />} title="Copy" onClick={handleCopy} />
              <ActionButton icon={<Trash2 size={11} />} title="Delete" onClick={() => onDelete?.(id)} />
            </>
          ) : (
            <>
              <ActionButton icon={<Copy size={11} />} title="Copy" onClick={handleCopy} />
              <ActionButton icon={<RotateCcw size={11} />} title="Regenerate" onClick={() => onRegenerate?.(id)} />
              <ActionButton icon={<ArrowRight size={11} />} title="Continue" onClick={() => onContinue?.(id)} />
              <ActionButton icon={<Pencil size={11} />} title="Edit" onClick={handleEditStart} />
              <ActionButton icon={<Trash2 size={11} />} title="Delete" onClick={() => onDelete?.(id)} />
            </>
          )}
        </div>
      )}

      {ctxMenu && (
        <div
          ref={ctxMenuRef}
          className="fixed z-50 min-w-[140px] bg-popover border border-border rounded-md shadow-md py-1 text-[12px]"
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          onMouseDown={e => e.stopPropagation()}
        >
          {isUser ? (
            <>
              <button className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80 flex items-center gap-2" onClick={handleCtxResend}>
                <RefreshCw size={11} /> Re-send
              </button>
              <button className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80 flex items-center gap-2" onClick={handleCtxEdit}>
                <Pencil size={11} /> Edit
              </button>
              <button className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80 flex items-center gap-2" onClick={handleCtxCopy}>
                <Copy size={11} /> Copy
              </button>
              <div className="border-t border-border/40 my-1" />
              <button className="w-full text-left px-3 py-1.5 hover:bg-red-500/20 text-red-400 flex items-center gap-2" onClick={handleCtxDelete}>
                <Trash2 size={11} /> Delete
              </button>
            </>
          ) : (
            <>
              <button className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80 flex items-center gap-2" onClick={handleCtxCopy}>
                <Copy size={11} /> Copy
              </button>
              <button className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80 flex items-center gap-2" onClick={handleCtxRegenerate}>
                <RotateCcw size={11} /> Re-Generate
              </button>
              <button className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80 flex items-center gap-2" onClick={handleCtxContinue}>
                <ArrowRight size={11} /> Continue
              </button>
              <button className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80 flex items-center gap-2" onClick={handleCtxEdit}>
                <Pencil size={11} /> Edit
              </button>
              <div className="border-t border-border/40 my-1" />
              <button className="w-full text-left px-3 py-1.5 hover:bg-red-500/20 text-red-400 flex items-center gap-2" onClick={handleCtxDelete}>
                <Trash2 size={11} /> Delete
              </button>
            </>
          )}
        </div>
      )}
    </div>
  )
}
