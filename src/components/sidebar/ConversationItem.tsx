import { MoreVertical, Pencil, Trash2 } from 'lucide-react'
import { useState, useRef, useEffect } from 'react'
import { cn, clampToViewport } from '../../lib/utils'
import { Button } from '@/components/ui/button'
import { useConversations } from '../../stores/conversations'
import type { Conversation } from '../../types'

interface Props {
  conversation: Conversation
  active: boolean
  onClick: () => void
}

export function ConversationItem({ conversation, active, onClick }: Props) {
  const { remove, updateTitle } = useConversations()
  const [hovered, setHovered] = useState(false)
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number } | null>(null)
  const [renaming, setRenaming] = useState(false)
  const [renameValue, setRenameValue] = useState(conversation.title)
  const renameInputRef = useRef<HTMLInputElement>(null)
  const ctxMenuRef = useRef<HTMLDivElement>(null)
  const menuBtnRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (renaming && renameInputRef.current) {
      renameInputRef.current.focus()
      renameInputRef.current.select()
    }
  }, [renaming])

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

  const openCtxMenu = (x: number, y: number) => {
    if (renaming) return
    setCtxMenu({ x, y })
  }

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    openCtxMenu(e.clientX, e.clientY)
  }

  const handleMenuClick = (e: React.MouseEvent) => {
    e.stopPropagation()
    if (ctxMenu) { setCtxMenu(null); return }
    const rect = menuBtnRef.current?.getBoundingClientRect()
    openCtxMenu(rect ? rect.left : e.clientX, rect ? rect.bottom + 4 : e.clientY)
  }

  const handleRenameStart = () => {
    setCtxMenu(null)
    setRenameValue(conversation.title)
    setRenaming(true)
  }

  const handleRenameCommit = () => {
    const trimmed = renameValue.trim()
    if (trimmed && trimmed !== conversation.title) {
      updateTitle(conversation.id, trimmed)
    }
    setRenaming(false)
  }

  const handleRenameCancel = () => setRenaming(false)

  const handleDelete = () => {
    setCtxMenu(null)
    remove(conversation.id)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') handleRenameCommit()
    if (e.key === 'Escape') handleRenameCancel()
  }

  return (
    <div
      className={cn(
        'group flex items-center justify-between px-3 py-2 rounded-md cursor-pointer text-sm transition-colors',
        active ? 'bg-primary/20 text-foreground' : 'text-muted-foreground hover:bg-accent hover:text-foreground'
      )}
      onClick={renaming ? undefined : onClick}
      onContextMenu={handleContextMenu}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {renaming ? (
        <input
          ref={renameInputRef}
          value={renameValue}
          onChange={e => setRenameValue(e.target.value)}
          onBlur={handleRenameCommit}
          onKeyDown={handleKeyDown}
          onClick={e => e.stopPropagation()}
          className="flex-1 bg-transparent border border-[var(--primary)]/50 rounded px-1.5 py-0.5 text-sm text-foreground outline-none"
        />
      ) : (
        <span className="truncate flex-1">{conversation.title}</span>
      )}
      <Button
        ref={menuBtnRef}
        onClick={handleMenuClick}
        variant="ghost"
        size="icon-xs"
        className={cn(
          'ml-1 shrink-0 text-muted-foreground transition-opacity',
          hovered && !renaming ? 'opacity-100' : 'opacity-0'
        )}
      >
        <MoreVertical size={12} />
      </Button>

      {ctxMenu && (
        <div
          ref={ctxMenuRef}
          className="fixed z-50 min-w-[130px] bg-popover border border-border rounded-md shadow-md py-1 text-[12px]"
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          onMouseDown={e => e.stopPropagation()}
        >
          <button
            className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80 flex items-center gap-2"
            onClick={handleRenameStart}
          >
            <Pencil size={11} /> Rename
          </button>
          <div className="border-t border-border/40 my-1" />
          <button
            className="w-full text-left px-3 py-1.5 hover:bg-red-500/20 text-red-400 flex items-center gap-2"
            onClick={handleDelete}
          >
            <Trash2 size={11} /> Delete
          </button>
        </div>
      )}
    </div>
  )
}
