import { useEffect, useRef, useMemo, useCallback } from 'react'
import { FileCode, Loader2 } from 'lucide-react'
import { useMessages } from '../../stores/messages'
import type { StreamBlock } from '../../stores/messages'
import { useConversations } from '../../stores/conversations'
import { useProviders } from '../../stores/providers'
import { useMcpTools } from '../../stores/mcpTools'
import { useSkills } from '../../stores/skills'
import { chat } from '../../lib/tauri'
import { TOOL_CALLS_CONTENT_PREFIX, toolKey } from '../../lib/constants'
import { MessageBubble } from './MessageBubble'
import { TimelineStrip } from './TimelineStrip'
import { MarkdownRenderer } from './MarkdownRenderer'
import { cn } from '../../lib/utils'
import { useAttachmentCache } from '../../stores/attachmentCache'
import { ARTIFACT_INSTRUCTIONS, parseArtifacts, parseStreamingSegments } from '../../lib/parseArtifacts'
import type { StreamingArtifactHint } from '../../lib/parseArtifacts'
import { useArtifacts } from '../../stores/artifacts'

function StreamingArtifactCard({ hint }: { hint: StreamingArtifactHint }) {
  return (
    <div className="not-prose flex items-center gap-2 mt-2 px-3 py-2 rounded-lg border bg-secondary border-border text-foreground w-full max-w-xs cursor-not-allowed select-none opacity-80">
      {hint.complete
        ? <FileCode size={14} className="shrink-0 text-muted-foreground" />
        : <Loader2 size={14} className="shrink-0 text-primary animate-spin" />
      }
      <div className="flex-1 min-w-0">
        <p className="text-xs font-medium truncate">{hint.title}</p>
        <p className="text-[10px] text-muted-foreground">{hint.complete ? hint.type : 'Generating…'}</p>
      </div>
    </div>
  )
}

function StreamingBlocks({ blocks, streamBuffer, modelLabel, resolvedPermissions, pendingPermission }: { blocks: StreamBlock[]; streamBuffer: string; modelLabel?: string; resolvedPermissions: import('../../stores/messages').ResolvedPermission[]; pendingPermission: import('../../stores/messages').PermissionRequest | null }) {
  const streamSegs = useMemo(() => parseStreamingSegments(streamBuffer), [streamBuffer])
  const lastSegIsArtifact = streamSegs.length > 0 && !!streamSegs[streamSegs.length - 1].artifactHint

  return (
    <div className="flex flex-col max-w-[75%] w-full">
      <TimelineStrip blocks={blocks} resolvedPermissions={resolvedPermissions} pendingPermission={pendingPermission} />
      {streamBuffer ? (
        <div className={cn(
          'w-full rounded-xl px-4 py-3 text-sm leading-relaxed bg-secondary text-foreground border border-border',
          blocks.length > 0 ? 'rounded-tl-[3px]' : 'rounded-bl-sm'
        )}>
          {modelLabel && <p className="text-[10px] text-muted-foreground/60 mb-1.5">{modelLabel}</p>}
          <div className="prose prose-invert prose-sm max-w-none">
            {streamSegs.map((seg, i) =>
              seg.artifactHint
                ? <StreamingArtifactCard key={i} hint={seg.artifactHint} />
                : seg.text
                  ? <MarkdownRenderer key={i}>{seg.text}</MarkdownRenderer>
                  : null
            )}
            {!lastSegIsArtifact && <span className="inline-block w-2 h-4 bg-primary animate-pulse ml-0.5 align-middle" />}
          </div>
        </div>
      ) : (
        <div className={cn(
          'w-full rounded-xl px-4 py-3 text-sm bg-secondary border border-border',
          blocks.length > 0 ? 'rounded-tl-[3px]' : 'rounded-bl-sm'
        )}>
          {modelLabel && <p className="text-[10px] text-muted-foreground/60 mb-1.5">{modelLabel}</p>}
          <span className="inline-block w-2 h-4 bg-primary animate-pulse align-middle" />
        </div>
      )}
    </div>
  )
}

export function MessageList() {
  const { messages, streaming, streamBuffer, streamBlocks, messageBlocks, statusLabel, truncateAfter, truncateFrom, updateMessage, deleteMessage, prependSkillBlocks, setStreamError, pendingPermission, resolvedPermissions } = useMessages()
  const { activeId } = useConversations()
  const { selectedProviderId, selectedModelId, modelOverrides } = useProviders()
  const enabledTools = useMcpTools(s => s.enabledTools)
  const allTools = useMcpTools(s => s.tools)
  const { enabledContext: enabledSkillsContext, skills } = useSkills()
  const lookupAttachments = useAttachmentCache(s => s.lookup)
  const lookupConversation = useAttachmentCache(s => s.lookupConversation)
  const storeForConversation = useAttachmentCache(s => s.storeForConversation)
  const closeArtifact = useArtifacts(s => s.setActive)
  const bottomRef = useRef<HTMLDivElement>(null)
  const prevStreamingRef = useRef(false)
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const userScrolledUp = useRef(false)

  const modelLabel = useMemo(() => {
    if (!selectedModelId) return undefined
    const overrides = modelOverrides[selectedProviderId] ?? []
    return overrides.find(o => o.model_id === selectedModelId)?.custom_name ?? selectedModelId
  }, [selectedProviderId, selectedModelId, modelOverrides])

  const handleScroll = useCallback(() => {
    const el = scrollContainerRef.current
    if (!el) return
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight
    userScrolledUp.current = distanceFromBottom > 80
  }, [])

  useEffect(() => {
    userScrolledUp.current = false
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages.length, activeId])

  useEffect(() => { closeArtifact(null) }, [activeId])

  // Auto-open artifact panel when streaming ends and last message has artifacts
  useEffect(() => {
    if (prevStreamingRef.current && !streaming) {
      const lastAssistant = [...messages].reverse().find(m => m.role === 'assistant')
      if (lastAssistant) {
        const segs = parseArtifacts(lastAssistant.content, lastAssistant.id)
        const artifacts = segs.filter(s => s.artifact).map(s => s.artifact!)
        if (artifacts.length > 0) closeArtifact(artifacts[artifacts.length - 1])
      }
    }
    prevStreamingRef.current = streaming
  }, [streaming])

  useEffect(() => {
    if (!userScrolledUp.current) {
      bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
    }
  }, [streamBuffer, statusLabel, streamBlocks.length])

  const getSkillsContext = () => {
    const sc = enabledSkillsContext()
    return sc ? `${ARTIFACT_INSTRUCTIONS}\n\n${sc}` : ARTIFACT_INSTRUCTIONS
  }

  const getDisabledTools = () => {
    const enabled = enabledTools()
    const enabledKeys = new Set(enabled.map(toolKey))
    return allTools
      .filter(t => !enabledKeys.has(toolKey(t)))
      .map(toolKey)
  }

  const handleRegenerate = async (assistantMsgId: string) => {
    if (!activeId || streaming) return
    const idx = messages.findIndex(m => m.id === assistantMsgId)
    if (idx <= 0) return
    const preceding = messages.slice(0, idx).reverse().find(m => m.role === 'user')
    if (!preceding) return
    await truncateAfter(preceding.id)
    prependSkillBlocks(skills.filter(s => s.enabled).map(s => s.name))
    try {
      await chat.sendMessage(
        activeId,
        preceding.content,
        getDisabledTools(),
        undefined,
        selectedProviderId || undefined,
        selectedModelId || undefined,
        undefined,
        getSkillsContext(),
      )
    } catch (e) {
      setStreamError(String(e))
    }
  }

  const handleContinue = async (_assistantMsgId: string) => {
    if (!activeId || streaming) return
    prependSkillBlocks(skills.filter(s => s.enabled).map(s => s.name))
    try {
      await chat.continueGeneration(
        activeId,
        getDisabledTools(),
        undefined,
        selectedProviderId || undefined,
        selectedModelId || undefined,
        getSkillsContext(),
      )
    } catch (e) {
      setStreamError(String(e))
    }
  }

  const handleDelete = async (msgId: string) => {
    if (streaming) return
    await deleteMessage(msgId)
    closeArtifact(null)
  }

  const handleEditAssistant = async (msgId: string, newContent: string) => {
    if (streaming) return
    await updateMessage(msgId, newContent)
  }

  const handleEdit = async (msgId: string, newContent: string) => {
    if (!activeId || streaming) return
    const msgObj = messages.find(m => m.id === msgId)
    const atts = (msgObj ? lookupAttachments(msgObj.content) : undefined)
      ?? lookupConversation(activeId)
    if (atts?.length) storeForConversation(activeId, atts)
    const effort = selectedProviderId && selectedModelId
      ? localStorage.getItem(`reasoning:${selectedProviderId}:${selectedModelId}`) ?? undefined
      : undefined
    await truncateFrom(msgId)
    await chat.sendMessage(
      activeId,
      newContent,
      getDisabledTools(),
      effort || undefined,
      selectedProviderId || undefined,
      selectedModelId || undefined,
      atts?.length ? atts : undefined,
    )
  }

  const handleResend = async (msgId: string) => {
    const msgObj = messages.find(m => m.id === msgId)
    if (!msgObj) return
    await handleEdit(msgId, msgObj.content)
  }

  const visibleMessages = messages.filter(
    m => (m.role === 'user' || m.role === 'assistant')
      && !m.content.startsWith(TOOL_CALLS_CONTENT_PREFIX)
  )

  const lastAssistantIdx = visibleMessages.reduce((acc, m, i) => m.role === 'assistant' ? i : acc, -1)

  const versionOf = useMemo(() => {
    const map = new Map<string, number>()
    const counts = new Map<string, number>()
    for (const msg of visibleMessages) {
      if (msg.role !== 'assistant') continue
      for (const seg of parseArtifacts(msg.content, msg.id)) {
        if (!seg.artifact) continue
        // prefer explicit identifier, fall back to normalized title
        const key = seg.artifact.identifier ?? seg.artifact.title.toLowerCase().trim()
        const v = (counts.get(key) ?? 0) + 1
        counts.set(key, v)
        map.set(seg.artifact.id, v)
      }
    }
    return map
  }, [visibleMessages])

  const isStreaming = streaming && (streamBuffer.length > 0 || streamBlocks.length > 0)

  return (
    <div ref={scrollContainerRef} onScroll={handleScroll} className="flex-1 overflow-y-auto px-4 py-6 space-y-6">
      {visibleMessages.map((msg, idx) => {
        const blocks = msg.role === 'assistant' ? (messageBlocks[msg.id] ?? []) : []
        const showPermissions = !streaming && idx === lastAssistantIdx
        const bubble = (
          <MessageBubble
            id={msg.id}
            messageId={msg.id}
            role={msg.role as 'user' | 'assistant'}
            content={msg.content}
            thinking={blocks.length === 0 ? msg.thinking : undefined}
            hasStrip={blocks.length > 0}
            modelLabel={msg.role === 'assistant' ? modelLabel : undefined}
            attachments={msg.role === 'user' ? lookupAttachments(msg.content) : undefined}
            onEdit={msg.role === 'user' ? handleEdit : handleEditAssistant}
            onRegenerate={msg.role === 'assistant' ? handleRegenerate : undefined}
            onContinue={msg.role === 'assistant' ? handleContinue : undefined}
            onDelete={handleDelete}
            onResend={msg.role === 'user' ? handleResend : undefined}
            versionOf={versionOf}
          />
        )
        if (blocks.length === 0) return <div key={msg.id}>{bubble}</div>
        return (
          <div key={msg.id} className="flex flex-col max-w-[75%] w-full">
            <TimelineStrip blocks={blocks} resolvedPermissions={showPermissions ? resolvedPermissions : []} />
            {bubble}
          </div>
        )
      })}
      {statusLabel && !isStreaming && (
        <div className="flex items-center gap-2 text-xs text-muted-foreground px-2">
          <span className="inline-block w-3 h-3 rounded-full bg-primary animate-pulse" />
          {statusLabel}
        </div>
      )}
      {streaming && (
        <StreamingBlocks blocks={streamBlocks} streamBuffer={streamBuffer} modelLabel={modelLabel} resolvedPermissions={resolvedPermissions} pendingPermission={pendingPermission} />
      )}
      <div ref={bottomRef} />
    </div>
  )
}
