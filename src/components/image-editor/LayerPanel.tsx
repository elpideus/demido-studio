import { useState, useRef, useCallback } from 'react'
import { Eye, EyeOff, Lock, Unlock, Plus, Trash2, Copy } from 'lucide-react'
import { useImageEditor, layerCanvases } from '../../stores/imageEditor'
import type { BlendMode, Layer } from '../../stores/imageEditor'

const BLEND_MODES: { value: BlendMode; label: string }[] = [
  { value: 'source-over', label: 'Normal' },
  { value: 'multiply',    label: 'Multiply' },
  { value: 'screen',      label: 'Screen' },
  { value: 'overlay',     label: 'Overlay' },
  { value: 'darken',      label: 'Darken' },
  { value: 'lighten',     label: 'Lighten' },
  { value: 'color-dodge', label: 'Color Dodge' },
  { value: 'hard-light',  label: 'Hard Light' },
]

function LayerThumb({ layerId, w, h }: { layerId: string; w: number; h: number }) {
  const sz = 28
  const c = layerCanvases.get(layerId)
  if (!c) return <div className="w-7 h-7 rounded border border-border/30 bg-muted/40 shrink-0" />

  const tmp = document.createElement('canvas')
  tmp.width = sz; tmp.height = sz
  const ctx = tmp.getContext('2d')!
  const scale = Math.min(sz / w, sz / h)
  const dx = (sz - w * scale) / 2, dy = (sz - h * scale) / 2
  // checkerboard bg
  for (let y = 0; y < sz; y += 4)
    for (let x = 0; x < sz; x += 4) {
      ctx.fillStyle = ((x / 4 + y / 4) % 2 === 0) ? '#aaa' : '#fff'
      ctx.fillRect(x, y, 4, 4)
    }
  ctx.drawImage(c, dx, dy, w * scale, h * scale)

  return (
    <img
      src={tmp.toDataURL()}
      className="w-7 h-7 rounded border border-border/30 shrink-0 object-cover"
      alt=""
      draggable={false}
    />
  )
}

interface LayerRowProps {
  layer: Layer
  isActive: boolean
  isDragOver: boolean
  dragOverSide: 'top' | 'bottom' | null
  onDragStart(e: React.PointerEvent): void
  onDragMove(e: React.PointerEvent): void
  onDragEnd(e: React.PointerEvent): void
}

function LayerRow({ layer, isActive, isDragOver, dragOverSide, onDragStart, onDragMove, onDragEnd }: LayerRowProps) {
  const [renaming, setRenaming] = useState(false)
  const [draft, setDraft] = useState(layer.name)
  const [thumbHover, setThumbHover] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  const {
    setActiveLayer, toggleLayerVisibility, toggleLayerLock,
    removeLayer, duplicateLayer, renameLayer,
    setLayerOpacity, setLayerBlendMode,
    canvasWidth, canvasHeight, layers,
  } = useImageEditor()
  const repaintVersion = useImageEditor(s => s.repaintVersion)

  const commitRename = () => {
    setRenaming(false)
    if (draft.trim()) renameLayer(layer.id, draft.trim())
    else setDraft(layer.name)
  }

  const startRename = () => {
    setDraft(layer.name)
    setRenaming(true)
    setTimeout(() => { inputRef.current?.focus(); inputRef.current?.select() }, 0)
  }

  const isHidden = !layer.visible
  const isLocked = layer.locked

  // Row base classes depending on state
  const rowBase = [
    'flex flex-col gap-0.5 px-1.5 py-1 cursor-pointer border-b border-border/15 transition-colors relative',
    isActive ? 'bg-primary/10' : 'hover:bg-accent/20',
    isHidden ? 'opacity-40' : '',
    isLocked && !isHidden ? 'opacity-60' : '',
  ].filter(Boolean).join(' ')

  const lockedTint = isLocked ? 'ring-1 ring-red-500/10' : ''

  return (
    <div
      className={`${rowBase} ${lockedTint}`}
      onClick={() => !renaming && setActiveLayer(layer.id)}
      onDoubleClick={() => !renaming && startRename()}
      onKeyDown={e => { if (e.key === 'F2') startRename() }}
      tabIndex={0}
      style={{
        borderTop: isDragOver && dragOverSide === 'top' ? '2px solid var(--primary)' : undefined,
        borderBottom: isDragOver && dragOverSide === 'bottom' ? '2px solid var(--primary)' : undefined,
        background: isLocked ? 'rgba(239,68,68,0.03)' : undefined,
      }}
    >
      <div className="flex items-center gap-1.5">
        {/* Drag handle */}
        <div
          className="w-3 flex flex-col gap-0.5 cursor-grab shrink-0 opacity-30 hover:opacity-70 touch-none"
          onPointerDown={onDragStart}
          onPointerMove={onDragMove}
          onPointerUp={onDragEnd}
          onPointerCancel={onDragEnd}
        >
          <span className="block w-3 h-px bg-foreground" />
          <span className="block w-3 h-px bg-foreground" />
          <span className="block w-3 h-px bg-foreground" />
        </div>

        {/* Thumbnail with eye icon hover */}
        <div
          className="relative shrink-0"
          onMouseEnter={() => setThumbHover(true)}
          onMouseLeave={() => setThumbHover(false)}
        >
          <LayerThumb layerId={layer.id} w={canvasWidth} h={canvasHeight} key={repaintVersion} />
          {thumbHover && (
            <button
              className="absolute inset-0 flex items-center justify-center bg-black/50 rounded transition-opacity"
              onClick={e => { e.stopPropagation(); toggleLayerVisibility(layer.id) }}
              title={layer.visible ? 'Hide layer' : 'Show layer'}
            >
              {layer.visible
                ? <Eye size={12} className="text-white" />
                : <EyeOff size={12} className="text-white/60" />
              }
            </button>
          )}
        </div>

        {/* Name */}
        <div className="flex-1 min-w-0">
          {renaming ? (
            <input
              ref={inputRef}
              value={draft}
              onChange={e => setDraft(e.target.value)}
              onBlur={commitRename}
              onKeyDown={e => {
                if (e.key === 'Enter') commitRename()
                if (e.key === 'Escape') { setRenaming(false); setDraft(layer.name) }
                e.stopPropagation()
              }}
              onClick={e => e.stopPropagation()}
              className="w-full text-[11px] bg-background border border-primary/40 rounded px-1 py-0 outline-none"
            />
          ) : (
            <span className="text-[11px] text-foreground/75 truncate block leading-tight">
              {layer.name}
            </span>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity" onClick={e => e.stopPropagation()}>
          <button
            onClick={() => toggleLayerLock(layer.id)}
            title={layer.locked ? 'Unlock' : 'Lock'}
            className={`w-5 h-5 flex items-center justify-center rounded transition-colors
              ${layer.locked ? 'text-red-400 hover:text-red-300' : 'text-muted-foreground hover:text-foreground'}`}
          >
            {layer.locked ? <Lock size={10} /> : <Unlock size={10} />}
          </button>
          <button
            onClick={() => duplicateLayer(layer.id)}
            title="Duplicate"
            className="w-5 h-5 flex items-center justify-center rounded text-muted-foreground hover:text-foreground"
          >
            <Copy size={10} />
          </button>
          <button
            onClick={() => removeLayer(layer.id)}
            disabled={layers.length <= 1}
            title="Delete"
            className="w-5 h-5 flex items-center justify-center rounded text-muted-foreground hover:text-red-400 disabled:opacity-20"
          >
            <Trash2 size={10} />
          </button>
        </div>
      </div>

      {/* Expanded controls when active */}
      {isActive && (
        <div className="flex items-center gap-1.5 pl-5 pt-0.5" onClick={e => e.stopPropagation()}>
          <select
            value={layer.blendMode}
            onChange={e => setLayerBlendMode(layer.id, e.target.value as BlendMode)}
            className="text-[10px] bg-background border border-border/40 rounded px-1 py-0.5 text-foreground/60 flex-1 min-w-0"
          >
            {BLEND_MODES.map(m => <option key={m.value} value={m.value}>{m.label}</option>)}
          </select>
          <div className="flex items-center gap-1 shrink-0">
            <input
              type="range" min={0} max={100} value={layer.opacity}
              onChange={e => setLayerOpacity(layer.id, Number(e.target.value))}
              className="w-14 h-1 accent-primary"
            />
            <span className="text-[10px] text-muted-foreground w-7 text-right tabular-nums">
              {layer.opacity}%
            </span>
          </div>
        </div>
      )}
    </div>
  )
}

export function LayerPanel({ width, onResizeStart }: { width: number; onResizeStart(e: React.PointerEvent): void }) {
  const {
    layers, activeLayerId,
    addLayer, cropToSelection, mergeAll,
    selection, undo, redo,
    reorderLayers,
  } = useImageEditor()

  // Drag state for reordering
  const [draggingId, setDraggingId] = useState<string | null>(null)
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null)
  const [dragOverSide, setDragOverSide] = useState<'top' | 'bottom'>('bottom')
  const listRef = useRef<HTMLDivElement>(null)
  const startYRef = useRef(0)

  // Displayed order: top layer first (reversed from store)
  const displayed = [...layers].reverse()

  const handleDragStart = useCallback((e: React.PointerEvent, layerId: string) => {
    e.currentTarget.setPointerCapture(e.pointerId)
    setDraggingId(layerId)
    startYRef.current = e.clientY
  }, [])

  const handleDragMove = useCallback((e: React.PointerEvent) => {
    if (!draggingId || !listRef.current) return
    const listRect = listRef.current.getBoundingClientRect()
    const children = Array.from(listRef.current.children) as HTMLElement[]
    for (let i = 0; i < children.length; i++) {
      const rect = children[i].getBoundingClientRect()
      if (e.clientY >= rect.top && e.clientY < rect.bottom) {
        const mid = rect.top + rect.height / 2
        setDragOverIndex(i)
        setDragOverSide(e.clientY < mid ? 'top' : 'bottom')
        return
      }
    }
    // Below all
    if (e.clientY >= listRect.bottom) { setDragOverIndex(children.length - 1); setDragOverSide('bottom') }
  }, [draggingId])

  const handleDragEnd = useCallback((_e: React.PointerEvent) => {
    if (draggingId !== null && dragOverIndex !== null) {
      // displayed is reversed; convert dragOverIndex to store index
      const storeTarget = layers.length - 1 - dragOverIndex + (dragOverSide === 'bottom' ? 0 : 1)
      reorderLayers(draggingId, storeTarget)
    }
    setDraggingId(null)
    setDragOverIndex(null)
  }, [draggingId, dragOverIndex, dragOverSide, layers.length, reorderLayers])

  return (
    <div
      className="flex flex-col border-l border-border/30 bg-background shrink-0 relative"
      style={{ width }}
    >
      {/* Resize handle */}
      <div
        className="absolute left-0 top-0 bottom-0 w-1 cursor-ew-resize hover:bg-primary/30 active:bg-primary/50 transition-colors z-10"
        onPointerDown={onResizeStart}
      />

      {/* Header */}
      <div className="flex items-center justify-between px-2 py-1.5 border-b border-border/30 ml-1">
        <span className="text-[11px] font-medium text-foreground/50 uppercase tracking-wide">Layers</span>
        <div className="flex items-center gap-0.5">
          <button
            onClick={() => addLayer()}
            title="New transparent layer"
            className="w-6 h-6 flex items-center justify-center text-muted-foreground hover:text-foreground rounded hover:bg-accent/50"
          >
            <Plus size={12} />
          </button>
          <button
            onClick={() => addLayer({ solidColor: '#ffffff', name: 'Color Layer' })}
            title="New color fill layer"
            className="w-6 h-6 flex items-center justify-center text-muted-foreground hover:text-foreground rounded hover:bg-accent/50 text-[10px]"
          >
            ■
          </button>
        </div>
      </div>

      {/* Layer list */}
      <div ref={listRef} className="flex-1 overflow-y-auto overflow-x-hidden min-h-0 group">
        {displayed.map((layer, i) => (
          <LayerRow
            key={layer.id}
            layer={layer}
            isActive={layer.id === activeLayerId}
            isDragOver={dragOverIndex === i}
            dragOverSide={dragOverIndex === i ? dragOverSide : null}
            onDragStart={e => handleDragStart(e, layer.id)}
            onDragMove={handleDragMove}
            onDragEnd={handleDragEnd}
          />
        ))}
      </div>

      {/* Bottom actions */}
      <div className="border-t border-border/30 px-2 py-1.5 ml-1 flex flex-col gap-1">
        <div className="flex gap-1">
          <button
            onClick={undo}
            className="flex-1 text-[10px] py-1 rounded hover:bg-accent/40 text-muted-foreground hover:text-foreground transition-colors"
          >
            Undo
          </button>
          <button
            onClick={redo}
            className="flex-1 text-[10px] py-1 rounded hover:bg-accent/40 text-muted-foreground hover:text-foreground transition-colors"
          >
            Redo
          </button>
        </div>
        {selection && (
          <button
            onClick={cropToSelection}
            className="w-full text-[10px] py-1 rounded bg-primary/10 hover:bg-primary/20 text-primary transition-colors"
          >
            Crop to Selection
          </button>
        )}
        <button
          onClick={mergeAll}
          className="w-full text-[10px] py-1 rounded hover:bg-accent/40 text-muted-foreground hover:text-foreground transition-colors"
        >
          Flatten Image
        </button>
      </div>
    </div>
  )
}
