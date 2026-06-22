import { useRef, useState, useEffect, KeyboardEvent } from 'react'
import { Send, Square, Paperclip, FileText, Image } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { ToolSelector } from './ToolSelector'
import { ReasoningSelector } from './ReasoningSelector'
import { useConversations } from '../../stores/conversations'
import { useMessages } from '../../stores/messages'
import { useAttachmentCache } from '../../stores/attachmentCache'
import { useMcpTools } from '../../stores/mcpTools'
import { useSkills } from '../../stores/skills'
import { useProviders } from '../../stores/providers'
import { useImageEditor } from '../../stores/imageEditor'
import { useWindowManager } from '../../stores/windowManager'
import { chat, reasoning, fs } from '../../lib/tauri'
import { toolKey } from '../../lib/constants'
import type { FileAttachment, FsEntry } from '../../types'
import { ARTIFACT_INSTRUCTIONS } from '../../lib/parseArtifacts'

function reasoningKey(providerId: string, modelId: string) {
  return `reasoning:${providerId}:${modelId}`
}

// Matches @"quoted path" (spaces allowed) or @word (no spaces)
const MENTION_RE = /@("(?:[^"\\]|\\.)*"|\S+)/g

function findTagAt(text: string, pos: number): { start: number; end: number } | null {
  MENTION_RE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = MENTION_RE.exec(text)) !== null) {
    if (pos > m.index && pos <= m.index + m[0].length) return { start: m.index, end: m.index + m[0].length }
  }
  return null
}

function parseTokens(text: string): Array<{ hl: boolean; v: string }> {
  const out: Array<{ hl: boolean; v: string }> = []
  MENTION_RE.lastIndex = 0
  let last = 0, m: RegExpExecArray | null
  while ((m = MENTION_RE.exec(text)) !== null) {
    if (m.index > last) out.push({ hl: false, v: text.slice(last, m.index) })
    const token = m[1]
    // quoted → always file path; unquoted → heuristic
    const hl = token.startsWith('"') || /[/\\]|\.\w+$/.test(token)
    out.push({ hl, v: m[0] })
    last = m.index + m[0].length
  }
  if (last < text.length) out.push({ hl: false, v: text.slice(last) })
  return out
}

// Detect `@query` segment ending at cursorPos
function detectMention(text: string, cursor: number): { start: number; query: string } | null {
  let i = cursor - 1
  while (i >= 0 && text[i] !== ' ' && text[i] !== '\n') {
    if (text[i] === '@') return { start: i, query: text.slice(i + 1, cursor) }
    i--
  }
  return null
}

interface MentionState {
  start: number
  query: string
  filtered: FsEntry[]
  selected: number
}

export function InputBar() {
  const [value, setValue] = useState('')
  const [reasoningOptions, setReasoningOptions] = useState<string[] | null>(null)
  const [reasoningMode, setReasoningMode] = useState('off')
  const [attachments, setAttachments] = useState<FileAttachment[]>([])
  const [attachError, setAttachError] = useState<string | null>(null)
  const [attachMenuOpen, setAttachMenuOpen] = useState(false)
  const [mention, setMention] = useState<MentionState | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const imageInputRef = useRef<HTMLInputElement>(null)
  const attachMenuRef = useRef<HTMLDivElement>(null)
  const mentionPopupRef = useRef<HTMLDivElement>(null)
  const dropZoneRef = useRef<HTMLDivElement>(null)
  const errorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const allFilesRef = useRef<FsEntry[]>([])
  const lastCursorRef = useRef(0)
  const valueRef = useRef(value)
  valueRef.current = value

  const activeId = useConversations(s => s.activeId)
  const createConversation = useConversations(s => s.create)
  const workingDir = useConversations(s => {
    const conv = s.conversations.find(c => c.id === s.activeId)
    return (conv?.agent_mode !== 'off' ? conv?.working_directory : null) ?? null
  })
  const { streaming, prependSkillBlocks, streamError, setStreamError } = useMessages()
  const openWithImage = useImageEditor(s => s.openWithImage)
  const openWindow = useWindowManager(s => s.openWindow)
  const storeAttachments = useAttachmentCache(s => s.store)
  const storeForConversation = useAttachmentCache(s => s.storeForConversation)
  const lookupConversation = useAttachmentCache(s => s.lookupConversation)
  const enabledTools = useMcpTools(s => s.enabledTools)
  const allTools = useMcpTools(s => s.tools)
  const { enabledContext: enabledSkillsContext, skills } = useSkills()
  const { selectedProviderId, selectedModelId, providers, modelCapabilities } = useProviders()

  // Load file list when working dir changes (for @mention)
  useEffect(() => {
    allFilesRef.current = []
    if (!workingDir || !activeId) return
    fs.walk(activeId).then(entries => { allFilesRef.current = entries }).catch(() => {})
  }, [workingDir, activeId])

  // Scroll selected mention item into view when arrow keys change selection
  useEffect(() => {
    if (!mention || !mentionPopupRef.current) return
    const el = mentionPopupRef.current.children[mention.selected] as HTMLElement | undefined
    el?.scrollIntoView({ block: 'nearest' })
  }, [mention?.selected])

  useEffect(() => {
    let cancelled = false
    if (!selectedProviderId || !selectedModelId) { setReasoningOptions(null); return }
    setReasoningOptions(null)
    reasoning.getModelReasoning(selectedProviderId, selectedModelId).then(info => {
      if (cancelled) return
      if (!info) { setReasoningOptions(null); return }
      setReasoningOptions(info.allowedOptions)
      const persisted = localStorage.getItem(reasoningKey(selectedProviderId, selectedModelId))
      setReasoningMode(persisted && info.allowedOptions.includes(persisted) ? persisted : info.default)
    }).catch(() => { if (!cancelled) setReasoningOptions(null) })
    return () => { cancelled = true }
  }, [selectedProviderId, selectedModelId])

  useEffect(() => () => { if (errorTimerRef.current) clearTimeout(errorTimerRef.current) }, [])

  useEffect(() => {
    const handler = (e: Event) => {
      const text = (e as CustomEvent<string>).detail
      setValue(text)
      setTimeout(() => textareaRef.current?.focus(), 50)
    }
    window.addEventListener('demido:prefill', handler)
    return () => window.removeEventListener('demido:prefill', handler)
  }, [])

  useEffect(() => {
    if (!attachMenuOpen) return
    const handler = (e: MouseEvent) => {
      if (attachMenuRef.current && !attachMenuRef.current.contains(e.target as Node)) setAttachMenuOpen(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [attachMenuOpen])

  const selectedProvider = providers.find(p => p.id === selectedProviderId)
  const caps = modelCapabilities[selectedProviderId]?.[selectedModelId]
  const visionSupported = (() => {
    if (!selectedProvider || !selectedModelId) return false
    if (caps) return caps.vision
    return selectedProvider.type === 'anthropic'
  })()
  const toolsSupported = caps ? caps.tools : true

  const handleReasoningChange = (v: string) => {
    setReasoningMode(v)
    if (selectedProviderId && selectedModelId) localStorage.setItem(reasoningKey(selectedProviderId, selectedModelId), v)
  }

  const showAttachError = (msg: string) => {
    if (errorTimerRef.current) clearTimeout(errorTimerRef.current)
    setAttachError(msg)
    errorTimerRef.current = setTimeout(() => setAttachError(null), 3000)
  }

  const addAttachment = (file: File, isImage: boolean) => {
    const maxSize = isImage ? 5 * 1024 * 1024 : 512 * 1024
    const maxLabel = isImage ? '5 MB' : '512 KB'
    if (file.size > maxSize) { showAttachError(`${file.name} is too large — max ${maxLabel}`); return }
    const reader = new FileReader()
    reader.onload = () => {
      const content = reader.result as string
      if (isImage) {
        // Open in image editor
        openWithImage(content, file.name)
        openWindow('image-editor', 'image-editor', file.name, { initialSize: { width: 1100, height: 720 } })
      }
      setAttachments(prev => {
        if (prev.some(a => a.name === file.name)) { showAttachError(`${file.name} is already attached`); return prev }
        return [...prev, isImage ? { name: file.name, content, mimeType: file.type } : { name: file.name, content }]
      })
    }
    reader.onerror = () => showAttachError(`Could not read ${file.name}.`)
    if (isImage) reader.readAsDataURL(file); else reader.readAsText(file)
  }

  const handleFilePick = (e: React.ChangeEvent<HTMLInputElement>) => {
    Array.from(e.target.files ?? []).forEach(f => addAttachment(f, false)); e.target.value = ''
  }
  const handleImagePick = (e: React.ChangeEvent<HTMLInputElement>) => {
    Array.from(e.target.files ?? []).forEach(f => addAttachment(f, true)); e.target.value = ''
  }

  // @mention: update on every keystroke
  const updateMention = (text: string, cursor: number) => {
    if (!workingDir) { setMention(null); return }
    const m = detectMention(text, cursor)
    if (!m) { setMention(null); return }
    const q = m.query.toLowerCase()
    const filtered = allFilesRef.current
      .filter(f => f.name.toLowerCase().includes(q) || f.path.replace(/\\/g, '/').toLowerCase().includes(q))
      .slice(0, 12)
    setMention({ start: m.start, query: m.query, filtered, selected: 0 })
  }

  const toRelative = (absPath: string) => {
    if (!workingDir) return absPath
    const norm = (p: string) => p.replace(/\\/g, '/').replace(/\/+$/, '')
    const base = norm(workingDir)
    const path = norm(absPath)
    if (path.toLowerCase().startsWith(base.toLowerCase() + '/')) return path.slice(base.length + 1)
    return path
  }

  const insertMention = (entry: FsEntry) => {
    const el = textareaRef.current
    if (!mention || !el) return
    const cursor = lastCursorRef.current || mention.start
    const rel = toRelative(entry.path)
    const tag = `@${rel.includes(' ') ? `"${rel}"` : rel} `
    const newVal = value.slice(0, mention.start) + tag + value.slice(cursor)
    setValue(newVal)
    setMention(null)
    requestAnimationFrame(() => {
      el.focus()
      const pos = mention.start + tag.length
      el.setSelectionRange(pos, pos)
    })
  }

  const handleSend = async () => {
    const content = value.trim()
    if (!content || streaming) return
    let convId = activeId
    if (!convId) {
      if (!selectedProviderId || !selectedModelId) return
      const conv = await createConversation(selectedProviderId, selectedModelId)
      convId = conv.id
    }
    setValue('')
    setMention(null)
    const currentAttachments = attachments
    setAttachments([])
    if (currentAttachments.length > 0) {
      storeAttachments(content, currentAttachments)
      storeForConversation(convId, currentAttachments)
    }
    if (textareaRef.current) textareaRef.current.style.height = 'auto'
    const enabled = enabledTools()
    const enabledKeys = new Set(enabled.map(toolKey))
    const disabledTools = allTools.filter(t => !enabledKeys.has(toolKey(t))).map(toolKey)
    const effort = reasoningOptions ? reasoningMode : undefined
    prependSkillBlocks(skills.filter(s => s.enabled).map(s => s.name))
    try {
      const historicalAtts = currentAttachments.length === 0 ? lookupConversation(convId) : undefined
      await chat.sendMessage(
        convId, content, disabledTools, effort,
        selectedProviderId || undefined, selectedModelId || undefined,
        currentAttachments.length > 0 ? currentAttachments : undefined,
        (() => { const sc = enabledSkillsContext(); return sc ? `${ARTIFACT_INSTRUCTIONS}\n\n${sc}` : ARTIFACT_INSTRUCTIONS })(),
        historicalAtts,
      )
    } catch (e) { setStreamError(String(e)) }
  }

  const handleClick = () => {
    const el = textareaRef.current
    if (!el) return
    const pos = el.selectionStart
    const tag = findTagAt(value, pos)
    if (tag) requestAnimationFrame(() => el.setSelectionRange(tag.end, tag.end))
  }

  const handleKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (mention && mention.filtered.length > 0) {
      if (e.key === 'ArrowDown') { e.preventDefault(); setMention(m => m ? { ...m, selected: Math.min(m.selected + 1, m.filtered.length - 1) } : m); return }
      if (e.key === 'ArrowUp') { e.preventDefault(); setMention(m => m ? { ...m, selected: Math.max(m.selected - 1, 0) } : m); return }
      if (e.key === 'Enter' || e.key === 'Tab') { e.preventDefault(); insertMention(mention.filtered[mention.selected]); return }
      if (e.key === 'Escape') { setMention(null); return }
    }
    if (e.key === 'Backspace') {
      const el = textareaRef.current
      if (el && el.selectionStart === el.selectionEnd) {
        const tag = findTagAt(value, el.selectionStart)
        if (tag) {
          e.preventDefault()
          const newVal = value.slice(0, tag.start) + value.slice(tag.end)
          setValue(newVal)
          requestAnimationFrame(() => el.setSelectionRange(tag.start, tag.start))
          return
        }
      }
    }
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend() }
  }

  // Auto-grow: runs after React commits new value so scrollHeight is accurate
  useEffect(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = Math.min(el.scrollHeight, 200) + 'px'
  }, [value])

  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const newVal = e.target.value
    setValue(newVal)
    updateMention(newVal, e.target.selectionStart ?? newVal.length)
  }

  // Native drag-drop listeners (bypasses React synthetic event quirks in WebView2)
  useEffect(() => {
    const el = dropZoneRef.current
    if (!el) return
    const onDragOver = (e: DragEvent) => {
      e.preventDefault()
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'
    }
    const onDrop = (e: DragEvent) => {
      e.preventDefault()
      const path = e.dataTransfer?.getData('text/plain')
      if (!path) return
      const ta = textareaRef.current
      const pos = lastCursorRef.current
      // path is already relative (computed in FileExplorer onDragStart)
      const tag = `@${path.includes(' ') ? `"${path}"` : path} `
      const cur = valueRef.current
      setValue(cur.slice(0, pos) + tag + cur.slice(pos))
      requestAnimationFrame(() => { ta?.focus(); const p = pos + tag.length; ta?.setSelectionRange(p, p) })
    }
    el.addEventListener('dragover', onDragOver)
    el.addEventListener('drop', onDrop)
    return () => { el.removeEventListener('dragover', onDragOver); el.removeEventListener('drop', onDrop) }
  }, [workingDir])

  return (
    <div className="border-t border-border p-4">
      {/* @mention popup */}
      {mention && mention.filtered.length > 0 && (
        <div ref={mentionPopupRef} className="mb-2 bg-popover border border-border rounded-lg shadow-lg overflow-hidden max-h-48 overflow-y-auto">
          {mention.filtered.map((f, i) => (
            <button
              key={f.path}
              className={`flex items-center gap-2 w-full px-3 py-1.5 text-xs text-left hover:bg-accent transition-colors ${i === mention.selected ? 'bg-accent' : ''}`}
              onMouseDown={e => { e.preventDefault(); insertMention(f) }}
            >
              <span className={`shrink-0 text-[10px] font-mono px-1 rounded ${f.isDir ? 'bg-amber-400/20 text-amber-300' : 'bg-secondary text-muted-foreground'}`}>
                {f.isDir ? 'dir' : f.name.split('.').pop() || 'file'}
              </span>
              <span className="truncate text-foreground/80">{f.name}</span>
              <span className="ml-auto text-muted-foreground/50 truncate text-[10px] max-w-[40%]">{toRelative(f.path)}</span>
            </button>
          ))}
        </div>
      )}

      <div ref={dropZoneRef} className="flex items-end gap-3 bg-secondary border border-border rounded-xl px-4 py-3 focus-within:border-ring/50 transition-colors">
        <input ref={fileInputRef} type="file" multiple accept=".txt,.md,.ts,.tsx,.js,.jsx,.py,.rs,.json,.yaml,.yml,.toml,.csv" className="hidden" onChange={handleFilePick} />
        <input ref={imageInputRef} type="file" multiple accept="image/*" className="hidden" onChange={handleImagePick} />
        {reasoningOptions && <ReasoningSelector options={reasoningOptions} value={reasoningMode} onChange={handleReasoningChange} />}
        {toolsSupported && <ToolSelector />}
        <div className="relative shrink-0" ref={attachMenuRef}>
          {attachMenuOpen && (
            <div className="absolute bottom-full left-0 mb-1.5 flex flex-col gap-0.5 bg-popover border border-border rounded-lg shadow-md p-1 min-w-[110px] z-10">
              <button className="flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs text-foreground hover:bg-accent transition-colors" onClick={() => { setAttachMenuOpen(false); fileInputRef.current?.click() }}>
                <FileText size={12} className="text-muted-foreground" /> File
              </button>
              {visionSupported && (
                <button className="flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs text-foreground hover:bg-accent transition-colors" onClick={() => { setAttachMenuOpen(false); imageInputRef.current?.click() }}>
                  <Image size={12} className="text-muted-foreground" /> Image
                </button>
              )}
            </div>
          )}
          <Button onClick={() => setAttachMenuOpen(v => !v)} disabled={streaming} title="Attach" variant="ghost" size="icon-sm" className="text-muted-foreground">
            <Paperclip size={14} />
          </Button>
        </div>
        <div className="flex flex-col flex-1 gap-1.5">
          {attachments.length > 0 && (
            <div className="flex flex-wrap gap-1.5 items-end">
              {attachments.map((att, i) => (
                att.mimeType ? (
                  <div key={att.name} className="relative group shrink-0">
                    <img src={att.content} alt={att.name} className="h-16 w-16 object-cover rounded-lg border border-border" />
                    <div className="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 rounded-lg transition-opacity flex items-end p-1">
                      <span className="text-white text-[9px] truncate w-full leading-tight">{att.name}</span>
                    </div>
                    <button onClick={() => setAttachments(prev => prev.filter((_, idx) => idx !== i))} className="absolute -top-1 -right-1 w-4 h-4 bg-secondary border border-border rounded-full text-muted-foreground hover:text-foreground flex items-center justify-center text-[10px] leading-none">×</button>
                  </div>
                ) : (
                  <div key={att.name} className="relative group shrink-0 h-16 w-16 rounded-lg border border-border bg-accent flex flex-col items-center justify-center gap-1 px-1">
                    <FileText size={16} className="text-muted-foreground shrink-0" />
                    <span className="text-[8px] text-muted-foreground truncate w-full text-center leading-tight px-0.5">{att.name}</span>
                    <button onClick={() => setAttachments(prev => prev.filter((_, idx) => idx !== i))} className="absolute -top-1 -right-1 w-4 h-4 bg-secondary border border-border rounded-full text-muted-foreground hover:text-foreground flex items-center justify-center text-[10px] leading-none">×</button>
                  </div>
                )
              ))}
            </div>
          )}
          <div className="relative">
            {/* Highlight overlay — same font/line-height as textarea, pointer-events-none */}
            <div
              aria-hidden
              className="absolute inset-0 text-sm leading-relaxed whitespace-pre-wrap break-words pointer-events-none select-none overflow-hidden"
            >
              {parseTokens(value).map((p, i) =>
                p.hl
                  ? <mark key={i} className="bg-primary/20 text-primary not-italic rounded-[3px]">{p.v}</mark>
                  : <span key={i}>{p.v}</span>
              )}
            </div>
            <textarea
              ref={textareaRef}
              value={value}
              onChange={handleChange}
              onKeyDown={handleKey}
              onClick={handleClick}
              onSelect={() => { if (textareaRef.current) lastCursorRef.current = textareaRef.current.selectionStart }}
              onBlur={() => { if (textareaRef.current) lastCursorRef.current = textareaRef.current.selectionStart }}
              placeholder="Message…"
              rows={1}
              disabled={streaming}
              className="relative bg-transparent text-sm placeholder:text-muted-foreground outline-none resize-none leading-relaxed w-full z-10 caret-foreground"
              style={{ color: 'transparent' }}
            />
          </div>
        </div>
        {streaming ? (
          <Button onClick={() => chat.cancelStream()} size="icon-sm" className="shrink-0"><Square size={14} fill="currentColor" /></Button>
        ) : (
          <Button onClick={handleSend} disabled={!value.trim()} size="icon-sm" className="shrink-0"><Send size={14} /></Button>
        )}
      </div>
      <p className="text-[10px] text-muted-foreground mt-1.5 px-1">
        {attachError || streamError
          ? <span className="text-red-400">{attachError ?? streamError}</span>
          : workingDir
          ? 'Enter to send · Shift+Enter new line · @ to mention files'
          : 'Enter to send · Shift+Enter for new line'}
      </p>
    </div>
  )
}
