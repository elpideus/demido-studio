import { CAVEMAN_LEVELS, CAVEMAN_GROUPS, cavemanMeta, cavemanButtonLabel } from '../../lib/caveman'
import type { CavemanLevel } from '../../types'
import { Button } from '@/components/ui/button'
import { useDropdown } from './useDropdown'

export function CavemanSelector({
  value,
  onChange,
  align = 'right',
}: {
  value: CavemanLevel
  onChange: (level: CavemanLevel) => void
  align?: 'left' | 'right'
}) {
  const current = cavemanMeta(value)
  const { open, setOpen, containerRef } = useDropdown()

  function handleSelect(level: CavemanLevel) {
    onChange(level)
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
        aria-label="Caveman level"
        title={`Response style — ${current.hint}`}
        className="gap-1.5 bg-[#1b1b1b]"
      >
        <span>{current.icon}</span>
        <span>{cavemanButtonLabel(value)}</span>
        <svg className="w-3 h-3 opacity-60" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5">
          <path d="M3 5l3 3 3-3" />
        </svg>
      </Button>

      {open && (
        <div className={`absolute ${align === 'right' ? 'right-0' : 'left-0'} top-full mt-1 z-50 min-w-[250px] py-1 rounded-lg border border-border bg-popover shadow-xl`}>
          {CAVEMAN_GROUPS.map(groupName => (
            <div key={groupName}>
              <p className="px-3 pt-1.5 pb-1 text-[10px] uppercase tracking-wide text-muted-foreground/60">{groupName}</p>
              {CAVEMAN_LEVELS.filter(l => l.group === groupName).map(l => (
                <button
                  key={l.value}
                  onClick={() => handleSelect(l.value)}
                  className={`flex items-center gap-2 w-full px-3 py-1.5 text-sm text-left hover:bg-accent transition-colors ${value === l.value ? 'text-foreground font-medium' : 'text-muted-foreground'}`}
                >
                  <span className="shrink-0">{l.icon}</span>
                  <span className="shrink-0">{l.label}</span>
                  <span className="text-xs opacity-60 truncate">{l.hint}</span>
                  {value === l.value && (
                    <svg className="ml-auto w-3 h-3 shrink-0" viewBox="0 0 12 12" fill="none">
                      <path d="M2 6l3 3 5-5" stroke="currentColor" strokeWidth="1.5" />
                    </svg>
                  )}
                </button>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
