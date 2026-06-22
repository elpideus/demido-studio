import { getCurrentWindow } from '@tauri-apps/api/window'
import { Minus, Square, X } from 'lucide-react'

const win = getCurrentWindow()

export function WindowControls() {
  return (
    <div className="fixed top-2 right-2 z-[9999] flex items-center gap-1">
      <button
        onClick={() => win.minimize()}
        className="w-8 h-8 flex items-center justify-center rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
      >
        <Minus size={14} />
      </button>
      <button
        onClick={() => win.toggleMaximize()}
        className="w-8 h-8 flex items-center justify-center rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
      >
        <Square size={12} />
      </button>
      <button
        onClick={() => win.close()}
        className="w-8 h-8 flex items-center justify-center rounded-lg text-muted-foreground hover:bg-red-500/20 hover:text-red-400 transition-colors"
      >
        <X size={14} />
      </button>
    </div>
  )
}
