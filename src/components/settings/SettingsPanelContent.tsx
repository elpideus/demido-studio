import { useState } from 'react'
import { ProvidersSettings } from './ProvidersSettings'
import { ResetSettings } from './ResetSettings'
import { EngineSettings, type EngineSub } from './EngineSettings'
import { InterfaceSettings } from './InterfaceSettings'
import { InfoSettings } from './InfoSettings'
import { SystemPromptSettings } from './SystemPromptSettings'

type Tab = 'providers' | 'engine' | 'interface' | 'system-prompt' | 'info' | 'reset'

const TABS: { id: Tab; label: string }[] = [
  { id: 'providers',     label: 'Providers & Models' },
  { id: 'engine',        label: 'Engine' },
  { id: 'interface',     label: 'Interface' },
  { id: 'system-prompt', label: 'System Prompt' },
  { id: 'info',          label: 'Info' },
]

export function SettingsPanelContent() {
  const [tab, setTab] = useState<Tab>('providers')
  const [engineSub, setEngineSub] = useState<EngineSub>('runtime')

  return (
    <div className="flex flex-1 overflow-hidden h-full">
      {/* Tab rail: nav on top, the destructive tab pinned to the bottom and away from it. */}
      <div className="w-44 border-r border-border p-3 shrink-0 bg-[#1b1b1b] flex flex-col">
        <div className="space-y-0.5 flex-1">
        {TABS.map(t => (
          <button
            key={t.id}
            onClick={() => {
              if (t.id === 'engine') setEngineSub('runtime')
              setTab(t.id)
            }}
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
        <button
          onClick={() => setTab('reset')}
          className={`w-full text-left px-3 py-2 rounded-md text-sm transition-colors ${
            tab === 'reset'
              ? 'bg-red-500/15 text-red-400'
              : 'text-muted-foreground hover:bg-red-500/10 hover:text-red-400'
          }`}
        >
          Reset Data
        </button>
      </div>
      <div className="flex-1 overflow-y-auto p-6">
        {tab === 'providers'     && (
          <ProvidersSettings onDownloadModels={() => { setEngineSub('models'); setTab('engine') }} />
        )}
        {tab === 'engine'        && <EngineSettings initialSub={engineSub} />}
        {tab === 'interface'     && <InterfaceSettings />}
        {tab === 'info'          && <InfoSettings />}
        {tab === 'reset'         && <ResetSettings />}
        {tab === 'system-prompt' && <SystemPromptSettings />}
      </div>
    </div>
  )
}
