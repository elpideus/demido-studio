import { useCallback, useRef, useState } from 'react'
import { Toolbar } from './Toolbar'
import { EditorCanvas } from './EditorCanvas'
import { LayerPanel } from './LayerPanel'
import { ToolOptionsBar } from './ToolOptionsBar'
import { AdjustmentsPanel } from './AdjustmentsPanel'
import { useImageEditor, layerCanvases } from '../../stores/imageEditor'
import { SlidersHorizontal } from 'lucide-react'

function exportCanvas() {
  const { layers, canvasWidth, canvasHeight, fileName } = useImageEditor.getState()
  const tmp = document.createElement('canvas'); tmp.width = canvasWidth; tmp.height = canvasHeight
  const ctx = tmp.getContext('2d')!
  for (const layer of layers) {
    if (!layer.visible) continue
    const c = layerCanvases.get(layer.id); if (!c) continue
    ctx.save(); ctx.globalAlpha = layer.opacity / 100; ctx.globalCompositeOperation = layer.blendMode as GlobalCompositeOperation
    ctx.drawImage(c, 0, 0); ctx.restore()
  }
  const link = document.createElement('a')
  link.download = fileName.replace(/\.[^.]+$/, '') + '_edited.png'
  link.href = tmp.toDataURL('image/png'); link.click()
}

const MIN_LAYER_PANEL = 160
const MAX_LAYER_PANEL = 400
const DEFAULT_LAYER_PANEL = 220

export function ImageEditorContent() {
  const { fileName } = useImageEditor()
  const [layerPanelWidth, setLayerPanelWidth] = useState(DEFAULT_LAYER_PANEL)
  const [showAdjustments, setShowAdjustments] = useState(false)
  const resizingRef = useRef(false)
  const startXRef = useRef(0)
  const startWRef = useRef(DEFAULT_LAYER_PANEL)

  const handleResizeStart = useCallback((e: React.PointerEvent) => {
    e.preventDefault()
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    resizingRef.current = true
    startXRef.current = e.clientX
    startWRef.current = layerPanelWidth

    const onMove = (ev: PointerEvent) => {
      if (!resizingRef.current) return
      const delta = startXRef.current - ev.clientX
      setLayerPanelWidth(Math.max(MIN_LAYER_PANEL, Math.min(MAX_LAYER_PANEL, startWRef.current + delta)))
    }
    const onUp = () => { resizingRef.current = false; window.removeEventListener('pointermove', onMove); window.removeEventListener('pointerup', onUp) }
    window.addEventListener('pointermove', onMove); window.addEventListener('pointerup', onUp)
  }, [layerPanelWidth])

  return (
    <div className="flex flex-col h-full bg-zinc-950">
      {/* Top bar */}
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-zinc-800 bg-zinc-950 shrink-0">
        <div className="flex items-center gap-2">
          <span className="text-[10px] font-bold px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-400 border border-amber-500/30 tracking-wider">WIP</span>
          <a
            href="https://github.com/demidostudio/demido-studio/issues"
            target="_blank"
            rel="noopener noreferrer"
            className="text-[10px] px-1.5 py-0.5 rounded bg-blue-500/15 text-blue-400 border border-blue-500/25 hover:bg-blue-500/25 transition-colors cursor-pointer"
            title="Help develop this feature"
          >🛠 Help needed</a>
          <span className="text-[11px] text-zinc-500 truncate max-w-[200px] font-mono">{fileName}</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowAdjustments(v => !v)}
            className={`flex items-center gap-1.5 text-[11px] px-2 py-0.5 rounded transition-colors ${showAdjustments ? 'bg-blue-600/20 text-blue-400' : 'text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800'}`}
            title="Adjustments & Filters"
          >
            <SlidersHorizontal size={12} />
            <span>Adjustments</span>
          </button>
          <button
            onClick={exportCanvas}
            className="text-[11px] px-2.5 py-0.5 rounded bg-blue-600 text-white hover:bg-blue-500 transition-colors"
          >
            Export PNG
          </button>
        </div>
      </div>

      {/* Tool options bar */}
      <ToolOptionsBar />

      {/* Main area */}
      <div className="flex flex-1 min-h-0">
        <Toolbar />
        <EditorCanvas />
        {showAdjustments && <AdjustmentsPanel onClose={() => setShowAdjustments(false)} />}
        <LayerPanel width={layerPanelWidth} onResizeStart={handleResizeStart} />
      </div>
    </div>
  )
}
