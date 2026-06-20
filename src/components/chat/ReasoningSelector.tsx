import { useState, useRef, useEffect } from 'react'
import { Brain, ChevronUp } from 'lucide-react'
import { cn } from '../../lib/utils'

interface Props {
  options: string[]
  value: string
  onChange: (v: string) => void
}

export function ReasoningSelector({ options, value, onChange }: Props) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

  const active = value !== 'off'
  // "off" always last, rest in original order
  const nonOff = options.filter(o => o !== 'off')
  const hasOff = options.includes('off')

  const label = value === 'off' ? 'Think' : value.charAt(0).toUpperCase() + value.slice(1)

  return (
    <div ref={ref} className="relative shrink-0">
      <button
        onClick={() => setOpen(o => !o)}
        className={cn(
          'flex items-center gap-1.5 px-2.5 h-7 rounded-lg text-xs font-medium transition-colors border',
          active
            ? 'bg-primary/20 border-[var(--primary)]/60 text-primary'
            : 'bg-accent border-transparent text-muted-foreground hover:border-[var(--accent)]'
        )}
        title="Select reasoning mode"
      >
        <Brain size={12} />
        {label}
        <ChevronUp size={10} className={cn('transition-transform', open ? 'rotate-180' : '')} />
      </button>

      {open && (
        <div className="absolute bottom-full mb-1.5 left-0 bg-secondary border border-border rounded-lg shadow-xl overflow-hidden min-w-[110px] z-50">
          {nonOff.map(opt => (
            <button
              key={opt}
              onClick={() => { onChange(opt); setOpen(false) }}
              className={cn(
                'w-full text-left px-3 py-1.5 text-xs transition-colors',
                opt === value
                  ? 'text-primary bg-primary/10'
                  : 'text-foreground hover:bg-accent'
              )}
            >
              {opt === value && '✓ '}{opt.charAt(0).toUpperCase() + opt.slice(1)}
            </button>
          ))}
          {hasOff && nonOff.length > 0 && (
            <div className="h-px bg-accent mx-2 my-0.5" />
          )}
          {hasOff && (
            <button
              onClick={() => { onChange('off'); setOpen(false) }}
              className={cn(
                'w-full text-left px-3 py-1.5 text-xs transition-colors',
                value === 'off'
                  ? 'text-primary bg-primary/10'
                  : 'text-muted-foreground hover:bg-accent'
              )}
            >
              {value === 'off' && '✓ '}Off
            </button>
          )}
        </div>
      )}
    </div>
  )
}
