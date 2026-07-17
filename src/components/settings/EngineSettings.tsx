import { useState } from 'react'
import { Cpu, Boxes, Terminal } from 'lucide-react'
import { RuntimeTab } from './engine/RuntimeTab'
import { ModelsTab } from './engine/ModelsTab'
import { PythonTab } from './engine/PythonTab'

export type EngineSub = 'runtime' | 'models' | 'python'
type Sub = EngineSub

const SUBS: { id: Sub; label: string; icon: typeof Cpu }[] = [
  { id: 'runtime', label: 'Runtime', icon: Cpu },
  { id: 'models',  label: 'Models',  icon: Boxes },
  { id: 'python',  label: 'Python',  icon: Terminal },
  // Future: { id: 'suggestions', label: 'For your hardware', icon: Sparkles }
]

export function EngineSettings({ initialSub = 'runtime' }: { initialSub?: EngineSub }) {
  const [sub, setSub] = useState<Sub>(initialSub)

  return (
    <div className="flex flex-col h-full -m-6">
      {/* Sub-tab bar */}
      <div className="flex items-center gap-1 border-b border-border px-4 pt-3 shrink-0">
        {SUBS.map(s => {
          const Icon = s.icon
          const active = sub === s.id
          return (
            <button
              key={s.id}
              onClick={() => setSub(s.id)}
              className={`flex items-center gap-1.5 px-3 py-2 text-sm rounded-t-md border-b-2 -mb-px transition-colors ${
                active
                  ? 'border-primary text-foreground'
                  : 'border-transparent text-muted-foreground hover:text-foreground'
              }`}
            >
              <Icon size={14} />
              {s.label}
            </button>
          )
        })}
      </div>

      <div className="flex-1 min-h-0">
        {sub === 'runtime' && <RuntimeTab />}
        {sub === 'models' && <ModelsTab />}
        {sub === 'python' && <PythonTab />}
      </div>
    </div>
  )
}
