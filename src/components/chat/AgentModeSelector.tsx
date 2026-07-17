import { Button } from '@/components/ui/button'
import { useDropdown } from './useDropdown'

export type AgentMode = 'off' | 'cautious' | 'balanced' | 'autonomous'

export const AGENT_MODES: { value: AgentMode; label: string; icon: string }[] = [
  { value: 'off',        label: 'Off',        icon: '○'  },
  { value: 'cautious',   label: 'Cautious',   icon: '🔐' },
  { value: 'balanced',   label: 'Balanced',   icon: '⚡' },
  { value: 'autonomous', label: 'Autonomous', icon: '🤖' },
]

export function AgentModeSelector({
  value,
  onChange,
  align = 'right',
}: {
  value: AgentMode
  onChange: (mode: AgentMode) => void
  align?: 'left' | 'right'
}) {
  const current = AGENT_MODES.find(m => m.value === value) ?? AGENT_MODES[0]
  const { open, setOpen, containerRef } = useDropdown()

  function handleSelect(mode: AgentMode) {
    onChange(mode)
    setOpen(false)
  }

  return (
    <div className="relative" ref={containerRef}>
      <Button
        onClick={() => setOpen(o => !o)}
        variant="ghost"
        size="sm"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label="Agent mode"
        className="gap-1.5 bg-[#1b1b1b]"
      >
        <span>{current.icon}</span>
        <span>{value === 'off' ? 'Agent' : `${current.label} Agent`}</span>
        <svg className="w-3 h-3 opacity-60" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5">
          <path d="M3 5l3 3 3-3" />
        </svg>
      </Button>

      {open && (
        <div className={`absolute ${align === 'right' ? 'right-0' : 'left-0'} top-full mt-1 z-50 min-w-[140px] rounded-lg border border-border bg-popover shadow-xl`}>
          {AGENT_MODES.map(m => (
            <button
              key={m.value}
              onClick={() => handleSelect(m.value)}
              className={`flex items-center gap-2 w-full px-3 py-2 text-sm text-left hover:bg-accent transition-colors first:rounded-t-lg last:rounded-b-lg ${value === m.value ? 'text-foreground font-medium' : 'text-muted-foreground'}`}
            >
              <span>{m.icon}</span>
              <span>{m.label}</span>
              {value === m.value && (
                <svg className="ml-auto w-3 h-3" viewBox="0 0 12 12" fill="currentColor">
                  <path d="M2 6l3 3 5-5" stroke="currentColor" strokeWidth="1.5" fill="none" />
                </svg>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
