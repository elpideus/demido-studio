import { create } from 'zustand'
import { listen } from '@tauri-apps/api/event'
import { db, agent } from '../lib/tauri'
import type { Message } from '../types'

// Module-level — survives React component remounts, reset on cleanup
let _activeCleanup: (() => void) | null = null

const BLOCKS_STORAGE_KEY = 'demido:messageBlocks'

function loadPersistedBlocks(): Record<string, StreamBlock[]> {
  try {
    const raw = localStorage.getItem(BLOCKS_STORAGE_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

function persistBlocks(blocks: Record<string, StreamBlock[]>) {
  try {
    localStorage.setItem(BLOCKS_STORAGE_KEY, JSON.stringify(blocks))
  } catch {
    // ignore quota errors
  }
}

export interface ThinkingBlock {
  type: 'thinking'
  content: string
  done: boolean
}

export interface ToolBlock {
  type: 'tool'
  id: string
  name: string
  args: unknown
  result?: string
  done: boolean
}

export interface SkillBlock {
  type: 'skill'
  name: string
}

export type StreamBlock = ThinkingBlock | ToolBlock | SkillBlock

export interface PermissionRequest {
  toolName: string
  args: Record<string, unknown>
  description: string
}

export interface ResolvedPermission {
  toolName: string
  description: string
  approved: boolean
}

interface MessagesStore {
  messages: Message[]
  streaming: boolean
  streamBuffer: string
  streamBlocks: StreamBlock[]
  messageBlocks: Record<string, StreamBlock[]>
  statusLabel: string | null
  streamError: string | null
  pendingPermission: PermissionRequest | null
  resolvedPermissions: ResolvedPermission[]
  load: (conversationId: string) => Promise<void>
  addMessage: (msg: Message) => void
  truncateAfter: (messageId: string) => Promise<void>
  truncateFrom: (messageId: string) => Promise<void>
  deleteMessage: (messageId: string) => Promise<void>
  updateMessage: (messageId: string, content: string) => Promise<void>
  respondToPermission: (approved: boolean) => Promise<void>
  prependSkillBlocks: (skillNames: string[]) => void
  setStreamError: (msg: string | null) => void
  startListening: () => Promise<() => void>
}

function resetStream() {
  return {
    streaming: false,
    streamBuffer: '',
    streamBlocks: [] as StreamBlock[],
    statusLabel: null,
  }
}

export const useMessages = create<MessagesStore>((set, get) => ({
  messages: [],
  streaming: false,
  streamBuffer: '',
  streamBlocks: [],
  messageBlocks: loadPersistedBlocks(),
  statusLabel: null,
  streamError: null,
  pendingPermission: null,
  resolvedPermissions: [],

  setStreamError: (msg) => {
    set({ streamError: msg })
    if (msg) setTimeout(() => set({ streamError: null }), 5000)
  },

  load: async (conversationId) => {
    const messages = await db.listMessages(conversationId)
    set({ messages, ...resetStream(), pendingPermission: null, resolvedPermissions: [] })
  },

  addMessage: (msg) => set(s => ({ messages: [...s.messages, msg] })),

  truncateAfter: async (messageId) => {
    await db.deleteMessagesAfter(messageId)
    set(s => {
      const idx = s.messages.findIndex(m => m.id === messageId)
      if (idx === -1) return s
      return { messages: s.messages.slice(0, idx + 1) }
    })
  },

  truncateFrom: async (messageId) => {
    await db.deleteMessagesFrom(messageId)
    set(s => {
      const idx = s.messages.findIndex(m => m.id === messageId)
      if (idx === -1) return s
      return { messages: s.messages.slice(0, idx) }
    })
  },

  deleteMessage: async (messageId) => {
    await db.deleteMessage(messageId)
    set(s => ({ messages: s.messages.filter(m => m.id !== messageId) }))
  },

  updateMessage: async (messageId, content) => {
    await db.updateMessageContent(messageId, content)
    set(s => ({
      messages: s.messages.map(m => m.id === messageId ? { ...m, content } : m),
    }))
  },

  prependSkillBlocks: (skillNames) => {
    if (!skillNames.length) return
    const blocks: SkillBlock[] = skillNames.map(name => ({ type: 'skill', name }))
    set(s => ({ streamBlocks: [...blocks, ...s.streamBlocks] }))
  },

  respondToPermission: async (approved) => {
    const pending = get().pendingPermission
    if (!pending) return
    set(s => ({
      pendingPermission: null,
      resolvedPermissions: [...s.resolvedPermissions, { toolName: pending.toolName, description: pending.description, approved }],
    }))
    await agent.respondToPermission(approved)
  },

  startListening: async () => {
    if (_activeCleanup) {
      _activeCleanup()
      _activeCleanup = null
    }

    let streamOpen = false

    const unlistenUserMsg = await listen<Message>('user_message', (e) => {
      set(s => ({ messages: [...s.messages, e.payload] }))
    })

    const unlistenThinking = await listen<string>('stream_thinking', (e) => {
      if (!streamOpen) return
      set(s => {
        const blocks = [...s.streamBlocks]
        const last = blocks[blocks.length - 1]
        if (last && last.type === 'thinking' && !last.done) {
          blocks[blocks.length - 1] = { ...last, content: last.content + e.payload }
        } else {
          blocks.push({ type: 'thinking', content: e.payload, done: false })
        }
        return { streamBlocks: blocks, streaming: true, statusLabel: null }
      })
    })

    const unlistenThinkingEnd = await listen('stream_thinking_end', () => {
      if (!streamOpen) return
      set(s => {
        const blocks = [...s.streamBlocks]
        const last = blocks[blocks.length - 1]
        if (last && last.type === 'thinking' && !last.done) {
          blocks[blocks.length - 1] = { ...last, done: true }
        }
        return { streamBlocks: blocks }
      })
    })

    const unlistenToken = await listen<string>('stream_token', (e) => {
      if (!streamOpen) return
      set(s => {
        // Seal any open thinking block when content starts arriving
        const blocks = [...s.streamBlocks]
        const last = blocks[blocks.length - 1]
        if (last && last.type === 'thinking' && !last.done) {
          blocks[blocks.length - 1] = { ...last, done: true }
        }
        return { streamBuffer: s.streamBuffer + e.payload, streaming: true, statusLabel: null, streamBlocks: blocks }
      })
    })

    const unlistenDone = await listen<Message>('stream_done', (e) => {
      streamOpen = false
      set(s => {
        const newBlocks = s.streamBlocks.length > 0
          ? { ...s.messageBlocks, [e.payload.id]: s.streamBlocks }
          : s.messageBlocks
        if (s.streamBlocks.length > 0) persistBlocks(newBlocks)
        return {
          messages: [...s.messages, e.payload],
          messageBlocks: newBlocks,
          ...resetStream(),
        }
      })
    })

    const unlistenContinueDone = await listen<Message>('stream_continue_done', (e) => {
      streamOpen = false
      set(s => {
        // Merge new stream blocks into the existing blocks for this message
        const existing = s.messageBlocks[e.payload.id] ?? []
        const merged = s.streamBlocks.length > 0 ? [...existing, ...s.streamBlocks] : existing
        const newBlocks = merged.length > 0
          ? { ...s.messageBlocks, [e.payload.id]: merged }
          : s.messageBlocks
        if (merged.length > 0) persistBlocks(newBlocks)
        return {
          messages: s.messages.map(m => m.id === e.payload.id ? e.payload : m),
          messageBlocks: newBlocks,
          ...resetStream(),
        }
      })
    })

    const unlistenTool = await listen<{ name: string; id: string; args: unknown }>('tool_call', (e) => {
      if (!streamOpen) return
      set(s => {
        // Seal any open thinking block
        const blocks = [...s.streamBlocks]
        const last = blocks[blocks.length - 1]
        if (last && last.type === 'thinking' && !last.done) {
          blocks[blocks.length - 1] = { ...last, done: true }
        }
        blocks.push({ type: 'tool', id: e.payload.id, name: e.payload.name, args: e.payload.args, done: false })
        return { streamBlocks: blocks, streaming: true, statusLabel: null }
      })
    })

    const unlistenToolResult = await listen<{ id: string; name: string; result: string }>('tool_call_result', (e) => {
      if (!streamOpen) return
      set(s => {
        const blocks = s.streamBlocks.map(b =>
          b.type === 'tool' && b.id === e.payload.id
            ? { ...b, result: e.payload.result, done: true }
            : b
        )
        return { streamBlocks: blocks }
      })
    })

    const unlistenStatus = await listen<{ label: string | null }>('stream_status', (e) => {
      streamOpen = true
      set({ statusLabel: e.payload.label ?? null })
    })

    const unlistenCancelled = await listen('stream_cancelled', () => {
      streamOpen = false
      set({ ...resetStream(), pendingPermission: null })
    })

    const unlistenPermission = await listen<PermissionRequest>('tool_permission_request', (e) => {
      set({ pendingPermission: e.payload })
    })

    const cleanup = () => {
      unlistenThinking()
      unlistenThinkingEnd()
      unlistenToken()
      unlistenDone()
      unlistenContinueDone()
      unlistenTool()
      unlistenToolResult()
      unlistenStatus()
      unlistenCancelled()
      unlistenPermission()
      unlistenUserMsg()
      _activeCleanup = null
    }
    _activeCleanup = cleanup
    return cleanup
  },
}))
