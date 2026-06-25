import { useEffect, useState, useCallback, useRef } from 'react'
import { Sidebar } from './components/sidebar/Sidebar'
import { ChatView } from './components/chat/ChatView'
import { AuthGate } from './components/auth/AuthGate'
import { WindowManager } from './components/windows/WindowManager'
import { useConversations } from './stores/conversations'
import { useProviders } from './stores/providers'
import { useSettings } from './stores/settings'
import { useMcpTools } from './stores/mcpTools'
import { useSkills } from './stores/skills'
import { useWindowManager } from './stores/windowManager'
import { useArtifacts } from './stores/artifacts'
import { ArtifactPanel } from './components/artifacts/ArtifactPanel'
import { invoke } from '@tauri-apps/api/core'
import { WindowControls } from './components/WindowControls'

function EarlyAccessDisclaimer({ onAccept }: { onAccept: () => void }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-background border border-border rounded-xl shadow-2xl max-w-md w-full mx-4 p-6 flex flex-col gap-4">
        <div className="flex items-center gap-2">
          <span className="text-yellow-400 text-xl">⚠️</span>
          <h2 className="text-lg font-semibold text-foreground">Early Access — Use at Your Own Risk</h2>
        </div>
        <p className="text-sm text-muted-foreground leading-relaxed">
          This software is in its <strong>very early stages</strong>. It may crash, lose data, behave unexpectedly, or change drastically between updates.
        </p>
        <ul className="text-sm text-muted-foreground list-disc list-inside space-y-1">
          <li>Not production-ready</li>
          <li>No guarantees of data safety or stability</li>
          <li>Your API keys and data are your responsibility</li>
          <li>Features may break or disappear without notice</li>
        </ul>
        <p className="text-sm text-muted-foreground">
          By continuing, you accept full responsibility for your use of this software.
        </p>
        <button
          onClick={onAccept}
          className="mt-2 w-full rounded-lg bg-primary text-primary-foreground py-2 text-sm font-medium hover:bg-primary/90 transition-colors"
        >
          I Understand, Continue
        </button>
      </div>
    </div>
  )
}

export default function App() {
  const loadConvs = useConversations(s => s.load)
  const listenForTitleUpdates = useConversations(s => s.listenForTitleUpdates)
  const loadProviders = useProviders(s => s.load)
  const { load: loadSettings, settings, loaded } = useSettings()
  const loadMcpTools = useMcpTools(s => s.load)
  const loadSkills = useSkills(s => s.load)
  const [unlocked, setUnlocked] = useState(false)
  const [disclaimerAccepted, setDisclaimerAccepted] = useState(
    () => localStorage.getItem('disclaimer_accepted') === '1'
  )

  useEffect(() => {
    (window as any).resetDisclaimer = () => {
      localStorage.removeItem('disclaimer_accepted')
      setDisclaimerAccepted(false)
    }
  }, [])

  const { openWindow, snapLayout } = useWindowManager()
  const artifactOpen = useArtifacts(s => s.activeArtifact !== null)
  const [artifactWidth, setArtifactWidth] = useState(420)
  const [isDragging, setIsDragging] = useState(false)
  const dragging = useRef(false)

  const onDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    dragging.current = true
    setIsDragging(true)
    const startX = e.clientX
    const startW = artifactWidth

    const onMove = (ev: MouseEvent) => {
      if (!dragging.current) return
      const delta = startX - ev.clientX
      const maxW = Math.floor(window.innerWidth * 0.75)
      const minW = Math.max(280, Math.floor(window.innerWidth * 0.2))
      setArtifactWidth(Math.max(minW, Math.min(maxW, startW + delta)))
    }
    const onUp = () => {
      dragging.current = false
      setIsDragging(false)
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [artifactWidth])
  const leftFraction  = snapLayout.left?.fraction  ?? 0
  const rightFraction = snapLayout.right?.fraction ?? 0
  const leftPx  = `${leftFraction  * 100}%`
  const rightPx = `${rightFraction * 100}%`

  useEffect(() => {
    loadSettings()
  }, [])

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if ((e.ctrlKey && e.shiftKey && e.key === 'I') || e.key === 'F12') {
        e.preventDefault()
        invoke('open_devtools')
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  useEffect(() => {
    if (loaded && (!settings.auth_enabled || unlocked)) {
      loadConvs()
      loadProviders()
      loadMcpTools()
      loadSkills()
      let cancelled = false
      let cleanup: (() => void) | undefined
      listenForTitleUpdates().then(fn => {
        if (cancelled) { fn(); return }
        cleanup = fn
      })
      return () => {
        cancelled = true
        cleanup?.()
      }
    }
  }, [loaded, unlocked, settings.auth_enabled])

  if (!loaded) return null

  if (settings.auth_enabled && !unlocked) {
    return <AuthGate onUnlock={() => setUnlocked(true)} />
  }

  return (
    <>
    {!disclaimerAccepted && (
      <EarlyAccessDisclaimer onAccept={() => {
        localStorage.setItem('disclaimer_accepted', '1')
        setDisclaimerAccepted(true)
      }} />
    )}
    <div className="flex h-screen bg-background text-foreground overflow-hidden relative">
      <div className="flex flex-1 overflow-hidden min-w-0">
      {/* Left spacer: pushes chat right when a panel is snapped left */}
      {leftFraction > 0 && (
        <div style={{ flex: `0 0 ${leftPx}`, minWidth: 0 }} />
      )}

      {/* Base layer: Sidebar + Chat + Artifact panel */}
      <div className="flex flex-1 overflow-hidden min-w-0">
        <Sidebar
          onOpenSettings={() => openWindow('settings', 'settings', 'Settings')}
          onOpenTools={() => openWindow('tools', 'tools', 'Tools')}
          onOpenAccounts={() => openWindow('accounts', 'accounts', 'Accounts')}
          onOpenEmail={() => openWindow('email', 'email', 'Email', { initialSize: { width: Math.round(window.innerWidth * 0.88), height: Math.round(window.innerHeight * 0.88) } })}
          onOpenCalendar={() => openWindow('calendar', 'calendar', 'Calendar')}
          onOpenContacts={() => openWindow('contacts', 'contacts', 'Contacts')}
        />
        <ChatView />
        {artifactOpen && (
          <>
            <div
              onMouseDown={onDragStart}
              className="w-1 shrink-0 cursor-col-resize hover:bg-primary/40 transition-colors bg-border"
            />
            <ArtifactPanel width={artifactWidth} isDragging={isDragging} />
          </>
        )}
      </div>

      {/* Right spacer: pushes chat left when a panel is snapped right */}
      {rightFraction > 0 && (
        <div style={{ flex: `0 0 ${rightPx}`, minWidth: 0 }} />
      )}

      {/* Window system overlay — floats above everything */}
      <WindowManager />
      <WindowControls />
      <span className="fixed bottom-2 right-3 text-[10px] text-foreground/50 pointer-events-none select-none z-50">v{__APP_VERSION__}</span>
      </div>
    </div>
    </>
  )
}
