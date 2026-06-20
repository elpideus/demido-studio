import { create } from 'zustand'
import type { FileAttachment } from '../types'

type State = {
  cache: Map<string, FileAttachment[]>
  convCache: Map<string, FileAttachment[]>
  store: (content: string, attachments: FileAttachment[]) => void
  lookup: (content: string) => FileAttachment[] | undefined
  storeForConversation: (conversationId: string, attachments: FileAttachment[]) => void
  lookupConversation: (conversationId: string) => FileAttachment[] | undefined
}

// ponytail: session-only client cache, lost on refresh (backend stores plain text only)
export const useAttachmentCache = create<State>((set, get) => ({
  cache: new Map(),
  convCache: new Map(),
  store: (content, attachments) =>
    set(s => { const m = new Map(s.cache); m.set(content, attachments); return { cache: m } }),
  lookup: (content) => get().cache.get(content),
  storeForConversation: (conversationId, attachments) =>
    set(s => { const m = new Map(s.convCache); m.set(conversationId, attachments); return { convCache: m } }),
  lookupConversation: (conversationId) => get().convCache.get(conversationId),
}))
