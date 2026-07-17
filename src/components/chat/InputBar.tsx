import { useRef, useState, useEffect, KeyboardEvent } from 'react'
import { Send, Square, Paperclip, FileText, Image } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { ToolSelector } from './ToolSelector'
import { ReasoningSelector } from './ReasoningSelector'
import { useConversations } from '../../stores/conversations'
import { useMessages } from '../../stores/messages'
import { useAttachmentCache } from '../../stores/attachmentCache'
import { useMcpTools } from '../../stores/mcpTools'
import { useSkills, expandCommand, withSkillLocation, usageOf, type SkillCommandEntry } from '../../stores/skills'
import { useBuiltinTools } from '../../stores/builtinTools'
import { useProviders } from '../../stores/providers'
import { useImageEditor } from '../../stores/imageEditor'
import { useWindowManager } from '../../stores/windowManager'
import { chat, reasoning, fs, google, skills as skillsApi } from '../../lib/tauri'
import { toolKey } from '../../lib/constants'
import { dropImagesIfBlind } from '../../lib/attachments'
import type { FileAttachment, FsEntry, GItem } from '../../types'
import { ARTIFACT_INSTRUCTIONS } from '../../lib/parseArtifacts'

function reasoningKey(providerId: string, modelId: string) {
  return `reasoning:${providerId}:${modelId}`
}

// Matches @"quoted path" (spaces allowed) or @word (no spaces)
const MENTION_RE = /@("(?:[^"\\]|\\.)*"|\S+)/g
// Matches @!"type:id" or @!type:id
const GITEM_RE = /@!("(?:[^"\\]|\\.)*"|\S+)/g

function findTagAt(text: string, pos: number): { start: number; end: number } | null {
  for (const re of [MENTION_RE, GITEM_RE]) {
    re.lastIndex = 0
    let m: RegExpExecArray | null
    while ((m = re.exec(text)) !== null) {
      if (pos > m.index && pos <= m.index + m[0].length) return { start: m.index, end: m.index + m[0].length }
    }
  }
  return null
}

function parseTokens(text: string): Array<{ hl: boolean; gitem: boolean; v: string }> {
  const out: Array<{ hl: boolean; gitem: boolean; v: string }> = []

  const matches: Array<{ index: number; raw: string; isGitem: boolean; token: string }> = []

  MENTION_RE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = MENTION_RE.exec(text)) !== null) {
    matches.push({ index: m.index, raw: m[0], isGitem: false, token: m[1] })
  }
  GITEM_RE.lastIndex = 0
  while ((m = GITEM_RE.exec(text)) !== null) {
    if (!matches.some(x => x.index === m!.index)) {
      matches.push({ index: m.index, raw: m[0], isGitem: true, token: m[1] })
    }
  }
  matches.sort((a, b) => a.index - b.index)

  let last = 0
  for (const match of matches) {
    if (match.index > last) out.push({ hl: false, gitem: false, v: text.slice(last, match.index) })
    if (match.isGitem) {
      out.push({ hl: true, gitem: true, v: match.raw })
    } else {
      const hl = match.token.startsWith('"') || /[/\\]|\.\w+$/.test(match.token)
      out.push({ hl, gitem: false, v: match.raw })
    }
    last = match.index + match.raw.length
  }
  if (last < text.length) out.push({ hl: false, gitem: false, v: text.slice(last) })
  return out
}

// Detect `@query` segment ending at cursorPos (plain file mention)
function detectMention(text: string, cursor: number): { start: number; query: string; isGitem: boolean } | null {
  let i = cursor - 1
  while (i >= 0 && text[i] !== ' ' && text[i] !== '\n') {
    if (text[i] === '@') {
      // check if preceded by '!' → gitem
      if (i > 0 && text[i - 1] === '!') {
        // This would be a mid-word @!, unusual - skip
      }
      return { start: i, query: text.slice(i + 1, cursor), isGitem: false }
    }
    if (text[i] === '!' && i > 0 && text[i - 1] === '@') {
      return { start: i - 1, query: text.slice(i + 1, cursor), isGitem: true }
    }
    i--
  }
  return null
}

// Detect a `/command` segment. Only at the very start of the input, so a path like `src/lib`
// or a date never opens the popup.
function detectSlash(text: string, cursor: number): { query: string } | null {
  if (!text.startsWith('/')) return null
  const head = text.slice(1)
  // Once the command name is complete (a space typed), the popup's job is done.
  if (/\s/.test(head)) return null
  if (cursor > head.length + 1) return null
  return { query: head }
}

interface MentionState {
  start: number
  query: string
  filtered: FsEntry[]
  selected: number
}

interface SlashState {
  query: string
  filtered: SkillCommandEntry[]
  selected: number
}

interface GItemMentionState {
  start: number
  query: string
  items: GItem[]
  selected: number
  loading: boolean
}

export function InputBar() {
  const [value, setValue] = useState('')
  const [reasoningOptions, setReasoningOptions] = useState<string[] | null>(null)
  const [reasoningMode, setReasoningMode] = useState('off')
  const [attachments, setAttachments] = useState<FileAttachment[]>([])
  const [attachError, setAttachError] = useState<string | null>(null)
  const [attachMenuOpen, setAttachMenuOpen] = useState(false)
  const [mention, setMention] = useState<MentionState | null>(null)
  const [gitemMention, setGitemMention] = useState<GItemMentionState | null>(null)
  const [slash, setSlash] = useState<SlashState | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const imageInputRef = useRef<HTMLInputElement>(null)
  const attachMenuRef = useRef<HTMLDivElement>(null)
  const mentionPopupRef = useRef<HTMLDivElement>(null)
  const gitemPopupRef = useRef<HTMLDivElement>(null)
  const slashPopupRef = useRef<HTMLDivElement>(null)
  const dropZoneRef = useRef<HTMLDivElement>(null)
  const errorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const allFilesRef = useRef<FsEntry[]>([])
  const lastCursorRef = useRef(0)
  const valueRef = useRef(value)
  valueRef.current = value
  // Maps gitem key (e.g. "email:MSG_ID") → { title, content? }
  const gitemDataRef = useRef<Map<string, { title: string; content?: string }>>(new Map())
  const gitemDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)

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
  const { enabledContext: enabledSkillsContext, skills, enabledCommands } = useSkills()
  const disabledBuiltinKeys = useBuiltinTools(s => s.disabledKeys)
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
    if (!gitemMention || !gitemPopupRef.current) return
    const el = gitemPopupRef.current.children[gitemMention.selected] as HTMLElement | undefined
    el?.scrollIntoView({ block: 'nearest' })
  }, [gitemMention?.selected])

  useEffect(() => {
    if (!slash || !slashPopupRef.current) return
    const el = slashPopupRef.current.children[slash.selected] as HTMLElement | undefined
    el?.scrollIntoView({ block: 'nearest' })
  }, [slash?.selected])

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

  const dropBlindImages = (atts?: FileAttachment[]) => dropImagesIfBlind(atts, visionSupported)

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
    if (!m || m.isGitem) { setMention(null); return }
    const q = m.query.toLowerCase()
    const filtered = allFilesRef.current
      .filter(f => f.name.toLowerCase().includes(q) || f.path.replace(/\\/g, '/').toLowerCase().includes(q))
      .slice(0, 12)
    setMention({ start: m.start, query: m.query, filtered, selected: 0 })
  }

  // @! gitem mention: debounced search
  const updateGItemMention = (text: string, cursor: number) => {
    const m = detectMention(text, cursor)
    if (!m || !m.isGitem) {
      if (gitemDebounceRef.current) { clearTimeout(gitemDebounceRef.current); gitemDebounceRef.current = null }
      setGitemMention(null)
      return
    }
    const query = m.query
    setGitemMention(prev => prev ? { ...prev, query, loading: true } : { start: m.start, query, items: [], selected: 0, loading: true })

    if (gitemDebounceRef.current) clearTimeout(gitemDebounceRef.current)
    gitemDebounceRef.current = setTimeout(async () => {
      const items: GItem[] = []
      await Promise.allSettled([
        google.fetchEmails(query, 8).then(page => {
          for (const e of page.emails) {
            items.push({ type: 'email', id: e.id, title: e.subject || '(no subject)', subtitle: e.from })
          }
        }),
        google.fetchCalendarEvents(30, 7, 20).then(events => {
          const q = query.toLowerCase()
          for (const ev of events) {
            if (!q || ev.summary.toLowerCase().includes(q)) {
              const content = `Title: ${ev.summary}\nStart: ${ev.start}\nEnd: ${ev.end}${ev.location ? `\nLocation: ${ev.location}` : ''}${ev.description ? `\nDescription: ${ev.description}` : ''}`
              items.push({ type: 'event', id: ev.id, title: ev.summary, subtitle: `${ev.start} → ${ev.end}`, content })
            }
          }
        }),
        google.fetchContacts(query, 10).then(page => {
          for (const c of page.contacts) {
            items.push({ type: 'contact', id: c.id.replace(/^people\//, ''), title: c.display_name, subtitle: c.emails[0]?.value })
          }
        }),
      ])
      setGitemMention(prev => prev ? { ...prev, items: items.slice(0, 20), loading: false, selected: 0 } : null)
    }, 300)
  }

  // `/command`: filter on every keystroke. No debounce — the list is already in memory.
  const updateSlash = (text: string, cursor: number) => {
    const s = detectSlash(text, cursor)
    if (!s) { setSlash(null); return }
    const q = s.query.toLowerCase()
    const filtered = enabledCommands()
      .filter(c => c.invocation.toLowerCase().includes(q) || c.description.toLowerCase().includes(q))
      .slice(0, 12)
    setSlash({ query: s.query, filtered, selected: 0 })
  }

  const insertSlash = (cmd: SkillCommandEntry) => {
    const el = textareaRef.current
    if (!el) return
    const next = `/${cmd.invocation} `
    setValue(next)
    setSlash(null)
    requestAnimationFrame(() => {
      el.focus()
      el.setSelectionRange(next.length, next.length)
    })
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

  const insertGItem = (item: GItem) => {
    const el = textareaRef.current
    if (!gitemMention || !el) return
    const cursor = lastCursorRef.current || gitemMention.start
    const key = `${item.type}:${item.id}`
    const needsQuote = key.includes(' ')
    const tag = `@!${needsQuote ? `"${key}"` : key} `

    // Cache title and content for overlay + send-time resolution
    gitemDataRef.current.set(key, { title: item.title, content: item.content })
    const newVal = value.slice(0, gitemMention.start) + tag + value.slice(cursor)
    setValue(newVal)
    setGitemMention(null)
    requestAnimationFrame(() => {
      el.focus()
      const pos = gitemMention.start + tag.length
      el.setSelectionRange(pos, pos)
    })
  }

  // Replace @! tags with tool-readable references the LLM can act on
  const resolveGItems = (text: string): string => {
    GITEM_RE.lastIndex = 0
    return text.replace(GITEM_RE, (_, token) => {
      const key = token.startsWith('"') ? token.slice(1, -1) : token
      const [type, ...rest] = key.split(':')
      const id = rest.join(':')
      const cached = gitemDataRef.current.get(key)
      const title = cached?.title ?? id
      if (type === 'email') return `[email: "${title}" — id: ${id}, use read_email tool]`
      if (type === 'event') return `[calendar event: "${title}" — use list_calendar_events tool to find it]`
      if (type === 'contact') return `[contact: "${title}" — id: ${id}, use read_contact tool]`
      return token
    })
  }

  /**
   * Expand a leading `/command` into its prompt body. Returns null if the text isn't a command,
   * and throws if it names one that can't be resolved — a typo'd command must not be sent to the
   * model as literal text.
   */
  const resolveSlashCommand = async (raw: string): Promise<string | null> => {
    if (!raw.startsWith('/')) return null
    const head = raw.slice(1)
    const sep = head.search(/\s/)
    const name = sep === -1 ? head : head.slice(0, sep)
    const args = sep === -1 ? '' : head.slice(sep + 1).trim()
    if (!name) return null
    const cmd = enabledCommands().find(c => c.invocation === name)
    if (!cmd) throw new Error(`Unknown command /${name}. Its skill may be disabled.`)
    const body = cmd.file
      ? await skillsApi.readCommand(cmd.skillId, cmd.file)
      : (cmd.prompt ?? '')
    if (!body.trim()) throw new Error(`/${name} has no prompt body — its skill.json defines neither a valid 'file' nor a 'prompt'.`)
    let expanded: string
    try {
      expanded = expandCommand(body, args, cmd.params)
    } catch (e) {
      // Surface the call shape: the user needs to know what to type, not just what's missing.
      throw new Error(`${e instanceof Error ? e.message : String(e)}\nUsage: ${usageOf(cmd)}`)
    }
    return withSkillLocation(expanded, cmd.skillName, cmd.skillPath)
  }

  const handleSend = async () => {
    const raw = value.trim()
    if (!raw || streaming) return

    let expanded: string | null = null
    try {
      expanded = await resolveSlashCommand(raw)
    } catch (e) {
      showAttachError(e instanceof Error ? e.message : String(e))
      return
    }

    let convId = activeId
    if (!convId) {
      if (!selectedProviderId || !selectedModelId) return
      const conv = await createConversation(selectedProviderId, selectedModelId)
      convId = conv.id
    }
    setValue('')
    setMention(null)
    setGitemMention(null)
    setSlash(null)
    const currentAttachments = attachments
    setAttachments([])
    // Keyed on the *sent* text, not the typed text: the cache is looked up by message content, and
    // a `/command` or `@!` tag means those two differ.
    const content = resolveGItems(expanded ?? raw)
    if (currentAttachments.length > 0) {
      storeAttachments(content, currentAttachments)
      storeForConversation(convId, currentAttachments)
    }
    if (textareaRef.current) textareaRef.current.style.height = 'auto'

    const enabled = enabledTools()
    const enabledKeys = new Set(enabled.map(toolKey))
    const disabledTools = [
      ...allTools.filter(t => !enabledKeys.has(toolKey(t))).map(toolKey),
      ...disabledBuiltinKeys(),
    ]
    const effort = reasoningOptions ? reasoningMode : undefined
    prependSkillBlocks(skills.filter(s => s.enabled).map(s => s.name))
    try {
      const historicalAtts = currentAttachments.length === 0 ? lookupConversation(convId) : undefined
      // Caps gate what is *sent*, not just what the attach button renders: attachments
      // survive a model switch, and convCache replays them on every later send in the
      // conversation. A text-only model 500s on image content, so drop images here.
      const sentAtts = dropBlindImages(currentAttachments)
      const sentHistorical = dropBlindImages(historicalAtts)
      const droppedImages =
        (currentAttachments.length - sentAtts.length) +
        ((historicalAtts?.length ?? 0) - sentHistorical.length)
      if (droppedImages > 0) {
        showAttachError(
          `${selectedModelId || 'This model'} has no vision support — sent without ${droppedImages === 1 ? 'the image' : `${droppedImages} images`}.`,
        )
      }
      await chat.sendMessage(
        convId, content, disabledTools, effort,
        selectedProviderId || undefined, selectedModelId || undefined,
        sentAtts.length > 0 ? sentAtts : undefined,
        (() => { const sc = enabledSkillsContext(); return sc ? `${ARTIFACT_INSTRUCTIONS}\n\n${sc}` : ARTIFACT_INSTRUCTIONS })(),
        sentHistorical.length ? sentHistorical : undefined,
        skills.filter(s => s.enabled).map(s => s.id),
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
    if (slash && slash.filtered.length > 0) {
      if (e.key === 'ArrowDown') { e.preventDefault(); setSlash(s => s ? { ...s, selected: Math.min(s.selected + 1, s.filtered.length - 1) } : s); return }
      if (e.key === 'ArrowUp') { e.preventDefault(); setSlash(s => s ? { ...s, selected: Math.max(s.selected - 1, 0) } : s); return }
      if (e.key === 'Enter' || e.key === 'Tab') { e.preventDefault(); insertSlash(slash.filtered[slash.selected]); return }
      if (e.key === 'Escape') { setSlash(null); return }
    }
    if (gitemMention && gitemMention.items.length > 0) {
      if (e.key === 'ArrowDown') { e.preventDefault(); setGitemMention(m => m ? { ...m, selected: Math.min(m.selected + 1, m.items.length - 1) } : m); return }
      if (e.key === 'ArrowUp') { e.preventDefault(); setGitemMention(m => m ? { ...m, selected: Math.max(m.selected - 1, 0) } : m); return }
      if (e.key === 'Enter' || e.key === 'Tab') { e.preventDefault(); insertGItem(gitemMention.items[gitemMention.selected]); return }
      if (e.key === 'Escape') { setGitemMention(null); return }
    }
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
    const cursor = e.target.selectionStart ?? newVal.length
    updateMention(newVal, cursor)
    updateGItemMention(newVal, cursor)
    updateSlash(newVal, cursor)
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

  const gitemTypeIcon = (type: GItem['type']) => type === 'email' ? '✉' : type === 'event' ? '📅' : '👤'
  const gitemTypeBadge = (type: GItem['type']) => type === 'email' ? 'email' : type === 'event' ? 'event' : 'contact'

  return (
    <div className="border-t border-border p-4">
      {/* /command popup — skill-provided slash commands */}
      {slash && slash.filtered.length > 0 && (
        <div ref={slashPopupRef} className="mb-2 bg-popover border border-border rounded-lg shadow-lg overflow-hidden max-h-56 overflow-y-auto">
          {slash.filtered.map((c, i) => (
            <button
              key={`${c.skillId}:${c.name}`}
              onClick={() => insertSlash(c)}
              className={`flex items-center gap-2 w-full px-3 py-1.5 text-xs text-left hover:bg-accent transition-colors ${i === slash.selected ? 'bg-accent' : ''}`}
            >
              <span className="font-mono text-primary shrink-0">{usageOf(c)}</span>
              <span className="text-muted-foreground truncate">{c.description}</span>
              <span className="ml-auto text-[10px] text-muted-foreground/70 shrink-0">{c.skillName}</span>
            </button>
          ))}
        </div>
      )}

      {/* @! gitem popup */}
      {gitemMention && (gitemMention.items.length > 0 || gitemMention.loading) && (
        <div ref={gitemPopupRef} className="mb-2 bg-popover border border-border rounded-lg shadow-lg overflow-hidden max-h-56 overflow-y-auto">
          {gitemMention.loading && gitemMention.items.length === 0 && (
            <div className="px-3 py-2 text-xs text-muted-foreground">Searching…</div>
          )}
          {gitemMention.items.map((item, i) => (
            <button
              key={`${item.type}:${item.id}`}
              className={`flex items-center gap-2 w-full px-3 py-1.5 text-xs text-left hover:bg-accent transition-colors ${i === gitemMention.selected ? 'bg-accent' : ''}`}
              onMouseDown={e => { e.preventDefault(); insertGItem(item) }}
            >
              <span className="shrink-0 text-[10px]">{gitemTypeIcon(item.type)}</span>
              <span className={`shrink-0 text-[10px] font-mono px-1 rounded bg-blue-400/20 text-blue-300`}>
                {gitemTypeBadge(item.type)}
              </span>
              <span className="truncate text-foreground/80">{item.title}</span>
              {item.subtitle && <span className="ml-auto text-muted-foreground/50 truncate text-[10px] max-w-[40%]">{item.subtitle}</span>}
            </button>
          ))}
        </div>
      )}

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
                    <img
                      src={att.content}
                      alt={att.name}
                      className="h-16 w-16 object-cover rounded-lg border border-border cursor-pointer"
                      onClick={() => {
                        openWithImage(att.content, att.name)
                        openWindow('image-editor', 'image-editor', att.name, { initialSize: { width: 1100, height: 720 } })
                      }}
                    />
                    <div className="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 rounded-lg transition-opacity flex items-end p-1 pointer-events-none">
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
                  ? <mark key={i} className={`not-italic rounded-[3px] ${p.gitem ? 'bg-blue-500/20 text-blue-300' : 'bg-primary/20 text-primary'}`}>{p.v}</mark>
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
          ? 'Enter to send · Shift+Enter new line · / for skill commands · @ to mention files · @! to mention emails, events, contacts'
          : 'Enter to send · Shift+Enter for new line · / for skill commands · @! to mention emails, events, contacts'}
      </p>
    </div>
  )
}

