import { useState, useRef, useEffect } from 'react'
import { Wrench } from 'lucide-react'
import { useMcpTools } from '../../stores/mcpTools'
import { useSkills } from '../../stores/skills'
import { ToolSelectorPopup } from './ToolSelectorPopup'

export function ToolSelector() {
  const [open, setOpen] = useState(false)
  const enabledTools = useMcpTools(s => s.enabledTools)
  const skills = useSkills(s => s.skills)
  const activeCount = enabledTools().length + skills.filter(s => s.enabled).length
  const wrapperRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (wrapperRef.current && !wrapperRef.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [])

  return (
    <div ref={wrapperRef} className="relative shrink-0">
      <button
        onClick={() => setOpen(o => !o)}
        className={`relative w-7 h-7 flex items-center justify-center rounded-lg transition-colors ${
          open
            ? 'bg-primary/20 border border-[var(--primary)]/60'
            : activeCount > 0
              ? 'bg-primary/10 border border-[var(--primary)]/30 hover:border-[var(--primary)]/60'
              : 'bg-accent border border-transparent hover:border-border'
        }`}
        title="Tools"
      >
        <Wrench size={13} className={activeCount > 0 ? 'text-primary' : 'text-muted-foreground'} />
        {activeCount > 0 && (
          <span className="absolute -top-1.5 -right-1.5 w-3.5 h-3.5 bg-primary rounded-full text-[8px] text-white flex items-center justify-center leading-none">
            {activeCount}
          </span>
        )}
      </button>

      {open && <ToolSelectorPopup />}
    </div>
  )
}
