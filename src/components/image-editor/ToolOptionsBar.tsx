import { useImageEditor, CROP_RATIOS } from '../../stores/imageEditor'
import type { CropRatio, ShapeType } from '../../stores/imageEditor'

function ColorInput({ color, onChange }: { color: string; onChange(c: string): void }) {
  const valid = (v: string) => /^#[0-9a-fA-F]{6}$/.test(v)
  return (
    <div className="flex items-center gap-1 shrink-0">
      <div
        className="w-5 h-5 rounded border border-border/50 cursor-pointer hover:scale-105 transition-transform shrink-0"
        style={{ background: color }}
        onClick={() => {
          const inp = document.createElement('input'); inp.type = 'color'; inp.value = color
          inp.onchange = () => onChange(inp.value); inp.click()
        }}
      />
      <input
        type="text"
        defaultValue={color}
        key={color}
        onBlur={e => { if (valid(e.target.value)) onChange(e.target.value) }}
        onKeyDown={e => { if (e.key === 'Enter' && valid((e.target as HTMLInputElement).value)) onChange((e.target as HTMLInputElement).value) }}
        className="w-[68px] bg-background border border-border/40 rounded px-1.5 py-0.5 text-[10px] font-mono text-foreground/70 focus:outline-none focus:border-primary/50"
        maxLength={7}
      />
    </div>
  )
}

const SHAPE_TYPES: { value: ShapeType; label: string }[] = [
  { value: 'rect', label: 'Rect' },
  { value: 'ellipse', label: 'Ellipse' },
  { value: 'triangle', label: 'Triangle' },
  { value: 'polygon', label: 'Polygon' },
]

export function ToolOptionsBar() {
  const {
    activeTool, brushSize, brushHardness, brushOpacity,
    setBrushSize, setBrushHardness, setBrushOpacity,
    foregroundColor, backgroundColor, setForegroundColor, setBackgroundColor,
    shapeType, setShapeType, polygonSides, setPolygonSides,
    cropRatio, setCropRatio,
    gradientType, setGradientType,
    burnDodgeStrength, setBurnDodgeStrength,
    zoom, setZoom, setPan, canvasWidth, canvasHeight, selection,
  } = useImageEditor()

  const isBrushLike = activeTool === 'brush' || activeTool === 'eraser' || activeTool === 'clone'
  const isShapeTool = activeTool === 'shape'
  const isCrop = activeTool === 'crop'
  const isPen = activeTool === 'pen'
  const isText = activeTool === 'text'

  const fitToView = () => {
    const el = document.querySelector('[data-editor-canvas]') as HTMLElement | null
    if (!el) return
    const { clientWidth: cw, clientHeight: ch } = el
    const z = Math.min((cw - 40) / canvasWidth, (ch - 40) / canvasHeight, 1)
    setZoom(z); setPan((cw - canvasWidth * z) / 2, (ch - canvasHeight * z) / 2)
  }

  return (
    <div className="flex items-center gap-3 px-3 py-1.5 border-b border-border/30 bg-background text-[11px] overflow-x-auto shrink-0 min-h-[36px]">

      {/* Color always visible */}
      <ColorInput color={foregroundColor} onChange={setForegroundColor} />

      {/* Brush-specific */}
      {isBrushLike && (
        <>
          <div className="w-px h-4 bg-border/30 shrink-0" />
          <label className="flex items-center gap-1.5 shrink-0">
            <span className="text-muted-foreground">Size</span>
            <input type="range" min={1} max={200} value={brushSize} onChange={e => setBrushSize(Number(e.target.value))} className="w-20 h-1 accent-primary" />
            <input type="number" min={1} max={500} value={brushSize} onChange={e => setBrushSize(Number(e.target.value))} className="w-10 bg-background border border-border/40 rounded px-1 py-0.5 text-[10px] text-center" />
          </label>
          <label className="flex items-center gap-1.5 shrink-0">
            <span className="text-muted-foreground">Hardness</span>
            <input type="range" min={0} max={100} value={brushHardness} onChange={e => setBrushHardness(Number(e.target.value))} className="w-14 h-1 accent-primary" />
            <span className="text-muted-foreground w-6 tabular-nums">{brushHardness}%</span>
          </label>
          <label className="flex items-center gap-1.5 shrink-0">
            <span className="text-muted-foreground">Opacity</span>
            <input type="range" min={1} max={100} value={brushOpacity} onChange={e => setBrushOpacity(Number(e.target.value))} className="w-14 h-1 accent-primary" />
            <span className="text-muted-foreground w-6 tabular-nums">{brushOpacity}%</span>
          </label>
        </>
      )}

      {/* Shape-specific */}
      {isShapeTool && (
        <>
          <div className="w-px h-4 bg-border/30 shrink-0" />
          <div className="flex items-center gap-1 shrink-0">
            {SHAPE_TYPES.map(s => (
              <button
                key={s.value}
                onClick={() => setShapeType(s.value)}
                className={`px-2 py-0.5 rounded text-[10px] transition-colors ${shapeType === s.value ? 'bg-primary/20 text-primary' : 'text-muted-foreground hover:bg-accent/40 hover:text-foreground'}`}
              >
                {s.label}
              </button>
            ))}
          </div>
          {shapeType === 'polygon' && (
            <label className="flex items-center gap-1.5 shrink-0">
              <span className="text-muted-foreground">Sides</span>
              <input type="number" min={3} max={12} value={polygonSides} onChange={e => setPolygonSides(Number(e.target.value))} className="w-10 bg-background border border-border/40 rounded px-1 py-0.5 text-[10px] text-center" />
            </label>
          )}
          <label className="flex items-center gap-1.5 shrink-0">
            <span className="text-muted-foreground">Size</span>
            <input type="range" min={1} max={20} value={Math.round(brushSize * 0.15 + 1)} onChange={e => setBrushSize(Math.round((Number(e.target.value) - 1) / 0.15))} className="w-14 h-1 accent-primary" />
          </label>
        </>
      )}

      {/* Crop-specific */}
      {isCrop && (
        <>
          <div className="w-px h-4 bg-border/30 shrink-0" />
          <label className="flex items-center gap-1.5 shrink-0">
            <span className="text-muted-foreground">Ratio</span>
            <select
              value={cropRatio}
              onChange={e => setCropRatio(e.target.value as CropRatio)}
              className="bg-background border border-border/40 rounded px-1.5 py-0.5 text-[10px] text-foreground/70"
            >
              {CROP_RATIOS.map(r => <option key={r} value={r}>{r}</option>)}
            </select>
          </label>
        </>
      )}

      {/* Gradient */}
      {activeTool === 'gradient' && (
        <>
          <div className="w-px h-4 bg-border/30 shrink-0" />
          <ColorInput color={foregroundColor} onChange={setForegroundColor} />
          <span className="text-muted-foreground/50">→</span>
          <ColorInput color={backgroundColor} onChange={setBackgroundColor} />
          <div className="flex items-center gap-1 shrink-0">
            {(['linear', 'radial'] as const).map(t => (
              <button key={t} onClick={() => setGradientType(t)}
                className={`px-2 py-0.5 rounded text-[10px] transition-colors ${gradientType === t ? 'bg-primary/20 text-primary' : 'text-muted-foreground hover:bg-accent/40'}`}>
                {t}
              </button>
            ))}
          </div>
        </>
      )}

      {/* Dodge / Burn */}
      {(activeTool === 'dodge' || activeTool === 'burn') && (
        <>
          <div className="w-px h-4 bg-border/30 shrink-0" />
          <label className="flex items-center gap-1.5 shrink-0">
            <span className="text-muted-foreground">Size</span>
            <input type="range" min={1} max={200} value={brushSize} onChange={e => setBrushSize(Number(e.target.value))} className="w-20 h-1 accent-primary" />
            <span className="text-muted-foreground w-8 tabular-nums">{brushSize}</span>
          </label>
          <label className="flex items-center gap-1.5 shrink-0">
            <span className="text-muted-foreground">Strength</span>
            <input type="range" min={1} max={100} value={burnDodgeStrength} onChange={e => setBurnDodgeStrength(Number(e.target.value))} className="w-16 h-1 accent-primary" />
            <span className="text-muted-foreground w-6 tabular-nums">{burnDodgeStrength}%</span>
          </label>
        </>
      )}

      {/* Text-specific */}
      {isText && (
        <>
          <div className="w-px h-4 bg-border/30 shrink-0" />
          <label className="flex items-center gap-1.5 shrink-0">
            <span className="text-muted-foreground">Size</span>
            <input type="range" min={8} max={200} value={brushSize} onChange={e => setBrushSize(Number(e.target.value))} className="w-16 h-1 accent-primary" />
            <span className="text-muted-foreground tabular-nums">{Math.max(8, Math.round(brushSize * 0.6))}px</span>
          </label>
        </>
      )}

      {/* Pen hint */}
      {isPen && (
        <>
          <div className="w-px h-4 bg-border/30 shrink-0" />
          <span className="text-muted-foreground/60 text-[10px]">Click to add points · Click first point to close · Esc to cancel</span>
        </>
      )}

      {/* Selection info */}
      {selection && (
        <span className="text-muted-foreground/50 text-[10px] ml-1">
          {Math.round(selection.w)}×{Math.round(selection.h)}
        </span>
      )}

      {/* Zoom controls (always right-aligned) */}
      <div className="ml-auto flex items-center gap-1 shrink-0">
        <button onClick={() => setZoom(Math.max(0.05, zoom / 1.5))} className="w-5 h-5 flex items-center justify-center text-muted-foreground hover:text-foreground rounded hover:bg-accent/40">−</button>
        <span
          onClick={fitToView}
          title="Click to fit"
          className="text-muted-foreground text-[10px] w-10 text-center cursor-pointer hover:text-foreground select-none tabular-nums"
        >
          {Math.round(zoom * 100)}%
        </span>
        <button onClick={() => setZoom(Math.min(32, zoom * 1.5))} className="w-5 h-5 flex items-center justify-center text-muted-foreground hover:text-foreground rounded hover:bg-accent/40">+</button>
      </div>
    </div>
  )
}
