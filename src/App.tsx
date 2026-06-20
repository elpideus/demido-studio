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

export default function App() {
  const loadConvs = useConversations(s => s.load)
  const listenForTitleUpdates = useConversations(s => s.listenForTitleUpdates)
  const loadProviders = useProviders(s => s.load)
  const { load: loadSettings, settings, loaded } = useSettings()
  const loadMcpTools = useMcpTools(s => s.load)
  const loadSkills = useSkills(s => s.load)
  const [unlocked, setUnlocked] = useState(false)

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
    <div className="flex h-screen bg-background text-foreground overflow-hidden">
      {/* Left spacer: pushes chat right when a panel is snapped left */}
      {leftFraction > 0 && (
        <div style={{ flex: `0 0 ${leftPx}`, minWidth: 0 }} />
      )}

      {/* Base layer: Sidebar + Chat + Artifact panel */}
      <div className="flex flex-1 overflow-hidden min-w-0">
        <Sidebar
          onOpenSettings={() => openWindow('settings', 'settings', 'Settings')}
          onOpenTools={() => openWindow('tools', 'tools', 'Tools')}
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
    </div>
  )
}
