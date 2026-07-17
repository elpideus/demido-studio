import { create } from 'zustand'
import { listen } from '@tauri-apps/api/event'
import { db } from '../lib/tauri'
import type { Conversation, CavemanLevel } from '../types'
import { useArtifacts } from './artifacts'

type AgentMode = 'off' | 'cautious' | 'balanced' | 'autonomous'

interface ConversationsStore {
  conversations: Conversation[]
  activeId: string | null
  /** Defaults picked on the home page, applied to the next conversation created. */
  pendingAgentMode: AgentMode
  pendingCavemanLevel: CavemanLevel
  pendingWorkingDirectory: string | null
  setPendingAgentMode: (mode: AgentMode) => void
  setPendingCavemanLevel: (level: CavemanLevel) => void
  setPendingWorkingDirectory: (path: string | null) => void
  load: () => Promise<void>
  create: (providerId: string, modelId: string) => Promise<Conversation>
  remove: (id: string) => Promise<void>
  setActive: (id: string | null) => void
  updateTitle: (id: string, title: string) => Promise<void>
  setAgentMode: (id: string, mode: AgentMode) => Promise<void>
  setCavemanLevel: (id: string, level: CavemanLevel) => Promise<void>
  setWorkingDirectory: (id: string, path: string | null) => Promise<void>
  listenForTitleUpdates: () => Promise<() => void>
}

export const useConversations = create<ConversationsStore>((set, get) => ({
  conversations: [],
  activeId: null,
  pendingAgentMode: 'off',
  pendingCavemanLevel: 'off',
  pendingWorkingDirectory: null,

  // Agent off has no working folder, same as the header's rule — otherwise a folder picked
  // before switching off would ride along into the new conversation unseen.
  setPendingAgentMode: (mode) =>
    set(mode === 'off' ? { pendingAgentMode: mode, pendingWorkingDirectory: null } : { pendingAgentMode: mode }),
  setPendingCavemanLevel: (level) => set({ pendingCavemanLevel: level }),
  setPendingWorkingDirectory: (path) => set({ pendingWorkingDirectory: path }),

  load: async () => {
    const conversations = await db.listConversations()
    set({ conversations })
  },

  create: async (providerId, modelId) => {
    const created = await db.createConversation(providerId, modelId)
    const { pendingAgentMode, pendingCavemanLevel, pendingWorkingDirectory } = get()
    if (pendingAgentMode !== 'off') await db.setAgentMode(created.id, pendingAgentMode)
    if (pendingCavemanLevel !== 'off') await db.setCavemanLevel(created.id, pendingCavemanLevel)
    if (pendingWorkingDirectory !== null) await db.setWorkingDirectory(created.id, pendingWorkingDirectory)
    const conv: Conversation = {
      ...created,
      agent_mode: pendingAgentMode,
      caveman_level: pendingCavemanLevel,
      working_directory: pendingWorkingDirectory,
    }
    set(s => ({ conversations: [conv, ...s.conversations], activeId: conv.id }))
    return conv
  },

  remove: async (id) => {
    await db.deleteConversation(id)
    set(s => {
      if (s.activeId === id) useArtifacts.getState().setActive(null)
      return {
        conversations: s.conversations.filter(c => c.id !== id),
        activeId: s.activeId === id ? null : s.activeId,
      }
    })
  },

  setActive: (id) => set({ activeId: id }),

  updateTitle: async (id, title) => {
    await db.updateConversationTitle(id, title)
    set(s => ({
      conversations: s.conversations.map(c => c.id === id ? { ...c, title } : c),
    }))
  },

  setAgentMode: async (id, mode) => {
    await db.setAgentMode(id, mode)
    set(s => ({
      conversations: s.conversations.map(c => c.id === id ? { ...c, agent_mode: mode } : c),
    }))
  },

  setCavemanLevel: async (id, level) => {
    await db.setCavemanLevel(id, level)
    set(s => ({
      conversations: s.conversations.map(c => c.id === id ? { ...c, caveman_level: level } : c),
    }))
  },

  setWorkingDirectory: async (id, path) => {
    await db.setWorkingDirectory(id, path)
    set(s => ({
      conversations: s.conversations.map(c => c.id === id ? { ...c, working_directory: path } : c),
    }))
  },

  listenForTitleUpdates: async () => {
    const unlisten = await listen<{ id: string; title: string }>('conversation_title_updated', (e) => {
      set(s => ({
        conversations: s.conversations.map(c =>
          c.id === e.payload.id ? { ...c, title: e.payload.title } : c
        ),
      }))
    })
    return unlisten
  },
}))
