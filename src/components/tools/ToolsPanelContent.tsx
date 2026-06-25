import { useState, useEffect } from 'react'
import { McpSettings } from '../settings/McpSettings'
import { useSkills } from '../../stores/skills'
import { Trash2 } from 'lucide-react'

type Tab = 'mcp' | 'skills'

const TABS: { id: Tab; label: string }[] = [
  { id: 'mcp',    label: 'MCP Servers' },
  { id: 'skills', label: 'Skills' },
]

function SkillsList() {
  const { skills, load, delete: deleteSkill } = useSkills()
  const [confirmId, setConfirmId] = useState<string | null>(null)
  useEffect(() => { load() }, [])

  if (!skills.length) return <p className="text-sm text-muted-foreground">No skills installed.</p>

  return (
    <>
      <div className="space-y-2">
        {skills.map(s => (
          <div key={s.id} className="p-3 rounded-lg border border-border flex items-start gap-2">
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium text-foreground">{s.name}</div>
              {s.description && <div className="text-xs text-muted-foreground mt-0.5">{s.description}</div>}
              {s.version && <div className="text-xs text-muted-foreground/60 mt-0.5">v{s.version}</div>}
            </div>
            <button
              onClick={() => setConfirmId(s.id)}
              className="shrink-0 p-1.5 rounded text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors"
              title="Delete skill"
            >
              <Trash2 size={14} />
            </button>
          </div>
        ))}
      </div>

      {confirmId && (() => {
        const skill = skills.find(s => s.id === confirmId)
        return (
          <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
            <div className="bg-background border border-border rounded-xl p-6 w-80 shadow-xl space-y-4">
              <div className="text-sm font-semibold text-foreground">Delete skill?</div>
              <div className="text-sm text-muted-foreground">
                <span className="font-medium text-foreground">{skill?.name}</span> will be permanently removed.
              </div>
              <div className="flex gap-2 justify-end">
                <button
                  onClick={() => setConfirmId(null)}
                  className="px-3 py-1.5 text-sm rounded-md border border-border hover:bg-accent transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={async () => { await deleteSkill(confirmId); setConfirmId(null) }}
                  className="px-3 py-1.5 text-sm rounded-md bg-destructive text-destructive-foreground hover:bg-destructive/90 transition-colors"
                >
                  Delete
                </button>
              </div>
            </div>
          </div>
        )
      })()}
    </>
  )
}

export function ToolsPanelContent() {
  const [tab, setTab] = useState<Tab>('mcp')

  return (
    <div className="flex flex-1 overflow-hidden h-full">
      <div className="w-44 border-r border-border p-3 space-y-0.5 shrink-0 bg-[#1b1b1b]">
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
        {tab === 'mcp' && <McpSettings />}
        {tab === 'skills' && (
          <div className="space-y-3">
            <h3 className="text-sm font-semibold text-foreground">Skills</h3>
            <SkillsList />
          </div>
        )}
      </div>
    </div>
  )
}
