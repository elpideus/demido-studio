import { useState } from 'react'
import {
  MousePointer2, Move, RectangleHorizontal, Lasso, Crop,
  Pipette, Paintbrush, Eraser, PenTool,
  Type, Shapes, Lock, Unlock, Stamp, Blend,
  Sun, Moon,
} from 'lucide-react'
import { useImageEditor } from '../../stores/imageEditor'
import type { Tool, ShapeType } from '../../stores/imageEditor'

interface ToolDef {
  id: Tool
  label: string
  key: string
  icon: React.ReactNode
  sub?: { id: ShapeType | string; label: string; icon: React.ReactNode }[]
  subProp?: 'shapeType'
}

const TOOLS: ToolDef[] = [
  { id: 'select',     label: 'Select (S)',       key: 'S', icon: <MousePointer2 size={14} /> },
  { id: 'move',       label: 'Move (V)',          key: 'V', icon: <Move size={14} /> },
  { id: 'marquee',    label: 'Marquee (M)',       key: 'M', icon: <RectangleHorizontal size={14} /> },
  { id: 'lasso',      label: 'Lasso (L)',        key: 'L', icon: <Lasso size={14} /> },
  { id: 'crop',       label: 'Crop (C)',         key: 'C', icon: <Crop size={14} /> },
  { id: 'eyedropper', label: 'Color Picker (I)', key: 'I', icon: <Pipette size={14} /> },
  { id: 'brush',      label: 'Brush (B)',        key: 'B', icon: <Paintbrush size={14} /> },
  { id: 'eraser',     label: 'Eraser (E)',       key: 'E', icon: <Eraser size={14} /> },
  { id: 'clone',      label: 'Clone Stamp (Alt+click to sample, then paint)', key: '', icon: <Stamp size={14} /> },
  { id: 'gradient',   label: 'Gradient (G)',     key: 'G', icon: <Blend size={14} /> },
  { id: 'dodge',      label: 'Dodge (lighten)',  key: '', icon: <Sun size={14} /> },
  { id: 'burn',       label: 'Burn (darken)',    key: '', icon: <Moon size={14} /> },
  { id: 'pen',        label: 'Pen (P)',          key: 'P', icon: <PenTool size={14} /> },
  { id: 'text',       label: 'Text (T)',         key: 'T', icon: <Type size={14} /> },
  {
    id: 'shape', label: 'Shape (U)', key: 'U', icon: <Shapes size={14} />,
    subProp: 'shapeType',
    sub: [
      { id: 'rect',     label: 'Rectangle', icon: <span className="text-[10px]">▭</span> },
      { id: 'ellipse',  label: 'Ellipse',   icon: <span className="text-[10px]">◯</span> },
      { id: 'triangle', label: 'Triangle',  icon: <span className="text-[10px]">△</span> },
      { id: 'polygon',  label: 'Polygon',   icon: <span className="text-[10px]">⬡</span> },
    ],
  },
]

export function Toolbar() {
  const {
    activeTool, setActiveTool,
    shapeType, setShapeType,
    foregroundColor, backgroundColor,
    setForegroundColor, setBackgroundColor,
    activeLayerId, layers, toggleLayerLock,
  } = useImageEditor()

  const [expandedTool, setExpandedTool] = useState<string | null>(null)

  const activeLayer = layers.find(l => l.id === activeLayerId)

  const handleToolClick = (t: ToolDef) => {
    if (activeTool === t.id && t.sub) {
      setExpandedTool(prev => prev === t.id ? null : t.id)
      return
    }
    setActiveTool(t.id)
    setExpandedTool(null)
  }

  const swapColors = () => {
    const fg = foregroundColor
    setForegroundColor(backgroundColor)
    setBackgroundColor(fg)
  }

  const openColorPicker = (isFg: boolean) => {
    const inp = document.createElement('input')
    inp.type = 'color'
    inp.value = isFg ? foregroundColor : backgroundColor
    inp.onchange = () => isFg ? setForegroundColor(inp.value) : setBackgroundColor(inp.value)
    inp.click()
  }

  return (
    <div className="flex flex-col items-center gap-0.5 py-2 px-1 border-r border-border/30 bg-background w-10 shrink-0 select-none overflow-visible relative">
      {TOOLS.map(t => (
        <div key={t.id} className="relative w-full flex flex-col items-center">
          <button
            title={t.label}
            onClick={() => handleToolClick(t)}
            className={`w-8 h-8 rounded flex items-center justify-center transition-colors relative
              ${activeTool === t.id
                ? 'bg-primary/90 text-primary-foreground shadow-sm'
                : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground'
              }`}
          >
            {t.icon}
            {/* Sub-tool indicator */}
            {t.sub && (
              <span className="absolute bottom-0.5 right-0.5 w-1 h-1 rounded-full bg-current opacity-50" />
            )}
          </button>

          {/* Sub-tool flyout */}
          {expandedTool === t.id && t.sub && (
            <div className="absolute left-full top-0 ml-1 z-50 bg-popover border border-border rounded-md shadow-lg p-1 flex flex-col gap-0.5 min-w-[110px]">
              {t.sub.map(sub => (
                <button
                  key={sub.id}
                  onClick={() => {
                    if (t.subProp === 'shapeType') setShapeType(sub.id as ShapeType)
                    setActiveTool(t.id)
                    setExpandedTool(null)
                  }}
                  className={`flex items-center gap-2 px-2 py-1.5 rounded text-[11px] transition-colors
                    ${(t.subProp === 'shapeType' && shapeType === sub.id)
                      ? 'bg-primary/20 text-primary'
                      : 'hover:bg-accent text-foreground/80'
                    }`}
                >
                  {sub.icon}
                  {sub.label}
                </button>
              ))}
            </div>
          )}
        </div>
      ))}

      {/* Divider */}
      <div className="w-6 h-px bg-border/40 my-1" />

      {/* Lock active layer */}
      {activeLayer && (
        <button
          title={activeLayer.locked ? 'Unlock layer' : 'Lock layer'}
          onClick={() => activeLayer && toggleLayerLock(activeLayer.id)}
          className={`w-8 h-8 rounded flex items-center justify-center transition-colors
            ${activeLayer.locked
              ? 'text-red-400 bg-red-400/10 hover:bg-red-400/20'
              : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground'
            }`}
        >
          {activeLayer.locked ? <Lock size={13} /> : <Unlock size={13} />}
        </button>
      )}

      {/* Color swatches at bottom */}
      <div className="mt-auto mb-0.5 flex flex-col items-center gap-1">
        {/* Swap button */}
        <button
          onClick={swapColors}
          title="Swap colors (X)"
          className="text-[9px] text-muted-foreground hover:text-foreground transition-colors leading-none"
        >
          ⇄
        </button>
        {/* Swatch stack */}
        <div className="relative w-8 h-8">
          {/* Background */}
          <button
            onClick={() => openColorPicker(false)}
            title="Background color"
            className="absolute bottom-0 right-0 w-5 h-5 rounded border border-border/60 shadow-sm hover:scale-105 transition-transform"
            style={{ background: backgroundColor }}
          />
          {/* Foreground */}
          <button
            onClick={() => openColorPicker(true)}
            title="Foreground color"
            className="absolute top-0 left-0 w-5 h-5 rounded border border-border/60 shadow-sm hover:scale-105 transition-transform z-10"
            style={{ background: foregroundColor }}
          />
        </div>
      </div>
    </div>
  )
}
