import { useCallback, useEffect, useRef, useState } from 'react'
import { MessageSquare, FolderOpen, Settings, Wrench, Mail, Calendar, Users, UserCircle, Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { ConversationItem } from './ConversationItem'
import { SearchBar } from './SearchBar'
import { FileExplorer } from './FileExplorer'
import { useConversations } from '../../stores/conversations'
import { useProviders } from '../../stores/providers'

const MIN_W = 180, MAX_W = 500, DEFAULT_W = 240
const MIN_EXPLORER_H = 60, MAX_EXPLORER_H = 600, DEFAULT_EXPLORER_H = 200

type SidebarView = 'chats' | 'files' | null

interface Props {
  onOpenSettings: () => void
  onOpenTools: () => void
  onOpenAccounts: () => void
  onOpenEmail: () => void
  onOpenCalendar: () => void
  onOpenContacts: () => void
}

function Tooltip({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="relative group/tip">
      {children}
      <div className="absolute left-full ml-2 top-1/2 -translate-y-1/2 z-50 pointer-events-none opacity-0 group-hover/tip:opacity-100 transition-opacity delay-300">
        <div className="bg-popover border border-border text-foreground text-xs px-2 py-1 rounded-md shadow-md whitespace-nowrap">
          {label}
        </div>
      </div>
    </div>
  )
}

function IconBtn({ icon, label, active, onClick }: { icon: React.ReactNode; label: string; active?: boolean; onClick: () => void }) {
  return (
    <Tooltip label={label}>
      <button
        onClick={onClick}
        className={`w-10 h-10 flex items-center justify-center rounded-lg transition-colors ${
          active
            ? 'text-foreground bg-accent'
            : 'text-muted-foreground hover:text-foreground hover:bg-accent/60'
        }`}
      >
        {icon}
      </button>
    </Tooltip>
  )
}

export function Sidebar({ onOpenSettings, onOpenTools, onOpenAccounts, onOpenEmail, onOpenCalendar, onOpenContacts }: Props) {
  const { create, conversations, activeId, setActive } = useConversations()
  const { selectedProviderId, selectedModelId } = useProviders()
  const [view, setView] = useState<SidebarView>('chats')
  const [width, setWidthRaw] = useState(DEFAULT_W)
  const setWidth = (w: number) => setWidthRaw(Math.min(MAX_W, Math.max(MIN_W, w)))

  // Horizontal (width) drag for content panel
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
      setWidth(hStartWRef.current + (e.clientX - hStartXRef.current) - 48)
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

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!vDragRef.current) return
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
  const showFiles = workingDir !== null && activeConv?.agent_mode !== 'off'

  const toggleView = (v: SidebarView) => setView(cur => cur === v ? null : v)

  return (
    <div className="flex h-full shrink-0 border-r border-border">
      {/* Activity bar — 48px icon column */}
      <div className="w-12 flex flex-col items-center py-2 gap-1 bg-card border-r border-border/50 shrink-0">
        {/* Top icons */}
        <div
          className="mb-1 cursor-pointer"
          onClick={() => setActive(null)}
          title="Home"
        >
          <img src="/logo.svg" alt="Demido Studio" className="h-7 w-7 select-none pointer-events-none" />
        </div>

        <IconBtn
          icon={<MessageSquare size={18} />}
          label="Chats"
          active={view === 'chats'}
          onClick={() => toggleView('chats')}
        />

        {showFiles && (
          <IconBtn
            icon={<FolderOpen size={18} />}
            label="File Explorer"
            active={view === 'files'}
            onClick={() => toggleView('files')}
          />
        )}

        {/* Spacer */}
        <div className="flex-1" />

        {/* Bottom icons */}
        <IconBtn icon={<Mail size={18} />} label="Email" onClick={onOpenEmail} />
        <IconBtn icon={<Calendar size={18} />} label="Calendar" onClick={onOpenCalendar} />
        <IconBtn icon={<Users size={18} />} label="Contacts" onClick={onOpenContacts} />
        <IconBtn icon={<UserCircle size={18} />} label="Accounts" onClick={onOpenAccounts} />
        <div className="h-px w-6 bg-border/60 my-1" />
        <IconBtn icon={<Wrench size={18} />} label="Tools" onClick={onOpenTools} />
        <IconBtn icon={<Settings size={18} />} label="Settings" onClick={onOpenSettings} />
      </div>

      {/* Content panel — only shown when a view is active */}
      {view && (
        <div className="flex flex-col border-r border-border bg-card relative" style={{ width }}>
          {view === 'chats' && (
            <>
              <div className="p-3 flex items-center justify-between border-b border-border shrink-0">
                <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Chats</span>
                <Button
                  onClick={() => create(selectedProviderId, selectedModelId)}
                  variant="ghost"
                  size="icon-sm"
                  title="New conversation"
                  className="text-muted-foreground"
                >
                  <Plus size={14} />
                </Button>
              </div>
              <SearchBar />
              <div className="flex-1 min-h-0 overflow-y-auto">
                {conversations.length === 0 ? (
                  <div className="h-full flex flex-col items-center justify-center p-4 gap-2">
                    <img src="/violet.png" alt="" className="w-44 select-none pointer-events-none" style={{ filter: 'saturate(0%)', opacity: 0.05, maskImage: 'linear-gradient(to bottom, black 50%, transparent 100%)', WebkitMaskImage: 'linear-gradient(to bottom, black 50%, transparent 100%)' }} />
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
            </>
          )}

          {view === 'files' && showFiles && (
            <>
              <div className="px-3 py-2.5 text-xs font-semibold uppercase tracking-wider text-muted-foreground border-b border-border shrink-0">
                Explorer
              </div>
              <div className="flex-1 flex flex-col overflow-hidden">
                <FileExplorer rootPath={workingDir!} conversationId={activeId!} />
              </div>
            </>
          )}

          {/* Horizontal resize handle */}
          <div
            onMouseDown={handleHMouseDown}
            className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-accent active:bg-accent/80 transition-colors z-10"
          />
        </div>
      )}
    </div>
  )
}
