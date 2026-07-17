import { open } from '@tauri-apps/plugin-dialog'
import { X } from 'lucide-react'
import { Button } from '@/components/ui/button'

export function WorkingFolderButton({
  value,
  onChange,
}: {
  value: string | null
  onChange: (path: string | null) => void
}) {
  const folderName = value
    ? (value.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? value)
    : 'Set folder…'

  async function handlePick() {
    const result = await open({ directory: true, multiple: false })
    if (typeof result === 'string') onChange(result)
  }

  return (
    <div className="relative group">
      <Button
        onClick={handlePick}
        title={value ?? 'Pick working directory'}
        variant="ghost"
        size="sm"
        className="gap-1.5 max-w-[180px] text-muted-foreground bg-[#1b1b1b]"
      >
        <svg className="w-3.5 h-3.5 shrink-0" viewBox="0 0 16 16" fill="currentColor">
          <path d="M1.5 3A1.5 1.5 0 000 4.5v8A1.5 1.5 0 001.5 14h13a1.5 1.5 0 001.5-1.5V6a1.5 1.5 0 00-1.5-1.5H7.621a1.5 1.5 0 01-1.06-.44L5.5 3H1.5z" />
        </svg>
        {/* On hover the name stays readable but dissolves as it runs under the clear button —
            a gradient mask, so the fade is positional rather than the whole label dimming. */}
        <span
          className={`truncate ${value ? 'group-hover:[mask-image:linear-gradient(to_right,#000_50%,transparent_92%)]' : ''}`}
        >
          {folderName}
        </span>
      </Button>

      {/* Clear sits over the name's right edge rather than beside it: the button is already at its
          width cap, and a folder is only clearable once one is picked. Sibling, not child —
          nesting a button inside the picker button is invalid markup and swallows the click. */}
      {value && (
        <button
          onClick={() => onChange(null)}
          title="Clear working directory"
          aria-label="Clear working directory"
          className="absolute inset-y-0 right-0 flex items-center px-2 rounded-r-md text-red-500 opacity-0 group-hover:opacity-100 hover:text-red-400 focus-visible:opacity-100 transition-opacity"
        >
          <X size={13} />
        </button>
      )}
    </div>
  )
}
