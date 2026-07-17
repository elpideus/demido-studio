import { useState, useEffect } from 'react'
import type { ReactNode } from 'react'
import { PanelRightClose } from 'lucide-react'
import * as fileIconsJs from 'file-icons-js'
import { WindowFrame } from './WindowFrame'
import { SettingsPanelContent } from '../settings/SettingsPanelContent'
import { ToolsPanelContent } from '../tools/ToolsPanelContent'
import { ImageEditorContent } from '../image-editor/ImageEditorContent'
import { AccountsWindow, AccountsTitleBarActions } from './AccountsWindow'
import { CalendarWindow, CalendarTitleBarActions } from './CalendarWindow'
import { EmailWindow } from './EmailWindow'
import { ContactsWindow } from './ContactsWindow'
import { GraphifyWindow } from './GraphifyWindow'
import { ArtifactPanel } from '../artifacts/ArtifactPanel'
import { useWindowManager } from '../../stores/windowManager'
import { useArtifacts } from '../../stores/artifacts'
import { getExtension } from '../../lib/parseArtifacts'
import type { WindowComponent } from '../../types'

function ArtifactWindowCollapseAction() {
  const setPoppedOut = useArtifacts(s => s.setPoppedOut)
  const closeWindow = useWindowManager(s => s.closeWindow)
  return (
    <button
      className="p-1.5 rounded-md hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
      title="Collapse to side panel"
      onMouseDown={e => e.stopPropagation()}
      onClick={() => {
        setPoppedOut(false)
        closeWindow('artifact-viewer')
      }}
    >
      <PanelRightClose size={14} />
    </button>
  )
}

/** The open file, shown in the title bar after a separator: `Skill name │ md SKILL.md`. */
function ArtifactWindowTitleInfo() {
  const activeArtifact = useArtifacts(s => s.activeArtifact)
  const windowTitle = useWindowManager(s => s.windows['artifact-viewer']?.title)
  if (!activeArtifact) return null
  // For a plain artifact the window is already named after it — only the type badge adds anything.
  const showName = activeArtifact.title !== windowTitle
  const iconCls = fileIconsJs.getClassWithColor(`artifact${getExtension(activeArtifact.type)}`)
  return (
    <div className="flex items-center gap-2 min-w-0 pl-3 ml-3 border-l border-border">
      {iconCls && <span className={iconCls} style={{ fontSize: 13, lineHeight: 1, display: 'inline-block', width: 13 }} />}
      <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-secondary text-muted-foreground border border-border shrink-0">
        {activeArtifact.type}
      </span>
      {showName && (
        <span className="text-sm text-muted-foreground truncate" title={activeArtifact.title}>
          {activeArtifact.title}
        </span>
      )}
    </div>
  )
}

function renderContent(component: WindowComponent): { content: ReactNode; titleBarActions?: ReactNode; titleBarInfo?: ReactNode } {
  switch (component) {
    case 'settings':        return { content: <SettingsPanelContent /> }
    case 'tools':           return { content: <ToolsPanelContent /> }
    case 'image-editor':    return { content: <ImageEditorContent /> }
    case 'accounts':        return { content: <AccountsWindow />, titleBarActions: <AccountsTitleBarActions /> }
    case 'email':           return { content: <EmailWindow /> }
    case 'calendar':        return { content: <CalendarWindow />, titleBarActions: <CalendarTitleBarActions /> }
    case 'contacts':        return { content: <ContactsWindow /> }
    case 'graphify':        return { content: <GraphifyWindow /> }
    case 'artifact-viewer': return {
      content: <ArtifactPanel windowed />,
      titleBarActions: <ArtifactWindowCollapseAction />,
      titleBarInfo: <ArtifactWindowTitleInfo />,
    }
    default: {
      const _exhaustive: never = component
      void _exhaustive
      return { content: null }
    }
  }
}

export function WindowManager() {
  const { windows } = useWindowManager()
  const [snapEdge, setSnapEdge] = useState<'left' | 'right' | null>(null)
  const poppedOut = useArtifacts(s => s.poppedOut)
  const setActive = useArtifacts(s => s.setActive)

  // Snapped windows sit beside the chat rather than over it, so they don't scrim.
  const floating = Object.values(windows).filter(w => !w.snapState)
  const scrimZ = floating.length ? Math.min(...floating.map(w => w.zIndex)) - 1 : 0

  // Esc closes the top-most floating window. Snapped windows sit beside the chat and stay put.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return
      const target = e.target as HTMLElement | null
      if (target?.closest('input, textarea, select, [contenteditable="true"]')) return
      const open = Object.values(useWindowManager.getState().windows).filter(w => !w.snapState)
      if (!open.length) return
      const top = open.reduce((a, b) => (b.zIndex > a.zIndex ? b : a))
      e.preventDefault()
      useWindowManager.getState().closeWindow(top.id)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])

  // Closing the artifact window via its X (not the collapse button) closes the artifact entirely.
  useEffect(() => {
    if (poppedOut && !windows['artifact-viewer']) {
      useArtifacts.getState().setPoppedOut(false)
      setActive(null)
    }
  }, [poppedOut, windows])

  return (
    <div className="fixed inset-0 pointer-events-none" style={{ zIndex: 50 }}>
      {floating.length > 0 && (
        <div
          className="pointer-events-auto fixed inset-0 backdrop-blur-sm backdrop-saturate-[.6] backdrop-brightness-[.7] bg-black/10 transition-opacity duration-150"
          style={{ zIndex: scrimZ }}
        />
      )}
      {snapEdge && (
        <div
          className="pointer-events-none fixed inset-y-0 z-[999] bg-primary/10 border-2 border-[var(--primary)]/40 rounded-lg"
          style={{ [snapEdge]: 0, width: '50%' }}
        />
      )}
      {Object.values(windows).map(win => {
        const { content, titleBarActions, titleBarInfo } = renderContent(win.component)
        return (
          <WindowFrame
            key={win.id}
            window={win}
            titleBarActions={titleBarActions}
            titleBarInfo={titleBarInfo}
            onSnapCandidateChange={candidate => setSnapEdge(candidate ?? null)}
          >
            {content}
          </WindowFrame>
        )
      })}
    </div>
  )
}
