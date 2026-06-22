import { useCallback, useEffect, useRef, useState } from 'react'
import { Plus, Settings, Wrench } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { ConversationItem } from './ConversationItem'
import { SearchBar } from './SearchBar'
import { FileExplorer } from './FileExplorer'
import { useConversations } from '../../stores/conversations'
import { useProviders } from '../../stores/providers'

const MIN_W = 180, MAX_W = 500, DEFAULT_W = 240
const MIN_EXPLORER_H = 60, MAX_EXPLORER_H = 600, DEFAULT_EXPLORER_H = 200

interface Props {
  onOpenSettings: () => void
  onOpenTools: () => void
}

export function Sidebar({ onOpenSettings, onOpenTools }: Props) {
  const { create, conversations, activeId, setActive } = useConversations()
  const { selectedProviderId, selectedModelId } = useProviders()
  const [width, setWidthRaw] = useState(DEFAULT_W)
  const setWidth = (w: number) => setWidthRaw(Math.min(MAX_W, Math.max(MIN_W, w)))

  // Horizontal (width) drag
  const hDragRef = useRef(false)
  const hStartXRef = useRef(0)
  const hStartWRef = useRef(0)
  const widthRef = useRef(width)
  widthRef.current = width

  const handleHMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    hDragRef.current = true
    hStartXRef.current = e.clientX
    hStartWRef.current = widthRef.current
    document.body.style.userSelect = 'none'
    document.body.style.cursor = 'col-resize'
  }, [])

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!hDragRef.current) return
      setWidth(hStartWRef.current + (e.clientX - hStartXRef.current))
    }
    const onUp = () => {
      if (hDragRef.current) {
        hDragRef.current = false
        document.body.style.userSelect = ''
        document.body.style.cursor = ''
      }
    }
    document.addEventListener('mousemove', onMove)
    document.addEventListener('mouseup', onUp)
    return () => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseup', onUp)
      document.body.style.userSelect = ''
      document.body.style.cursor = ''
    }
  }, [])

  // Vertical (explorer height) drag
  const [explorerH, setExplorerHRaw] = useState(DEFAULT_EXPLORER_H)
  const setExplorerH = (h: number) => setExplorerHRaw(Math.max(MIN_EXPLORER_H, Math.min(MAX_EXPLORER_H, h)))
  const vDragRef = useRef(false)
  const vStartYRef = useRef(0)
  const vStartHRef = useRef(0)
  const explorerHRef = useRef(explorerH)
  explorerHRef.current = explorerH

  const handleVMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    vDragRef.current = true
    vStartYRef.current = e.clientY
    vStartHRef.current = explorerHRef.current
    document.body.style.userSelect = 'none'
    document.body.style.cursor = 'row-resize'
  }, [])

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!vDragRef.current) return
      // drag up = grow explorer
      setExplorerH(vStartHRef.current + (vStartYRef.current - e.clientY))
    }
    const onUp = () => {
      if (vDragRef.current) {
        vDragRef.current = false
        document.body.style.userSelect = ''
        document.body.style.cursor = ''
      }
    }
    document.addEventListener('mousemove', onMove)
    document.addEventListener('mouseup', onUp)
    return () => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseup', onUp)
    }
  }, [])

  const activeConv = conversations.find(c => c.id === activeId)
  const workingDir = activeConv?.working_directory ?? null
  const showExplorer = workingDir !== null && activeConv?.agent_mode !== 'off'

  return (
    <div className="flex flex-col border-r border-border bg-card shrink-0 relative" style={{ width }}>
      <div className="p-4 flex items-center justify-between border-b border-border">
        <span className="text-sm font-semibold text-foreground">Demido</span>
        <Button
          onClick={() => create(selectedProviderId, selectedModelId)}
          variant="ghost"
          size="icon-sm"
          title="New conversation"
          className="text-muted-foreground"
        >
          <Plus size={16} />
        </Button>
      </div>
      <SearchBar />

      {/* Conversations + optional file explorer */}
      <div className="flex-1 flex flex-col overflow-hidden min-h-0">
        {/* Conversation list */}
        <div className="flex-1 min-h-0 overflow-y-auto">
          {conversations.length === 0 ? (
            <div className="h-full flex flex-col items-center justify-center p-4 gap-2">
              <img src="/violet.png" alt="" className="w-44 select-none pointer-events-none" style={{filter: 'saturate(0%)', opacity: 0.05, maskImage: 'linear-gradient(to bottom, black 50%, transparent 100%)', WebkitMaskImage: 'linear-gradient(to bottom, black 50%, transparent 100%)'}}/>
              <p className="text-xs text-muted-foreground text-center opacity-30">No chats yet.</p>
            </div>
          ) : (
            <div className="p-2 space-y-0.5">
              {conversations.map(conv => (
                <ConversationItem key={conv.id} conversation={conv} active={conv.id === activeId} onClick={() => setActive(conv.id)} />
              ))}
            </div>
          )}
        </div>

        {/* File explorer pane */}
        {showExplorer && (
          <>
            {/* Vertical resize handle */}
            <div
              onMouseDown={handleVMouseDown}
              className="h-1 shrink-0 cursor-row-resize hover:bg-primary/30 bg-border transition-colors"
            />
            <div className="shrink-0 flex flex-col overflow-hidden" style={{ height: explorerH }}>
              <div className="px-3 py-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground border-b border-border/50 shrink-0">
                Explorer
              </div>
              <FileExplorer rootPath={workingDir!} conversationId={activeId!} />
            </div>
          </>
        )}
      </div>

      <div className="p-2 border-t border-border">
        <Button onClick={onOpenTools} variant="ghost" size="sm" className="w-full justify-start text-muted-foreground">
          <Wrench size={14} />
          Tools
        </Button>
        <Button onClick={onOpenSettings} variant="ghost" size="sm" className="w-full justify-start text-muted-foreground">
          <Settings size={14} />
          Settings
        </Button>
      </div>

      {/* Horizontal resize handle */}
      <div
        onMouseDown={handleHMouseDown}
        className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-accent active:bg-accent/80 transition-colors z-10"
      />
    </div>
  )
}
