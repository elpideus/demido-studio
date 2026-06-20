import { useState } from 'react'
import { WindowFrame } from './WindowFrame'
import { SettingsPanelContent } from '../settings/SettingsPanelContent'
import { ToolsPanelContent } from '../tools/ToolsPanelContent'
import { ImageEditorContent } from '../image-editor/ImageEditorContent'
import { useWindowManager } from '../../stores/windowManager'
import type { WindowComponent } from '../../types'

function renderContent(component: WindowComponent) {
  switch (component) {
    case 'settings':     return <SettingsPanelContent />
    case 'tools':        return <ToolsPanelContent />
    case 'image-editor': return <ImageEditorContent />
    default: {
      const _exhaustive: never = component
      void _exhaustive
      return null
    }
  }
}

export function WindowManager() {
  const { windows } = useWindowManager()
  const [snapEdge, setSnapEdge] = useState<'left' | 'right' | null>(null)

  return (
    <div className="fixed inset-0 pointer-events-none" style={{ zIndex: 50 }}>
      {snapEdge && (
        <div
          className="pointer-events-none fixed inset-y-0 z-[999] bg-primary/10 border-2 border-[var(--primary)]/40 rounded-lg"
          style={{ [snapEdge]: 0, width: '50%' }}
        />
      )}
      {Object.values(windows).map(win => (
        <WindowFrame
          key={win.id}
          window={win}
          onSnapCandidateChange={candidate => setSnapEdge(candidate ?? null)}
        >
          {renderContent(win.component)}
        </WindowFrame>
      ))}
    </div>
  )
}
