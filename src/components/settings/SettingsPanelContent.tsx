import { useState } from 'react'
import { ProvidersSettings } from './ProvidersSettings'
import { InterfaceSettings } from './InterfaceSettings'
import { InfoSettings } from './InfoSettings'
import { useSettings } from '../../stores/settings'

type Tab = 'providers' | 'interface' | 'system-prompt' | 'info'

const TABS: { id: Tab; label: string }[] = [
  { id: 'providers',     label: 'Providers & Models' },
  { id: 'interface',     label: 'Interface' },
  { id: 'system-prompt', label: 'System Prompt' },
  { id: 'info',          label: 'Info' },
]

export function SettingsPanelContent() {
  const [tab, setTab] = useState<Tab>('providers')
  const { settings, update } = useSettings()

  return (
    <div className="flex flex-1 overflow-hidden h-full">
      <div className="w-44 border-r border-border p-3 space-y-0.5 shrink-0">
        {TABS.map(t => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`w-full text-left px-3 py-2 rounded-md text-sm transition-colors ${
              tab === t.id
                ? 'bg-primary/20 text-foreground'
                : 'text-muted-foreground hover:bg-accent hover:text-foreground'
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto p-6">
        {tab === 'providers'     && <ProvidersSettings />}
        {tab === 'interface'     && <InterfaceSettings />}
        {tab === 'info'          && <InfoSettings />}
        {tab === 'system-prompt' && (
          <div className="space-y-4">
            <h3 className="text-sm font-semibold text-foreground">Global System Prompt</h3>
            <p className="text-xs text-muted-foreground">This prompt is prepended to every conversation.</p>
            <textarea
              value={settings.system_prompt}
              onChange={e => update('system_prompt', e.target.value)}
              placeholder="You are a helpful assistant..."
              rows={12}
              className="w-full bg-background border border-border rounded-xl px-4 py-3 text-sm text-foreground placeholder:text-muted-foreground outline-none focus:border-ring/50 resize-none leading-relaxed"
            />
          </div>
        )}
      </div>
    </div>
  )
}
