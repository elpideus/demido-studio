import { Trash2 } from 'lucide-react'
import { useState } from 'react'
import { cn } from '../../lib/utils'
import { Button } from '@/components/ui/button'
import { useConversations } from '../../stores/conversations'
import type { Conversation } from '../../types'

interface Props {
  conversation: Conversation
  active: boolean
  onClick: () => void
}

export function ConversationItem({ conversation, active, onClick }: Props) {
  const { remove } = useConversations()
  const [hovered, setHovered] = useState(false)

  return (
    <div
      className={cn(
        'group flex items-center justify-between px-3 py-2 rounded-md cursor-pointer text-sm transition-colors',
        active ? 'bg-primary/20 text-foreground' : 'text-muted-foreground hover:bg-accent hover:text-foreground'
      )}
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <span className="truncate flex-1">{conversation.title}</span>
      {hovered && (
        <Button
          onClick={e => { e.stopPropagation(); remove(conversation.id) }}
          variant="ghost"
          size="icon-xs"
          className="ml-1 shrink-0 text-muted-foreground hover:text-destructive"
        >
          <Trash2 size={12} />
        </Button>
      )}
    </div>
  )
}
