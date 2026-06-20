import { create } from 'zustand'

export type Tool =
  | 'select' | 'move' | 'marquee' | 'lasso' | 'crop'
  | 'eyedropper' | 'brush' | 'eraser' | 'clone'
  | 'gradient' | 'burn' | 'dodge'
  | 'pen' | 'text' | 'shape' | 'fill'

export type ShapeType = 'rect' | 'ellipse' | 'triangle' | 'polygon'
export type BlendMode = 'source-over' | 'multiply' | 'screen' | 'overlay' | 'darken' | 'lighten' | 'color-dodge' | 'hard-light'

export const CROP_RATIOS = ['Free', '1:1', '4:3', '3:2', '16:9', '2:1', '9:16', '4:5'] as const
export type CropRatio = typeof CROP_RATIOS[number]

export interface TextData {
  text: string
  font: string
  fontSize: number
  color: string
  bold: boolean
  italic: boolean
  x: number
  y: number
}

export interface Layer {
  id: string
  name: string
  visible: boolean
  opacity: number   // 0–100
  blendMode: BlendMode
  locked: boolean
  pendingDataUrl?: string
  solidColor?: string
  textData?: TextData   // vector text, re-editable
}

// Pixel data lives outside Zustand
export const layerCanvases = new Map<string, OffscreenCanvas>()

// History
const historyEntries: Map<string, Uint8ClampedArray>[] = []
let histIdx = -1
const MAX_HISTORY = 30

function genId() { return Math.random().toString(36).slice(2, 10) }

function snapshotLayers(layers: Layer[], w: number, h: number): Map<string, Uint8ClampedArray> {
  const snap = new Map<string, Uint8ClampedArray>()
  for (const l of layers) {
    const c = layerCanvases.get(l.id)
    if (!c) continue
    snap.set(l.id, c.getContext('2d')!.getImageData(0, 0, w, h).data.slice())
  }
  return snap
}

interface State {
  layers: Layer[]
  activeLayerId: string | null
  activeTool: Tool
  shapeType: ShapeType
  polygonSides: number
  cropRatio: CropRatio
  penPoints: { x: number; y: number }[]
  penClosed: boolean
  brushSize: number
  brushHardness: number
  brushOpacity: number
  foregroundColor: string
  backgroundColor: string
  canvasWidth: number
  canvasHeight: number
  zoom: number
  panX: number
  panY: number
  selection: { x: number; y: number; w: number; h: number } | null
  editingTextLayerId: string | null
  cloneSource: { x: number; y: number; layerId: string } | null   // clone stamp sample point
  gradientType: 'linear' | 'radial'
  burnDodgeStrength: number
  fileName: string
  repaintVersion: number
  historyVersion: number   // bumps on undo/redo so canvas can clear transform

  setActiveTool(t: Tool): void
  setActiveLayer(id: string): void
  setShapeType(t: ShapeType): void
  setPolygonSides(n: number): void
  setCropRatio(r: CropRatio): void
  setPenPoints(pts: { x: number; y: number }[], closed?: boolean): void
  clearPen(): void
  setBrushSize(n: number): void
  setBrushHardness(n: number): void
  setBrushOpacity(n: number): void
  setForegroundColor(c: string): void
  setBackgroundColor(c: string): void
  setZoom(z: number): void
  setPan(x: number, y: number): void
  setSelection(s: { x: number; y: number; w: number; h: number } | null): void
  setCanvasSize(w: number, h: number): void
  requestRepaint(): void
  clearPendingDataUrl(id: string): void
  setEditingTextLayer(id: string | null): void
  setCloneSource(src: { x: number; y: number; layerId: string } | null): void
  setGradientType(t: 'linear' | 'radial'): void
  setBurnDodgeStrength(n: number): void

  addLayer(opts?: { name?: string; solidColor?: string; pendingDataUrl?: string; textData?: TextData }): string
  removeLayer(id: string): void
  reorderLayers(fromId: string, toIndex: number): void
  duplicateLayer(id: string): void
  toggleLayerVisibility(id: string): void
  toggleLayerLock(id: string): void
  setLayerOpacity(id: string, v: number): void
  setLayerBlendMode(id: string, mode: BlendMode): void
  renameLayer(id: string, name: string): void
  addImageLayerAbove(dataUrl: string): void
  updateLayerTextData(id: string, data: TextData): void

  cropToSelection(): void
  cropToRect(x: number, y: number, w: number, h: number): void
  mergeAll(): void

  openWithImage(dataUrl: string, fileName: string): void
  openBlank(w: number, h: number, name?: string): void

  pushHistory(): void
  undo(): void
  redo(): void
}

export const useImageEditor = create<State>((set, get) => ({
  layers: [],
  activeLayerId: null,
  activeTool: 'brush',
  shapeType: 'rect',
  polygonSides: 5,
  cropRatio: 'Free',
  penPoints: [],
  penClosed: false,
  brushSize: 20,
  brushHardness: 80,
  brushOpacity: 100,
  foregroundColor: '#000000',
  backgroundColor: '#ffffff',
  canvasWidth: 800,
  canvasHeight: 600,
  zoom: 1,
  panX: 0,
  panY: 0,
  selection: null,
  editingTextLayerId: null,
  historyVersion: 0,
  cloneSource: null,
  gradientType: 'linear',
  burnDodgeStrength: 50,
  fileName: 'Untitled',
  repaintVersion: 0,

  setActiveTool: t => set({ activeTool: t }),
  setActiveLayer: id => set({ activeLayerId: id }),
  setShapeType: t => set({ shapeType: t }),
  setPolygonSides: n => set({ polygonSides: Math.max(3, Math.min(12, n)) }),
  setCropRatio: r => set({ cropRatio: r }),
  setPenPoints: (pts, closed = false) => set({ penPoints: pts, penClosed: closed }),
  clearPen: () => set({ penPoints: [], penClosed: false }),
  setBrushSize: n => set({ brushSize: Math.max(1, Math.min(500, n)) }),
  setBrushHardness: n => set({ brushHardness: Math.max(0, Math.min(100, n)) }),
  setBrushOpacity: n => set({ brushOpacity: Math.max(1, Math.min(100, n)) }),
  setForegroundColor: c => set({ foregroundColor: c }),
  setBackgroundColor: c => set({ backgroundColor: c }),
  setZoom: z => set({ zoom: Math.max(0.05, Math.min(32, z)) }),
  setPan: (x, y) => set({ panX: x, panY: y }),
  setSelection: s => set({ selection: s }),
  setCanvasSize: (w, h) => set({ canvasWidth: w, canvasHeight: h }),
  requestRepaint: () => set(s => ({ repaintVersion: s.repaintVersion + 1 })),
  setEditingTextLayer: id => set({ editingTextLayerId: id }),
  setCloneSource: src => set({ cloneSource: src }),
  setGradientType: t => set({ gradientType: t }),
  setBurnDodgeStrength: n => set({ burnDodgeStrength: Math.max(1, Math.min(100, n)) }),
  clearPendingDataUrl: id => set(s => ({
    layers: s.layers.map(l => l.id === id ? { ...l, pendingDataUrl: undefined } : l),
  })),

  addLayer(opts = {}) {
    const { layers, canvasWidth, canvasHeight } = get()
    const id = genId()
    const canvas = new OffscreenCanvas(canvasWidth, canvasHeight)
    if (opts.solidColor) {
      const ctx = canvas.getContext('2d')!
      ctx.fillStyle = opts.solidColor
      ctx.fillRect(0, 0, canvasWidth, canvasHeight)
    }
    layerCanvases.set(id, canvas)
    const layer: Layer = {
      id,
      name: opts.name ?? `Layer ${layers.length + 1}`,
      visible: true, opacity: 100, blendMode: 'source-over', locked: false,
      pendingDataUrl: opts.pendingDataUrl,
      solidColor: opts.solidColor,
      textData: opts.textData,
    }
    set({ layers: [...layers, layer], activeLayerId: id })
    return id
  },

  removeLayer(id) {
    const { layers, activeLayerId } = get()
    if (layers.length <= 1) return
    layerCanvases.delete(id)
    const next = layers.filter(l => l.id !== id)
    const newActive = activeLayerId === id ? (next[next.length - 1]?.id ?? null) : activeLayerId
    set({ layers: next, activeLayerId: newActive })
  },

  reorderLayers(fromId, toIndex) {
    const { layers } = get()
    const fromIndex = layers.findIndex(l => l.id === fromId)
    if (fromIndex < 0) return
    const arr = [...layers]
    const [item] = arr.splice(fromIndex, 1)
    arr.splice(Math.max(0, Math.min(toIndex, arr.length)), 0, item)
    set({ layers: arr })
  },

  duplicateLayer(id) {
    const { layers, canvasWidth, canvasHeight } = get()
    const src = layers.find(l => l.id === id)
    if (!src) return
    const newId = genId()
    const newC = new OffscreenCanvas(canvasWidth, canvasHeight)
    const srcC = layerCanvases.get(id)
    if (srcC) newC.getContext('2d')!.drawImage(srcC, 0, 0)
    layerCanvases.set(newId, newC)
    const idx = layers.findIndex(l => l.id === id)
    const newLayer: Layer = { ...src, id: newId, name: src.name + ' copy' }
    const arr = [...layers.slice(0, idx + 1), newLayer, ...layers.slice(idx + 1)]
    set({ layers: arr, activeLayerId: newId })
  },

  toggleLayerVisibility: id => set(s => ({
    layers: s.layers.map(l => l.id === id ? { ...l, visible: !l.visible } : l),
  })),

  toggleLayerLock: id => set(s => ({
    layers: s.layers.map(l => l.id === id ? { ...l, locked: !l.locked } : l),
  })),

  setLayerOpacity: (id, v) => set(s => ({
    layers: s.layers.map(l => l.id === id ? { ...l, opacity: v } : l),
  })),

  setLayerBlendMode: (id, mode) => set(s => ({
    layers: s.layers.map(l => l.id === id ? { ...l, blendMode: mode } : l),
  })),

  renameLayer: (id, name) => set(s => ({
    layers: s.layers.map(l => l.id === id ? { ...l, name } : l),
  })),

  updateLayerTextData: (id, data) => set(s => ({
    layers: s.layers.map(l => l.id === id ? { ...l, textData: data } : l),
  })),

  addImageLayerAbove(dataUrl) {
    const { layers, activeLayerId, canvasWidth, canvasHeight } = get()
    const id = genId()
    layerCanvases.set(id, new OffscreenCanvas(canvasWidth, canvasHeight))
    const layer: Layer = {
      id, name: 'Dropped Image', visible: true, opacity: 100,
      blendMode: 'source-over', locked: false, pendingDataUrl: dataUrl,
    }
    const idx = activeLayerId ? layers.findIndex(l => l.id === activeLayerId) : layers.length - 1
    const arr = idx >= 0
      ? [...layers.slice(0, idx + 1), layer, ...layers.slice(idx + 1)]
      : [...layers, layer]
    set({ layers: arr, activeLayerId: id })
  },

  cropToSelection() {
    const { selection } = get()
    if (!selection) return
    get().cropToRect(selection.x, selection.y, selection.w, selection.h)
  },

  cropToRect(x, y, w, h) {
    const { layers, canvasWidth, canvasHeight } = get()
    const cx = Math.round(Math.max(0, x)), cy = Math.round(Math.max(0, y))
    const cw = Math.round(Math.min(w, canvasWidth - cx))
    const ch = Math.round(Math.min(h, canvasHeight - cy))
    if (cw <= 0 || ch <= 0) return
    for (const l of layers) {
      const src = layerCanvases.get(l.id)
      if (!src) continue
      const newC = new OffscreenCanvas(cw, ch)
      newC.getContext('2d')!.drawImage(src, -cx, -cy)
      layerCanvases.set(l.id, newC)
    }
    set({ canvasWidth: cw, canvasHeight: ch, selection: null })
    set(s => ({ repaintVersion: s.repaintVersion + 1 }))
  },

  mergeAll() {
    const { layers, canvasWidth, canvasHeight } = get()
    const merged = new OffscreenCanvas(canvasWidth, canvasHeight)
    const ctx = merged.getContext('2d')!
    for (const l of layers) {
      if (!l.visible) continue
      const c = layerCanvases.get(l.id)
      if (!c) continue
      ctx.save()
      ctx.globalAlpha = l.opacity / 100
      ctx.globalCompositeOperation = l.blendMode as GlobalCompositeOperation
      ctx.drawImage(c, 0, 0)
      ctx.restore()
    }
    const id = genId()
    layerCanvases.clear()
    layerCanvases.set(id, merged)
    const layer: Layer = { id, name: 'Merged', visible: true, opacity: 100, blendMode: 'source-over', locked: false }
    set({ layers: [layer], activeLayerId: id })
    set(s => ({ repaintVersion: s.repaintVersion + 1 }))
  },

  openWithImage(dataUrl, fileName) {
    layerCanvases.clear()
    historyEntries.length = 0
    histIdx = -1
    const id = genId()
    layerCanvases.set(id, new OffscreenCanvas(1, 1))
    const layer: Layer = {
      id, name: fileName, visible: true, opacity: 100,
      blendMode: 'source-over', locked: false, pendingDataUrl: dataUrl,
    }
    set({
      layers: [layer], activeLayerId: id, fileName,
      selection: null, zoom: 1, panX: 0, panY: 0, repaintVersion: 0,
      penPoints: [], penClosed: false, editingTextLayerId: null,
    })
  },

  openBlank(w, h, name = 'Untitled') {
    layerCanvases.clear()
    historyEntries.length = 0
    histIdx = -1
    const id = genId()
    const canvas = new OffscreenCanvas(w, h)
    canvas.getContext('2d')!.fillStyle = '#ffffff'
    canvas.getContext('2d')!.fillRect(0, 0, w, h)
    layerCanvases.set(id, canvas)
    const layer: Layer = { id, name: 'Background', visible: true, opacity: 100, blendMode: 'source-over', locked: false }
    set({
      layers: [layer], activeLayerId: id, canvasWidth: w, canvasHeight: h,
      fileName: name, selection: null, zoom: 1, panX: 0, panY: 0, repaintVersion: 0,
      penPoints: [], penClosed: false, editingTextLayerId: null,
    })
  },

  pushHistory() {
    const { layers, canvasWidth, canvasHeight } = get()
    historyEntries.splice(histIdx + 1)
    historyEntries.push(snapshotLayers(layers, canvasWidth, canvasHeight))
    if (historyEntries.length > MAX_HISTORY) historyEntries.shift()
    histIdx = historyEntries.length - 1
  },

  undo() {
    if (histIdx <= 0) return
    const { layers, canvasWidth, canvasHeight } = get()
    if (histIdx === historyEntries.length - 1) {
      historyEntries.push(snapshotLayers(layers, canvasWidth, canvasHeight))
    }
    histIdx = Math.max(0, histIdx - 1)
    const snap = historyEntries[histIdx]
    for (const l of layers) {
      const c = layerCanvases.get(l.id)
      const data = snap?.get(l.id)
      if (!c || !data) continue
      c.getContext('2d')!.putImageData(new ImageData(data.slice(), canvasWidth, canvasHeight), 0, 0)
    }
    set(s => ({ repaintVersion: s.repaintVersion + 1, historyVersion: s.historyVersion + 1 }))
  },

  redo() {
    if (histIdx >= historyEntries.length - 1) return
    histIdx++
    const snap = historyEntries[histIdx]
    const { layers, canvasWidth, canvasHeight } = get()
    for (const l of layers) {
      const c = layerCanvases.get(l.id)
      const data = snap?.get(l.id)
      if (!c || !data) continue
      c.getContext('2d')!.putImageData(new ImageData(data.slice(), canvasWidth, canvasHeight), 0, 0)
    }
    set(s => ({ repaintVersion: s.repaintVersion + 1, historyVersion: s.historyVersion + 1 }))
  },
}))
