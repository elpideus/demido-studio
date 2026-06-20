import { useState, useCallback, useRef } from 'react'
import { useImageEditor, layerCanvases } from '../../stores/imageEditor'
import {
  adjustBrightness, adjustContrast, adjustHueSaturation,
  applyInvert, applyGrayscale, applyBlur, applySharpen,
  applyEmboss, applyFindEdges, applyMotionBlur, applyNoise,
  applyPosterize, applyThreshold, applyLevels, applyCurves,
  adjustVibrance, adjustShadowsHighlights,
} from './EditorCanvas'

function Slider({ label, min, max, step = 1, value, onChange, unit = '' }: {
  label: string; min: number; max: number; step?: number; value: number; onChange(v: number): void; unit?: string
}) {
  return (
    <label className="flex items-center gap-2 text-[11px]">
      <span className="text-zinc-400 w-20 shrink-0">{label}</span>
      <input type="range" min={min} max={max} step={step} value={value}
        onChange={e => onChange(Number(e.target.value))}
        className="flex-1 h-1 accent-blue-500" />
      <span className="text-zinc-300 w-10 text-right tabular-nums">{value}{unit}</span>
    </label>
  )
}

function PanelActions({ onApply, onCancel }: { onApply(): void; onCancel(): void }) {
  return (
    <div className="flex gap-2 mt-1">
      <button onClick={onApply} className="flex-1 py-1 bg-blue-600 hover:bg-blue-500 text-white rounded text-[11px]">Apply</button>
      <button onClick={onCancel} className="flex-1 py-1 bg-zinc-700 hover:bg-zinc-600 text-zinc-200 rounded text-[11px]">Cancel</button>
    </div>
  )
}

function useLayerPreview() {
  const { activeLayerId, pushHistory, requestRepaint } = useImageEditor()
  const originalRef = useRef<OffscreenCanvas | null>(null)

  const snapshot = useCallback(() => {
    const canvas = activeLayerId ? layerCanvases.get(activeLayerId) : null
    if (!canvas || originalRef.current) return canvas
    originalRef.current = new OffscreenCanvas(canvas.width, canvas.height)
    originalRef.current.getContext('2d')!.drawImage(canvas, 0, 0)
    return canvas
  }, [activeLayerId])

  const applyFn = useCallback((fn: (c: OffscreenCanvas) => void) => {
    const canvas = snapshot(); if (!canvas) return
    const tmp = new OffscreenCanvas(canvas.width, canvas.height)
    tmp.getContext('2d')!.drawImage(originalRef.current!, 0, 0)
    fn(tmp)
    canvas.getContext('2d')!.clearRect(0, 0, canvas.width, canvas.height)
    canvas.getContext('2d')!.drawImage(tmp, 0, 0)
    requestRepaint()
  }, [snapshot, requestRepaint])

  const apply = useCallback(() => { pushHistory(); originalRef.current = null }, [pushHistory])

  const cancel = useCallback(() => {
    const canvas = activeLayerId ? layerCanvases.get(activeLayerId) : null
    if (canvas && originalRef.current) {
      canvas.getContext('2d')!.clearRect(0, 0, canvas.width, canvas.height)
      canvas.getContext('2d')!.drawImage(originalRef.current, 0, 0)
      requestRepaint()
    }
    originalRef.current = null
  }, [activeLayerId, requestRepaint])

  return { applyFn, apply, cancel }
}

function BrightnessContrastPanel({ onClose }: { onClose(): void }) {
  const [brightness, setBrightness] = useState(0)
  const [contrast, setContrast] = useState(0)
  const { applyFn, apply, cancel } = useLayerPreview()
  return (
    <div className="p-3 flex flex-col gap-3">
      <Slider label="Brightness" min={-100} max={100} value={brightness} onChange={v => { setBrightness(v); applyFn(c => { if (v !== 0) adjustBrightness(c, v); if (contrast !== 0) adjustContrast(c, contrast) }) }} />
      <Slider label="Contrast" min={-100} max={100} value={contrast} onChange={v => { setContrast(v); applyFn(c => { if (brightness !== 0) adjustBrightness(c, brightness); if (v !== 0) adjustContrast(c, v) }) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

function HueSaturationPanel({ onClose }: { onClose(): void }) {
  const [hue, setHue] = useState(0)
  const [sat, setSat] = useState(100)
  const [light, setLight] = useState(0)
  const { applyFn, apply, cancel } = useLayerPreview()
  const preview = (h: number, s: number, l: number) => applyFn(c => adjustHueSaturation(c, h, s / 100, l / 100))
  return (
    <div className="p-3 flex flex-col gap-3">
      <Slider label="Hue" min={-180} max={180} value={hue} unit="°" onChange={v => { setHue(v); preview(v, sat, light) }} />
      <Slider label="Saturation" min={0} max={200} value={sat} unit="%" onChange={v => { setSat(v); preview(hue, v, light) }} />
      <Slider label="Lightness" min={-100} max={100} value={light} onChange={v => { setLight(v); preview(hue, sat, v) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

function VibrancePanel({ onClose }: { onClose(): void }) {
  const [vibrance, setVibrance] = useState(0)
  const { applyFn, apply, cancel } = useLayerPreview()
  return (
    <div className="p-3 flex flex-col gap-3">
      <Slider label="Vibrance" min={-100} max={100} value={vibrance} onChange={v => { setVibrance(v); applyFn(c => adjustVibrance(c, v)) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

function ShadowsHighlightsPanel({ onClose }: { onClose(): void }) {
  const [shadows, setShadows] = useState(0)
  const [highlights, setHighlights] = useState(0)
  const { applyFn, apply, cancel } = useLayerPreview()
  const preview = (s: number, h: number) => applyFn(c => adjustShadowsHighlights(c, s, h))
  return (
    <div className="p-3 flex flex-col gap-3">
      <Slider label="Shadows" min={-100} max={100} value={shadows} onChange={v => { setShadows(v); preview(v, highlights) }} />
      <Slider label="Highlights" min={-100} max={100} value={highlights} onChange={v => { setHighlights(v); preview(shadows, v) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

function LevelsPanel({ onClose }: { onClose(): void }) {
  const [inLow, setInLow] = useState(0)
  const [inHigh, setInHigh] = useState(255)
  const [gamma, setGamma] = useState(100)
  const [outLow, setOutLow] = useState(0)
  const [outHigh, setOutHigh] = useState(255)
  const { applyFn, apply, cancel } = useLayerPreview()
  const preview = (il: number, ih: number, g: number, ol: number, oh: number) =>
    applyFn(c => applyLevels(c, il, ih, g / 100, ol, oh))
  return (
    <div className="p-3 flex flex-col gap-3">
      <div className="text-[10px] text-zinc-500 uppercase tracking-wide">Input Levels</div>
      <Slider label="Black Point" min={0} max={253} value={inLow} onChange={v => { setInLow(Math.min(v, inHigh - 2)); preview(Math.min(v, inHigh - 2), inHigh, gamma, outLow, outHigh) }} />
      <Slider label="White Point" min={2} max={255} value={inHigh} onChange={v => { setInHigh(Math.max(v, inLow + 2)); preview(inLow, Math.max(v, inLow + 2), gamma, outLow, outHigh) }} />
      <Slider label="Gamma" min={10} max={300} value={gamma} unit="%" onChange={v => { setGamma(v); preview(inLow, inHigh, v, outLow, outHigh) }} />
      <div className="text-[10px] text-zinc-500 uppercase tracking-wide mt-1">Output Levels</div>
      <Slider label="Output Low" min={0} max={253} value={outLow} onChange={v => { setOutLow(Math.min(v, outHigh - 2)); preview(inLow, inHigh, gamma, Math.min(v, outHigh - 2), outHigh) }} />
      <Slider label="Output High" min={2} max={255} value={outHigh} onChange={v => { setOutHigh(Math.max(v, outLow + 2)); preview(inLow, inHigh, gamma, outLow, Math.max(v, outLow + 2)) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

// Simplified curves: 5 draggable points on a 128×128 canvas
function CurvesPanel({ onClose }: { onClose(): void }) {
  const [points, setPoints] = useState([
    { x: 0, y: 0 }, { x: 64, y: 64 }, { x: 128, y: 128 },
  ])
  const { applyFn, apply, cancel } = useLayerPreview()
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const dragging = useRef<number | null>(null)

  const drawCurve = useCallback((pts: typeof points) => {
    const c = canvasRef.current; if (!c) return
    const ctx = c.getContext('2d')!
    ctx.clearRect(0, 0, 128, 128)
    ctx.fillStyle = '#1a1a1a'; ctx.fillRect(0, 0, 128, 128)
    // Grid
    ctx.strokeStyle = '#333'; ctx.lineWidth = 0.5
    for (let i = 32; i < 128; i += 32) {
      ctx.beginPath(); ctx.moveTo(i, 0); ctx.lineTo(i, 128); ctx.stroke()
      ctx.beginPath(); ctx.moveTo(0, i); ctx.lineTo(128, i); ctx.stroke()
    }
    // Diagonal reference
    ctx.strokeStyle = '#444'; ctx.lineWidth = 1
    ctx.beginPath(); ctx.moveTo(0, 128); ctx.lineTo(128, 0); ctx.stroke()
    // Curve line
    const sorted = [...pts].sort((a, b) => a.x - b.x)
    ctx.strokeStyle = '#7eb6f6'; ctx.lineWidth = 1.5
    ctx.beginPath()
    for (let x = 0; x <= 128; x++) {
      let lo = sorted[0], hi = sorted[sorted.length - 1]
      for (let j = 0; j < sorted.length - 1; j++) {
        if (x >= sorted[j].x && x <= sorted[j + 1].x) { lo = sorted[j]; hi = sorted[j + 1]; break }
      }
      const t = lo.x === hi.x ? 0 : (x - lo.x) / (hi.x - lo.x)
      const y = 128 - (lo.y + t * (hi.y - lo.y))
      if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y)
    }
    ctx.stroke()
    // Control points
    sorted.forEach(p => {
      ctx.beginPath(); ctx.arc(p.x, 128 - p.y, 4, 0, Math.PI * 2)
      ctx.fillStyle = '#fff'; ctx.fill()
      ctx.strokeStyle = '#1a7de0'; ctx.lineWidth = 1.5; ctx.stroke()
    })
  }, [])

  const applyPoints = useCallback((pts: typeof points) => {
    drawCurve(pts)
    const scaled = pts.map(p => ({ x: Math.round(p.x * 2), y: Math.round(p.y * 2) }))
    applyFn(c => applyCurves(c, scaled))
  }, [drawCurve, applyFn])

  const onMouseDown = (e: React.MouseEvent) => {
    const c = canvasRef.current!
    const rect = c.getBoundingClientRect()
    const scale = 128 / rect.width
    const mx = (e.clientX - rect.left) * scale
    const my = (e.clientY - rect.top) * scale
    const cy = 128 - my
    // Check existing point
    const idx = points.findIndex(p => Math.hypot(p.x - mx, p.y - cy) < 8)
    if (idx >= 0) { dragging.current = idx; return }
    // Add new point
    const newPts = [...points, { x: mx, y: cy }].sort((a, b) => a.x - b.x)
    setPoints(newPts); applyPoints(newPts)
    dragging.current = newPts.findIndex(p => Math.abs(p.x - mx) < 1)
  }

  const onMouseMove = (e: React.MouseEvent) => {
    if (dragging.current === null) return
    const c = canvasRef.current!
    const rect = c.getBoundingClientRect()
    const scale = 128 / rect.width
    const mx = Math.max(0, Math.min(128, (e.clientX - rect.left) * scale))
    const cy = Math.max(0, Math.min(128, 128 - (e.clientY - rect.top) * scale))
    const newPts = [...points]
    newPts[dragging.current] = { x: mx, y: cy }
    setPoints(newPts); applyPoints(newPts)
  }

  const onMouseUp = () => { dragging.current = null }

  // Draw on mount
  useCallback(() => { drawCurve(points) }, [])

  return (
    <div className="p-3 flex flex-col gap-3">
      <canvas width={128} height={128} className="w-full rounded border border-zinc-700 cursor-crosshair"
        onMouseDown={onMouseDown} onMouseMove={onMouseMove} onMouseUp={onMouseUp}
        style={{ imageRendering: 'pixelated' }}
        ref={r => { if (r) { (canvasRef as React.MutableRefObject<HTMLCanvasElement>).current = r; drawCurve(points) } }} />
      <div className="text-[10px] text-zinc-500">Click to add points · Drag to adjust</div>
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

function GaussianBlurPanel({ onClose }: { onClose(): void }) {
  const [radius, setRadius] = useState(2)
  const { applyFn, apply, cancel } = useLayerPreview()
  return (
    <div className="p-3 flex flex-col gap-3">
      <Slider label="Radius" min={0} max={50} value={radius} unit="px" onChange={v => { setRadius(v); applyFn(c => applyBlur(c, v)) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

function MotionBlurPanel({ onClose }: { onClose(): void }) {
  const [angle, setAngle] = useState(0)
  const [distance, setDistance] = useState(10)
  const { applyFn, apply, cancel } = useLayerPreview()
  const preview = (a: number, d: number) => applyFn(c => applyMotionBlur(c, a, d))
  return (
    <div className="p-3 flex flex-col gap-3">
      <Slider label="Angle" min={0} max={360} value={angle} unit="°" onChange={v => { setAngle(v); preview(v, distance) }} />
      <Slider label="Distance" min={1} max={100} value={distance} unit="px" onChange={v => { setDistance(v); preview(angle, v) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

function PosterizePanel({ onClose }: { onClose(): void }) {
  const [levels, setLevels] = useState(4)
  const { applyFn, apply, cancel } = useLayerPreview()
  return (
    <div className="p-3 flex flex-col gap-3">
      <Slider label="Levels" min={2} max={32} value={levels} onChange={v => { setLevels(v); applyFn(c => applyPosterize(c, v)) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

function ThresholdPanel({ onClose }: { onClose(): void }) {
  const [value, setValue] = useState(128)
  const { applyFn, apply, cancel } = useLayerPreview()
  return (
    <div className="p-3 flex flex-col gap-3">
      <Slider label="Level" min={0} max={255} value={value} onChange={v => { setValue(v); applyFn(c => applyThreshold(c, v)) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

function NoisePanel({ onClose }: { onClose(): void }) {
  const [amount, setAmount] = useState(10)
  const { applyFn, apply, cancel } = useLayerPreview()
  return (
    <div className="p-3 flex flex-col gap-3">
      <Slider label="Amount" min={1} max={100} value={amount} unit="%" onChange={v => { setAmount(v); applyFn(c => applyNoise(c, v)) }} />
      <PanelActions onApply={() => { apply(); onClose() }} onCancel={() => { cancel(); onClose() }} />
    </div>
  )
}

type PanelId =
  | 'brightness' | 'hue' | 'vibrance' | 'shadowshighlights'
  | 'levels' | 'curves'
  | 'blur' | 'motionblur' | 'noise'
  | 'posterize' | 'threshold'
  | null

interface QuickAction { label: string; fn: (canvas: OffscreenCanvas) => void }

const QUICK_ACTIONS: QuickAction[] = [
  { label: 'Invert', fn: applyInvert },
  { label: 'Grayscale', fn: applyGrayscale },
  { label: 'Sharpen', fn: applySharpen },
  { label: 'Emboss', fn: applyEmboss },
  { label: 'Find Edges', fn: applyFindEdges },
  { label: 'Flip H', fn: c => {
    const tmp = new OffscreenCanvas(c.width, c.height)
    const ctx = tmp.getContext('2d')!
    ctx.save(); ctx.scale(-1, 1); ctx.drawImage(c, -c.width, 0); ctx.restore()
    c.getContext('2d')!.clearRect(0, 0, c.width, c.height)
    c.getContext('2d')!.drawImage(tmp, 0, 0)
  }},
  { label: 'Flip V', fn: c => {
    const tmp = new OffscreenCanvas(c.width, c.height)
    const ctx = tmp.getContext('2d')!
    ctx.save(); ctx.scale(1, -1); ctx.drawImage(c, 0, -c.height); ctx.restore()
    c.getContext('2d')!.clearRect(0, 0, c.width, c.height)
    c.getContext('2d')!.drawImage(tmp, 0, 0)
  }},
  { label: 'Rotate 90°', fn: c => {
    const tmp = new OffscreenCanvas(c.height, c.width)
    const ctx = tmp.getContext('2d')!
    ctx.save(); ctx.translate(c.height, 0); ctx.rotate(Math.PI / 2); ctx.drawImage(c, 0, 0); ctx.restore()
    const data = tmp.getContext('2d')!.getImageData(0, 0, tmp.width, tmp.height)
    c.getContext('2d')!.clearRect(0, 0, c.width, c.height)
    c.getContext('2d')!.putImageData(data, 0, 0)
  }},
]

const ADJUSTMENT_PANELS: { id: PanelId; label: string; group: string }[] = [
  { id: 'brightness',      label: 'Brightness / Contrast', group: 'Tone' },
  { id: 'levels',          label: 'Levels',                group: 'Tone' },
  { id: 'curves',          label: 'Curves',                group: 'Tone' },
  { id: 'shadowshighlights', label: 'Shadows / Highlights', group: 'Tone' },
  { id: 'hue',             label: 'Hue / Saturation',     group: 'Color' },
  { id: 'vibrance',        label: 'Vibrance',              group: 'Color' },
  { id: 'blur',            label: 'Gaussian Blur',         group: 'Blur' },
  { id: 'motionblur',      label: 'Motion Blur',           group: 'Blur' },
  { id: 'noise',           label: 'Add Noise',             group: 'Noise' },
  { id: 'posterize',       label: 'Posterize',             group: 'Other' },
  { id: 'threshold',       label: 'Threshold',             group: 'Other' },
]

export function AdjustmentsPanel({ onClose }: { onClose(): void }) {
  const [activePanel, setActivePanel] = useState<PanelId>(null)
  const { activeLayerId, pushHistory, requestRepaint } = useImageEditor()

  const runQuick = (action: QuickAction) => {
    const canvas = activeLayerId ? layerCanvases.get(activeLayerId) : null; if (!canvas) return
    pushHistory()
    action.fn(canvas)
    requestRepaint()
  }

  const groups = [...new Set(ADJUSTMENT_PANELS.map(p => p.group))]

  return (
    <div className="w-56 border-l border-zinc-800 bg-zinc-900 flex flex-col shrink-0 overflow-y-auto">
      <div className="flex items-center justify-between px-3 py-2 border-b border-zinc-800 shrink-0">
        <span className="text-[10px] font-semibold text-zinc-300 uppercase tracking-wide">Adjustments</span>
        <button onClick={onClose} className="text-zinc-500 hover:text-zinc-300 text-[12px]">✕</button>
      </div>

      {groups.map(group => (
        <div key={group} className="border-b border-zinc-800">
          <div className="px-3 py-1 text-[9px] text-zinc-600 uppercase tracking-widest">{group}</div>
          {ADJUSTMENT_PANELS.filter(p => p.group === group).map(ap => (
            <div key={ap.id}>
              <button
                onClick={() => setActivePanel(activePanel === ap.id ? null : ap.id)}
                className={`w-full flex items-center justify-between px-3 py-1.5 text-[11px] hover:bg-zinc-800 transition-colors ${activePanel === ap.id ? 'text-blue-400' : 'text-zinc-300'}`}
              >
                <span>{ap.label}</span>
                <span className="text-zinc-600 text-[9px]">{activePanel === ap.id ? '▲' : '▼'}</span>
              </button>
              {activePanel === ap.id && (
                <div className="bg-zinc-800/50 border-t border-zinc-800">
                  {ap.id === 'brightness'      && <BrightnessContrastPanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'levels'          && <LevelsPanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'curves'          && <CurvesPanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'shadowshighlights' && <ShadowsHighlightsPanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'hue'             && <HueSaturationPanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'vibrance'        && <VibrancePanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'blur'            && <GaussianBlurPanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'motionblur'      && <MotionBlurPanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'noise'           && <NoisePanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'posterize'       && <PosterizePanel onClose={() => setActivePanel(null)} />}
                  {ap.id === 'threshold'       && <ThresholdPanel onClose={() => setActivePanel(null)} />}
                </div>
              )}
            </div>
          ))}
        </div>
      ))}

      <div className="border-b border-zinc-800">
        <div className="px-3 py-1 text-[9px] text-zinc-600 uppercase tracking-widest">Quick Actions</div>
        <div className="px-3 pb-2 grid grid-cols-3 gap-1">
          {QUICK_ACTIONS.map(a => (
            <button key={a.label} onClick={() => runQuick(a)}
              className="px-1 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded text-[10px] transition-colors">
              {a.label}
            </button>
          ))}
        </div>
      </div>

      <div className="px-3 py-2 text-[10px] text-zinc-600">
        Edits apply to active layer. Ctrl+Z to undo.
      </div>
    </div>
  )
}
