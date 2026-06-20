import { useEffect } from 'react'
import { ChatHeader } from './ChatHeader'
import { MessageList } from './MessageList'
import { InputBar } from './InputBar'
import { useConversations } from '../../stores/conversations'
import { useMessages } from '../../stores/messages'
import { useProviders } from '../../stores/providers'

export function ChatView() {
  const { activeId, conversations } = useConversations()
  const { load, startListening } = useMessages()
  const setSelected = useProviders(s => s.setSelected)

  useEffect(() => {
    if (!activeId) return
    load(activeId)
    const conv = conversations.find(c => c.id === activeId)
    if (conv) setSelected(conv.provider_id, conv.model_id)
  }, [activeId])

  useEffect(() => {
    // Track whether this effect instance is still alive.
    // In React StrictMode, effects mount→cleanup→mount rapidly.
    // Without this flag, the first async completes after cleanup
    // and leaves a dangling listener alongside the second mount's listener.
    let alive = true
    let unlisten: (() => void) | undefined

    startListening().then(fn => {
      if (alive) {
        unlisten = fn
      } else {
        fn() // effect already cleaned up — unlisten immediately
      }
    })

    return () => {
      alive = false
      unlisten?.()
    }
  }, [])

  if (!activeId) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <p className="text-muted-foreground text-sm">Select or create a conversation</p>
      </div>
    )
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <ChatHeader />
      <MessageList />
      <InputBar />
    </div>
  )
}
