import { useState } from 'react'
import type { ReactNode } from 'react'
import { WindowFrame } from './WindowFrame'
import { SettingsPanelContent } from '../settings/SettingsPanelContent'
import { ToolsPanelContent } from '../tools/ToolsPanelContent'
import { ImageEditorContent } from '../image-editor/ImageEditorContent'
import { AccountsWindow, AccountsTitleBarActions } from './AccountsWindow'
import { CalendarWindow, CalendarTitleBarActions } from './CalendarWindow'
import { EmailWindow } from './EmailWindow'
import { ContactsWindow } from './ContactsWindow'
import { useWindowManager } from '../../stores/windowManager'
import type { WindowComponent } from '../../types'

function renderContent(component: WindowComponent): { content: ReactNode; titleBarActions?: ReactNode } {
  switch (component) {
    case 'settings':     return { content: <SettingsPanelContent /> }
    case 'tools':        return { content: <ToolsPanelContent /> }
    case 'image-editor': return { content: <ImageEditorContent /> }
    case 'accounts':     return { content: <AccountsWindow />, titleBarActions: <AccountsTitleBarActions /> }
    case 'email':        return { content: <EmailWindow /> }
    case 'calendar':     return { content: <CalendarWindow />, titleBarActions: <CalendarTitleBarActions /> }
    case 'contacts':     return { content: <ContactsWindow /> }
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

  return (
    <div className="fixed inset-0 pointer-events-none" style={{ zIndex: 50 }}>
      {snapEdge && (
        <div
          className="pointer-events-none fixed inset-y-0 z-[999] bg-primary/10 border-2 border-[var(--primary)]/40 rounded-lg"
          style={{ [snapEdge]: 0, width: '50%' }}
        />
      )}
      {Object.values(windows).map(win => {
        const { content, titleBarActions } = renderContent(win.component)
        return (
          <WindowFrame
            key={win.id}
            window={win}
            titleBarActions={titleBarActions}
            onSnapCandidateChange={candidate => setSnapEdge(candidate ?? null)}
          >
            {content}
          </WindowFrame>
        )
      })}
    </div>
  )
}
