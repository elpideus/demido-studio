import { useRef, useEffect, useCallback, useState } from 'react'
import { useImageEditor, layerCanvases } from '../../stores/imageEditor'
import { Rulers } from './Rulers'

// ─── Types ────────────────────────────────────────────────────────────────────

type Rect = { x: number; y: number; w: number; h: number }

interface TransformState {
  layerId: string
  bounds: Rect       // current displayed bounds (unrotated)
  rotation: number   // radians
  originalBounds: Rect  // content bounds when transform began
}

// ─── Image utilities ──────────────────────────────────────────────────────────

export function pixelOp(
  canvas: OffscreenCanvas,
  fn: (r: number, g: number, b: number, a: number) => [number, number, number, number],
) {
  const ctx = canvas.getContext('2d')!
  const { width: w, height: h } = canvas
  const id = ctx.getImageData(0, 0, w, h)
  const d = id.data
  for (let i = 0; i < d.length; i += 4) {
    const [r, g, b, a] = fn(d[i], d[i + 1], d[i + 2], d[i + 3])
    d[i] = r; d[i + 1] = g; d[i + 2] = b; d[i + 3] = a
  }
  ctx.putImageData(id, 0, 0)
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  r /= 255; g /= 255; b /= 255
  const max = Math.max(r, g, b), min = Math.min(r, g, b)
  const l = (max + min) / 2
  if (max === min) return [0, 0, l]
  const d = max - min
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
  let h = 0
  if (max === r) h = (g - b) / d + (g < b ? 6 : 0)
  else if (max === g) h = (b - r) / d + 2
  else h = (r - g) / d + 4
  return [h / 6, s, l]
}

function hue2rgb(p: number, q: number, t: number) {
  if (t < 0) t += 1; if (t > 1) t -= 1
  if (t < 1 / 6) return p + (q - p) * 6 * t
  if (t < 1 / 2) return q
  if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6
  return p
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  if (s === 0) { const v = Math.round(l * 255); return [v, v, v] }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s
  const p = 2 * l - q
  return [
    Math.round(hue2rgb(p, q, h + 1 / 3) * 255),
    Math.round(hue2rgb(p, q, h) * 255),
    Math.round(hue2rgb(p, q, h - 1 / 3) * 255),
  ]
}

export function adjustBrightness(canvas: OffscreenCanvas, v: number) {
  const amt = v * 2.55
  pixelOp(canvas, (r, g, b, a) => [
    Math.max(0, Math.min(255, r + amt)),
    Math.max(0, Math.min(255, g + amt)),
    Math.max(0, Math.min(255, b + amt)), a,
  ])
}

export function adjustContrast(canvas: OffscreenCanvas, v: number) {
  const f = (259 * (v + 255)) / (255 * (259 - v))
  pixelOp(canvas, (r, g, b, a) => [
    Math.max(0, Math.min(255, f * (r - 128) + 128)),
    Math.max(0, Math.min(255, f * (g - 128) + 128)),
    Math.max(0, Math.min(255, f * (b - 128) + 128)), a,
  ])
}

export function adjustHueSaturation(canvas: OffscreenCanvas, hDeg: number, sMult: number, lAdd: number) {
  pixelOp(canvas, (r, g, b, a) => {
    let [h, s, l] = rgbToHsl(r, g, b)
    h = ((h + hDeg / 360) % 1 + 1) % 1
    s = Math.max(0, Math.min(1, s * sMult))
    l = Math.max(0, Math.min(1, l + lAdd))
    const [nr, ng, nb] = hslToRgb(h, s, l)
    return [nr, ng, nb, a]
  })
}

export function applyInvert(canvas: OffscreenCanvas) {
  pixelOp(canvas, (r, g, b, a) => [255 - r, 255 - g, 255 - b, a])
}

export function applyGrayscale(canvas: OffscreenCanvas) {
  pixelOp(canvas, (r, g, b, a) => {
    const v = Math.round(0.299 * r + 0.587 * g + 0.114 * b)
    return [v, v, v, a]
  })
}

export function applyBlur(canvas: OffscreenCanvas, radius: number) {
  const tmp = new OffscreenCanvas(canvas.width, canvas.height)
  const ctx = tmp.getContext('2d')!
  ctx.filter = `blur(${radius}px)`
  ctx.drawImage(canvas, 0, 0)
  ctx.filter = 'none'
  const dest = canvas.getContext('2d')!
  dest.clearRect(0, 0, canvas.width, canvas.height)
  dest.drawImage(tmp, 0, 0)
}

export function applySharpen(canvas: OffscreenCanvas) {
  const { width: w, height: h } = canvas
  const ctx = canvas.getContext('2d')!
  const src = ctx.getImageData(0, 0, w, h)
  const dst = ctx.createImageData(w, h)
  const s = src.data, d = dst.data
  const kernel = [0, -1, 0, -1, 5, -1, 0, -1, 0]
  for (let y = 1; y < h - 1; y++) {
    for (let x = 1; x < w - 1; x++) {
      const i = (y * w + x) * 4
      for (let c = 0; c < 3; c++) {
        let sum = 0
        for (let ky = -1; ky <= 1; ky++)
          for (let kx = -1; kx <= 1; kx++)
            sum += s[((y + ky) * w + (x + kx)) * 4 + c] * kernel[(ky + 1) * 3 + (kx + 1)]
        d[i + c] = Math.max(0, Math.min(255, sum))
      }
      d[i + 3] = s[i + 3]
    }
  }
  ctx.putImageData(dst, 0, 0)
}

export function applyConvolution(canvas: OffscreenCanvas, kernel: number[], divisor = 1) {
  const { width: w, height: h } = canvas
  const ctx = canvas.getContext('2d')!
  const src = ctx.getImageData(0, 0, w, h)
  const dst = ctx.createImageData(w, h)
  const s = src.data, d = dst.data
  const ks = Math.round(Math.sqrt(kernel.length)), half = Math.floor(ks / 2)
  for (let y = half; y < h - half; y++) {
    for (let x = half; x < w - half; x++) {
      const i = (y * w + x) * 4
      for (let c = 0; c < 3; c++) {
        let sum = 0
        for (let ky = 0; ky < ks; ky++)
          for (let kx = 0; kx < ks; kx++)
            sum += s[((y + ky - half) * w + (x + kx - half)) * 4 + c] * kernel[ky * ks + kx]
        d[i + c] = Math.max(0, Math.min(255, sum / divisor))
      }
      d[i + 3] = s[i + 3]
    }
  }
  ctx.putImageData(dst, 0, 0)
}

export function applyEmboss(canvas: OffscreenCanvas) {
  applyConvolution(canvas, [-2, -1, 0, -1, 1, 1, 0, 1, 2])
}

export function applyFindEdges(canvas: OffscreenCanvas) {
  applyConvolution(canvas, [-1, -1, -1, -1, 8, -1, -1, -1, -1])
}

export function applyMotionBlur(canvas: OffscreenCanvas, angleDeg: number, distance: number) {
  const tmp = new OffscreenCanvas(canvas.width, canvas.height)
  const ctx = tmp.getContext('2d')!
  const rad = (angleDeg * Math.PI) / 180
  const dx = Math.cos(rad) * distance
  const dy = Math.sin(rad) * distance
  ctx.save()
  ctx.translate(canvas.width / 2 + dx / 2, canvas.height / 2 + dy / 2)
  for (let i = 0; i < distance; i++) {
    ctx.globalAlpha = 1 / distance
    ctx.drawImage(canvas,
      -canvas.width / 2 - (dx * i) / distance,
      -canvas.height / 2 - (dy * i) / distance,
    )
  }
  ctx.restore()
  const dest = canvas.getContext('2d')!
  dest.clearRect(0, 0, canvas.width, canvas.height)
  dest.drawImage(tmp, 0, 0)
}

export function applyNoise(canvas: OffscreenCanvas, amount: number) {
  pixelOp(canvas, (r, g, b, a) => {
    if (a === 0) return [r, g, b, a]
    const n = (Math.random() - 0.5) * amount * 2.55
    return [
      Math.max(0, Math.min(255, r + n)),
      Math.max(0, Math.min(255, g + n)),
      Math.max(0, Math.min(255, b + n)), a,
    ]
  })
}

export function applyPosterize(canvas: OffscreenCanvas, levels: number) {
  const step = 255 / Math.max(2, levels - 1)
  pixelOp(canvas, (r, g, b, a) => [
    Math.round(Math.round(r / step) * step),
    Math.round(Math.round(g / step) * step),
    Math.round(Math.round(b / step) * step), a,
  ])
}

export function applyThreshold(canvas: OffscreenCanvas, value: number) {
  pixelOp(canvas, (r, g, b, a) => {
    const v = 0.299 * r + 0.587 * g + 0.114 * b >= value ? 255 : 0
    return [v, v, v, a]
  })
}

export function applyLevels(
  canvas: OffscreenCanvas,
  inLow: number, inHigh: number, gamma: number,
  outLow: number, outHigh: number,
) {
  pixelOp(canvas, (r, g, b, a) => {
    const map = (v: number) => {
      const n = Math.max(0, Math.min(255, v - inLow)) / Math.max(1, inHigh - inLow)
      const g2 = Math.pow(n, 1 / gamma)
      return Math.round(outLow + g2 * (outHigh - outLow))
    }
    return [map(r), map(g), map(b), a]
  })
}

// Simplified curves: takes array of {x,y} points (0-255 input → 0-255 output)
export function applyCurves(canvas: OffscreenCanvas, points: { x: number; y: number }[]) {
  if (points.length < 2) return
  const sorted = [...points].sort((a, b) => a.x - b.x)
  const lut = new Uint8Array(256)
  for (let i = 0; i < 256; i++) {
    // Linear interpolation between control points
    let lo = sorted[0], hi = sorted[sorted.length - 1]
    for (let j = 0; j < sorted.length - 1; j++) {
      if (i >= sorted[j].x && i <= sorted[j + 1].x) { lo = sorted[j]; hi = sorted[j + 1]; break }
    }
    const t = lo.x === hi.x ? 0 : (i - lo.x) / (hi.x - lo.x)
    lut[i] = Math.max(0, Math.min(255, Math.round(lo.y + t * (hi.y - lo.y))))
  }
  pixelOp(canvas, (r, g, b, a) => [lut[r], lut[g], lut[b], a])
}

export function adjustVibrance(canvas: OffscreenCanvas, amount: number) {
  // Vibrance boosts low-saturation pixels more than high-saturation ones
  pixelOp(canvas, (r, g, b, a) => {
    const [h, s, l] = rgbToHsl(r, g, b)
    const boost = (amount / 100) * (1 - s) * 0.5  // low-sat pixels get bigger boost
    const ns = Math.max(0, Math.min(1, s + boost))
    const [nr, ng, nb] = hslToRgb(h, ns, l)
    return [nr, ng, nb, a]
  })
}

export function adjustShadowsHighlights(canvas: OffscreenCanvas, shadows: number, highlights: number) {
  pixelOp(canvas, (r, g, b, a) => {
    const lum = (0.299 * r + 0.587 * g + 0.114 * b) / 255
    const shadowAmt = (1 - lum) * shadows * 2.55     // darks get shadow boost
    const hiAmt = lum * highlights * 2.55             // lights get highlight boost
    return [
      Math.max(0, Math.min(255, r + shadowAmt + hiAmt)),
      Math.max(0, Math.min(255, g + shadowAmt + hiAmt)),
      Math.max(0, Math.min(255, b + shadowAmt + hiAmt)), a,
    ]
  })
}

// ─── Misc helpers ─────────────────────────────────────────────────────────────

function hexToRgb(hex: string) {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  return `${r},${g},${b}`
}

function drawCheckerboard(ctx: CanvasRenderingContext2D, w: number, h: number) {
  const sz = 8
  for (let y = 0; y < h; y += sz)
    for (let x = 0; x < w; x += sz) {
      ctx.fillStyle = ((x / sz + y / sz) % 2 === 0) ? '#c0c0c0' : '#e8e8e8'
      ctx.fillRect(x, y, sz, sz)
    }
}

function drawBrushDot(
  ctx: OffscreenCanvasRenderingContext2D, x: number, y: number,
  size: number, hardness: number, color: string, opacity: number, isEraser: boolean,
) {
  const r = size / 2
  ctx.save()
  ctx.globalAlpha = opacity / 100
  if (isEraser) {
    ctx.globalCompositeOperation = 'destination-out'
    const g = ctx.createRadialGradient(x, y, 0, x, y, r)
    const e = Math.min(hardness / 100, 0.99)
    g.addColorStop(0, 'rgba(0,0,0,1)'); g.addColorStop(e, 'rgba(0,0,0,1)'); g.addColorStop(1, 'rgba(0,0,0,0)')
    ctx.fillStyle = g
  } else {
    const rgb = hexToRgb(color)
    const g = ctx.createRadialGradient(x, y, 0, x, y, r)
    const e = Math.min(hardness / 100, 0.99)
    g.addColorStop(0, `rgba(${rgb},1)`); g.addColorStop(e, `rgba(${rgb},1)`); g.addColorStop(1, `rgba(${rgb},0)`)
    ctx.fillStyle = g
  }
  ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2); ctx.fill()
  ctx.restore()
}

function dodgeBurnDot(ctx: OffscreenCanvasRenderingContext2D, x: number, y: number, size: number, strength: number, isDodge: boolean) {
  const r = size / 2
  const cvs = ctx.canvas as OffscreenCanvas
  const w = cvs.width, h = cvs.height
  const id = ctx.getImageData(Math.max(0, x - r), Math.max(0, y - r), Math.min(size, w - Math.max(0, x - r)), Math.min(size, h - Math.max(0, y - r)))
  const d = id.data
  const amt = strength / 200  // 0..0.5
  for (let py = 0; py < id.height; py++) {
    for (let px = 0; px < id.width; px++) {
      const dist = Math.hypot(px - r, py - r)
      if (dist > r) continue
      const falloff = 1 - dist / r
      const i = (py * id.width + px) * 4
      for (let c = 0; c < 3; c++) {
        const v = d[i + c]
        d[i + c] = isDodge
          ? Math.min(255, v + (255 - v) * amt * falloff)
          : Math.max(0, v - v * amt * falloff)
      }
    }
  }
  ctx.putImageData(id, Math.max(0, x - r), Math.max(0, y - r))
}

function drawBrushSegment(
  ctx: OffscreenCanvasRenderingContext2D,
  x0: number, y0: number, x1: number, y1: number,
  size: number, hardness: number, color: string, opacity: number, isEraser: boolean,
) {
  const dist = Math.hypot(x1 - x0, y1 - y0)
  const steps = Math.max(1, Math.ceil(dist / Math.max(1, size * 0.2)))
  for (let i = 0; i <= steps; i++) {
    const t = i / steps
    drawBrushDot(ctx, x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, size, hardness, color, opacity, isEraser)
  }
}

function floodFill(canvas: OffscreenCanvas, sx: number, sy: number, fillColor: string) {
  const ctx = canvas.getContext('2d')!
  const w = canvas.width, h = canvas.height
  const imageData = ctx.getImageData(0, 0, w, h); const data = imageData.data
  const x = Math.floor(sx), y = Math.floor(sy)
  if (x < 0 || x >= w || y < 0 || y >= h) return
  const idx = (y * w + x) * 4
  const [tr, tg, tb, ta] = [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]
  const tmp = document.createElement('canvas'); tmp.width = tmp.height = 1
  const tc = tmp.getContext('2d')!; tc.fillStyle = fillColor; tc.fillRect(0, 0, 1, 1)
  const fd = tc.getImageData(0, 0, 1, 1).data
  const [fr, fg, fb, fa] = [fd[0], fd[1], fd[2], fd[3]]
  if (tr === fr && tg === fg && tb === fb && ta === fa) return
  const visited = new Uint8Array(w * h)
  const stack = [x + y * w]
  while (stack.length) {
    const pos = stack.pop()!
    if (visited[pos]) continue; visited[pos] = 1
    const px = pos % w, py = Math.floor(pos / w), i = pos * 4
    if (data[i] !== tr || data[i + 1] !== tg || data[i + 2] !== tb || data[i + 3] !== ta) continue
    data[i] = fr; data[i + 1] = fg; data[i + 2] = fb; data[i + 3] = fa
    if (px > 0) stack.push(pos - 1)
    if (px < w - 1) stack.push(pos + 1)
    if (py > 0) stack.push(pos - w)
    if (py < h - 1) stack.push(pos + w)
  }
  ctx.putImageData(imageData, 0, 0)
}

function drawPolygon(ctx: OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D, cx: number, cy: number, rx: number, ry: number, sides: number) {
  ctx.beginPath()
  for (let i = 0; i < sides; i++) {
    const angle = (i * 2 * Math.PI / sides) - Math.PI / 2
    const x = cx + rx * Math.cos(angle), y = cy + ry * Math.sin(angle)
    if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y)
  }
  ctx.closePath()
}

function drawTriangle(ctx: OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D, x0: number, y0: number, x1: number, y1: number) {
  ctx.beginPath()
  ctx.moveTo((x0 + x1) / 2, y0)
  ctx.lineTo(x1, y1)
  ctx.lineTo(x0, y1)
  ctx.closePath()
}

// ─── Transform helpers ────────────────────────────────────────────────────────

function getContentBounds(canvas: OffscreenCanvas): Rect | null {
  const { width: w, height: h } = canvas
  const data = canvas.getContext('2d')!.getImageData(0, 0, w, h).data
  let minX = w, minY = h, maxX = -1, maxY = -1
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (data[(y * w + x) * 4 + 3] > 8) {
        if (x < minX) minX = x
        if (x + 1 > maxX) maxX = x + 1
        if (y < minY) minY = y
        if (y + 1 > maxY) maxY = y + 1
      }
    }
  }
  return maxX < 0 ? null : { x: minX, y: minY, w: maxX - minX, h: maxY - minY }
}

const HANDLE_SIZE = 10  // px at zoom=1
const ROT_HANDLE_DIST = 30  // px above top-center

function drawTransformBox(ctx: CanvasRenderingContext2D, xform: TransformState, zoom: number) {
  const { bounds: b, rotation } = xform
  const cx = b.x + b.w / 2, cy = b.y + b.h / 2
  const hs = HANDLE_SIZE / zoom
  const rotDist = ROT_HANDLE_DIST / zoom

  ctx.save()
  ctx.translate(cx, cy)
  ctx.rotate(rotation)

  // Border
  ctx.strokeStyle = '#1a7de0'
  ctx.lineWidth = 1.5 / zoom
  ctx.setLineDash([])
  ctx.strokeRect(-b.w / 2, -b.h / 2, b.w, b.h)

  // Handles (8)
  const pts = [
    { x: -b.w / 2, y: -b.h / 2 }, { x: 0, y: -b.h / 2 }, { x: b.w / 2, y: -b.h / 2 },
    { x: -b.w / 2, y: 0 },                                   { x: b.w / 2, y: 0 },
    { x: -b.w / 2, y: b.h / 2 },  { x: 0, y: b.h / 2 },  { x: b.w / 2, y: b.h / 2 },
  ]
  ctx.setLineDash([])
  for (const p of pts) {
    // shadow
    ctx.save()
    ctx.shadowColor = 'rgba(0,0,0,0.4)'
    ctx.shadowBlur = 4 / zoom
    ctx.shadowOffsetX = 1 / zoom
    ctx.shadowOffsetY = 1 / zoom
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(p.x - hs / 2, p.y - hs / 2, hs, hs)
    ctx.restore()
    ctx.strokeStyle = '#1a7de0'
    ctx.lineWidth = 1.5 / zoom
    ctx.strokeRect(p.x - hs / 2, p.y - hs / 2, hs, hs)
  }

  // Rotation handle connector + circle (clamp inside canvas)
  const rawRotY = -b.h / 2 - rotDist
  const rotY = Math.max(-(cy - 12 / zoom), rawRotY)  // don't go above canvas top
  ctx.strokeStyle = '#1a7de0'
  ctx.lineWidth = 1.5 / zoom
  ctx.beginPath(); ctx.moveTo(0, -b.h / 2); ctx.lineTo(0, rotY); ctx.stroke()

  ctx.save()
  ctx.shadowColor = 'rgba(0,0,0,0.4)'
  ctx.shadowBlur = 4 / zoom
  ctx.beginPath(); ctx.arc(0, rotY, 6 / zoom, 0, Math.PI * 2)
  ctx.fillStyle = '#ffffff'; ctx.fill()
  ctx.restore()
  ctx.strokeStyle = '#1a7de0'; ctx.lineWidth = 1.5 / zoom
  ctx.beginPath(); ctx.arc(0, rotY, 6 / zoom, 0, Math.PI * 2); ctx.stroke()

  ctx.restore()
}

function hitTransformBox(pos: { x: number; y: number }, xform: TransformState, zoom: number): string | null {
  const { bounds: b, rotation } = xform
  const cx = b.x + b.w / 2, cy = b.y + b.h / 2
  const cos = Math.cos(-rotation), sin = Math.sin(-rotation)
  const dx = pos.x - cx, dy = pos.y - cy
  const lx = dx * cos - dy * sin, ly = dx * sin + dy * cos
  const hs = HANDLE_SIZE / zoom / 2
  const rotDist = ROT_HANDLE_DIST / zoom

  // Rotation handle
  if (Math.hypot(lx, ly - (-b.h / 2 - rotDist)) < 10 / zoom) return 'rotate'

  // 8 handles
  const handles = [
    { lx: -b.w / 2, ly: -b.h / 2, id: 'tl' }, { lx: 0, ly: -b.h / 2, id: 'tc' }, { lx: b.w / 2, ly: -b.h / 2, id: 'tr' },
    { lx: -b.w / 2, ly: 0, id: 'ml' },                                                { lx: b.w / 2, ly: 0, id: 'mr' },
    { lx: -b.w / 2, ly: b.h / 2, id: 'bl' },  { lx: 0, ly: b.h / 2, id: 'bc' },  { lx: b.w / 2, ly: b.h / 2, id: 'br' },
  ]
  for (const h of handles) {
    if (Math.abs(lx - h.lx) <= hs && Math.abs(ly - h.ly) <= hs) return h.id
  }

  // Inside = move
  if (lx >= -b.w / 2 && lx <= b.w / 2 && ly >= -b.h / 2 && ly <= b.h / 2) return 'move'

  return null
}

function applyTransformToLayer(
  layerCanvas: OffscreenCanvas,
  snapshotCanvas: OffscreenCanvas,
  origBounds: Rect,
  newBounds: Rect,
  rotation: number,
) {
  const ctx = layerCanvas.getContext('2d')!
  ctx.clearRect(0, 0, layerCanvas.width, layerCanvas.height)
  const cx = newBounds.x + newBounds.w / 2, cy = newBounds.y + newBounds.h / 2
  ctx.save()
  ctx.translate(cx, cy)
  ctx.rotate(rotation)
  ctx.drawImage(snapshotCanvas, origBounds.x, origBounds.y, origBounds.w, origBounds.h,
    -newBounds.w / 2, -newBounds.h / 2, newBounds.w, newBounds.h)
  ctx.restore()
}

function getTransformCursor(hitId: string | null, isSpace: boolean): string {
  if (isSpace) return 'grab'
  switch (hitId) {
    case 'rotate': return 'crosshair'
    case 'tl': case 'br': return 'nwse-resize'
    case 'tr': case 'bl': return 'nesw-resize'
    case 'tc': case 'bc': return 'ns-resize'
    case 'ml': case 'mr': return 'ew-resize'
    case 'move': return 'move'
    default: return 'default'
  }
}

// ─── Text Overlay ─────────────────────────────────────────────────────────────

function TextOverlay({ pos, zoom, panX, panY, onCommit, onCancel }: {
  pos: { x: number; y: number }; zoom: number; panX: number; panY: number
  onCommit(text: string): void; onCancel(): void
}) {
  const [text, setText] = useState('')
  const ref = useRef<HTMLTextAreaElement>(null)
  const { brushSize, foregroundColor } = useImageEditor()
  useEffect(() => { ref.current?.focus() }, [])
  const screenX = pos.x * zoom + panX, screenY = pos.y * zoom + panY
  return (
    <div className="absolute pointer-events-none" style={{ left: 0, top: 0, width: '100%', height: '100%', zIndex: 10 }}>
      <textarea
        ref={ref} value={text} onChange={e => setText(e.target.value)}
        onKeyDown={e => {
          if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); onCommit(text) }
          if (e.key === 'Escape') onCancel()
          e.stopPropagation()
        }}
        onBlur={() => onCommit(text)}
        className="absolute pointer-events-auto resize-none outline-none bg-transparent border border-dashed border-primary/50 min-w-[4ch] p-1"
        style={{ left: screenX, top: screenY, color: foregroundColor, fontSize: Math.max(8, brushSize * zoom * 0.6), fontFamily: 'sans-serif', lineHeight: 1.4, zIndex: 20 }}
        rows={1} cols={10}
      />
    </div>
  )
}

// ─── Main Component ────────────────────────────────────────────────────────────

export function EditorCanvas() {
  const displayRef = useRef<HTMLCanvasElement>(null)
  const overlayRef = useRef<HTMLCanvasElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  // Generic interaction refs
  const isDrawingRef = useRef(false)
  const isSpaceRef = useRef(false)
  const isPanningRef = useRef(false)
  const lastPosRef = useRef<{ x: number; y: number } | null>(null)
  const panStartRef = useRef<{ x: number; y: number; px: number; py: number } | null>(null)

  // Tool-specific refs
  const shapeStartRef = useRef<{ x: number; y: number } | null>(null)
  const moveSnapshotRef = useRef<ImageData | null>(null)
  const moveStartRef = useRef<{ x: number; y: number } | null>(null)
  const selStartRef = useRef<{ x: number; y: number } | null>(null)
  const lassoPathRef = useRef<{ x: number; y: number }[]>([])
  const cropStartRef = useRef<{ x: number; y: number } | null>(null)
  const cropRectRef = useRef<Rect | null>(null)
  const marqueeDragRef = useRef<string | null>(null)
  const marqueeDragBaseRef = useRef<Rect | null>(null)

  // Transform refs
  const xformRef = useRef<TransformState | null>(null)
  const xformSnapshotRef = useRef<OffscreenCanvas | null>(null)
  const xformDragRef = useRef<{
    handle: string
    startBounds: Rect
    startPos: { x: number; y: number }
    startRotation: number
    startAngle?: number  // for rotation drags
  } | null>(null)

  const [textPos, setTextPos] = useState<{ x: number; y: number } | null>(null)
  const [cursor, setCursor] = useState('default')
  const [cursorInfo, setCursorInfo] = useState<{ x: number; y: number; color: string } | null>(null)
  const [containerSize, setContainerSize] = useState({ w: 0, h: 0 })
  // xformVersion triggers re-draw of overlay; setXformVersion stable ref avoids dep loops
  const [xformVersion, setXformVersion] = useState(0)
  const setXformVersionRef = useRef(setXformVersion)
  setXformVersionRef.current = setXformVersion
  const bumpXform = useCallback(() => setXformVersionRef.current(v => v + 1), [])

  const cloneOffsetRef = useRef<{ dx: number; dy: number } | null>(null)
  const gradientStartRef = useRef<{ x: number; y: number } | null>(null)

  const store = useImageEditor()
  const {
    layers, activeLayerId, activeTool,
    brushSize, brushHardness, brushOpacity,
    foregroundColor, backgroundColor, shapeType, polygonSides, gradientType,
    burnDodgeStrength,
    canvasWidth, canvasHeight,
    zoom, panX, panY,
    selection, penPoints, repaintVersion, historyVersion,
    cloneSource,
    setForegroundColor, setZoom, setPan, setSelection, setPenPoints, clearPen,
    setActiveLayer, setCloneSource,
    cropToRect, addLayer, updateLayerTextData,
    clearPendingDataUrl, pushHistory, requestRepaint,
  } = store

  // ── Load pending images ──────────────────────────────────────────────────
  useEffect(() => {
    for (const layer of layers) {
      if (!layer.pendingDataUrl) continue
      const pending = layer.pendingDataUrl
      const img = new Image()
      img.onload = () => {
        const iw = img.naturalWidth, ih = img.naturalHeight
        const isFirst = layers.length === 1
        const tw = isFirst ? iw : canvasWidth
        const th = isFirst ? ih : canvasHeight
        let c = layerCanvases.get(layer.id)
        if (!c || c.width !== tw || c.height !== th) {
          c = new OffscreenCanvas(tw, th); layerCanvases.set(layer.id, c)
        }
        const ctx = c.getContext('2d')!; ctx.clearRect(0, 0, tw, th)
        ctx.drawImage(img, Math.round((tw - iw) / 2), Math.round((th - ih) / 2))
        if (isFirst) useImageEditor.setState({ canvasWidth: tw, canvasHeight: th })
        clearPendingDataUrl(layer.id); pushHistory(); requestRepaint()
      }
      img.src = pending
    }
  }, [layers]) // eslint-disable-line react-hooks/exhaustive-deps

  // ── Composite ─────────────────────────────────────────────────────────────
  const composite = useCallback(() => {
    const dc = displayRef.current; if (!dc) return
    const ctx = dc.getContext('2d'); if (!ctx) return
    ctx.clearRect(0, 0, canvasWidth, canvasHeight)
    drawCheckerboard(ctx, canvasWidth, canvasHeight)
    for (const layer of layers) {
      if (!layer.visible) continue
      const lc = layerCanvases.get(layer.id)
      if (!lc || lc.width !== canvasWidth || lc.height !== canvasHeight) continue
      ctx.save()
      ctx.globalAlpha = layer.opacity / 100
      ctx.globalCompositeOperation = layer.blendMode as GlobalCompositeOperation
      ctx.drawImage(lc, 0, 0)
      ctx.restore()
    }
  }, [layers, canvasWidth, canvasHeight])

  // ── Overlay ───────────────────────────────────────────────────────────────
  const drawOverlay = useCallback((opts?: {
    sel?: typeof selection
    lassoPath?: { x: number; y: number }[]
    shapePreview?: { type: string; x0: number; y0: number; x1: number; y1: number; sides?: number }
    cropRect?: Rect
    penPts?: { x: number; y: number }[]
    penCursor?: { x: number; y: number }
  }) => {
    const oc = overlayRef.current; if (!oc) return
    const ctx = oc.getContext('2d'); if (!ctx) return
    ctx.clearRect(0, 0, canvasWidth, canvasHeight)

    const lw = 1.5 / zoom
    const dash: [number, number] = [5 / zoom, 3 / zoom]

    // Transform box (select tool)
    const xf = xformRef.current
    if (xf && activeTool === 'select') {
      drawTransformBox(ctx, xf, zoom)
    }

    // Marquee selection rect
    const sel = opts?.sel ?? selection
    if (sel && activeTool !== 'select') {
      ctx.save()
      ctx.strokeStyle = '#0099ff'
      ctx.lineWidth = lw
      ctx.setLineDash(dash)
      ctx.strokeRect(sel.x, sel.y, sel.w, sel.h)
      ctx.setLineDash([])
      // corner handles for marquee when not actively drawing
      if (!opts?.sel) {
        const hs = 6 / zoom
        const corners = [
          { x: sel.x, y: sel.y }, { x: sel.x + sel.w, y: sel.y },
          { x: sel.x, y: sel.y + sel.h }, { x: sel.x + sel.w, y: sel.y + sel.h },
        ]
        ctx.fillStyle = '#0099ff'
        for (const c of corners) ctx.fillRect(c.x - hs / 2, c.y - hs / 2, hs, hs)
      }
      ctx.restore()
    }

    // Lasso
    const lp = opts?.lassoPath
    if (lp && lp.length > 1) {
      ctx.save(); ctx.strokeStyle = '#0099ff'; ctx.lineWidth = lw; ctx.setLineDash(dash)
      ctx.beginPath(); ctx.moveTo(lp[0].x, lp[0].y)
      for (const p of lp.slice(1)) ctx.lineTo(p.x, p.y)
      ctx.stroke(); ctx.restore()
    }

    // Crop overlay
    const cr = opts?.cropRect ?? (activeTool === 'crop' ? cropRectRef.current : null)
    if (cr) {
      ctx.save()
      ctx.fillStyle = 'rgba(0,0,0,0.45)'
      ctx.fillRect(0, 0, canvasWidth, canvasHeight)
      ctx.clearRect(cr.x, cr.y, cr.w, cr.h)
      ctx.strokeStyle = '#fff'; ctx.lineWidth = 1.5 / zoom; ctx.setLineDash([])
      ctx.strokeRect(cr.x, cr.y, cr.w, cr.h)
      // Rule of thirds guides
      ctx.strokeStyle = 'rgba(255,255,255,0.3)'; ctx.lineWidth = 0.5 / zoom
      for (let i = 1; i <= 2; i++) {
        ctx.beginPath(); ctx.moveTo(cr.x + cr.w * i / 3, cr.y); ctx.lineTo(cr.x + cr.w * i / 3, cr.y + cr.h); ctx.stroke()
        ctx.beginPath(); ctx.moveTo(cr.x, cr.y + cr.h * i / 3); ctx.lineTo(cr.x + cr.w, cr.y + cr.h * i / 3); ctx.stroke()
      }
      // Handles
      const hs = 8 / zoom
      const handles = [
        { x: cr.x, y: cr.y }, { x: cr.x + cr.w / 2, y: cr.y }, { x: cr.x + cr.w, y: cr.y },
        { x: cr.x, y: cr.y + cr.h / 2 }, { x: cr.x + cr.w, y: cr.y + cr.h / 2 },
        { x: cr.x, y: cr.y + cr.h }, { x: cr.x + cr.w / 2, y: cr.y + cr.h }, { x: cr.x + cr.w, y: cr.y + cr.h },
      ]
      ctx.fillStyle = '#fff'; ctx.strokeStyle = 'rgba(0,0,0,0.3)'; ctx.lineWidth = 0.5 / zoom
      for (const p of handles) { ctx.fillRect(p.x - hs / 2, p.y - hs / 2, hs, hs); ctx.strokeRect(p.x - hs / 2, p.y - hs / 2, hs, hs) }
      ctx.restore()
    }

    // Shape preview
    const sp = opts?.shapePreview
    if (sp) {
      ctx.save(); ctx.strokeStyle = foregroundColor; ctx.lineWidth = (brushSize * 0.15 + 1) / zoom; ctx.setLineDash([])
      if (sp.type === 'rect') ctx.strokeRect(sp.x0, sp.y0, sp.x1 - sp.x0, sp.y1 - sp.y0)
      else if (sp.type === 'ellipse') {
        const cx = (sp.x0 + sp.x1) / 2, cy = (sp.y0 + sp.y1) / 2
        ctx.beginPath(); ctx.ellipse(cx, cy, Math.abs(sp.x1 - sp.x0) / 2, Math.abs(sp.y1 - sp.y0) / 2, 0, 0, Math.PI * 2); ctx.stroke()
      } else if (sp.type === 'triangle') { drawTriangle(ctx, sp.x0, sp.y0, sp.x1, sp.y1); ctx.stroke() }
      else if (sp.type === 'polygon') {
        const cx = (sp.x0 + sp.x1) / 2, cy = (sp.y0 + sp.y1) / 2
        drawPolygon(ctx, cx, cy, Math.abs(sp.x1 - sp.x0) / 2, Math.abs(sp.y1 - sp.y0) / 2, sp.sides ?? 5); ctx.stroke()
      }
      ctx.restore()
    }

    // Pen path
    const pp = opts?.penPts ?? penPoints
    if (pp && pp.length > 0 && activeTool === 'pen') {
      ctx.save(); ctx.strokeStyle = '#0099ff'; ctx.lineWidth = lw; ctx.setLineDash([])
      ctx.beginPath(); ctx.moveTo(pp[0].x, pp[0].y)
      for (const p of pp.slice(1)) ctx.lineTo(p.x, p.y)
      if (opts?.penCursor) ctx.lineTo(opts.penCursor.x, opts.penCursor.y)
      ctx.stroke()
      ctx.fillStyle = '#0099ff'
      for (const p of pp) { ctx.beginPath(); ctx.arc(p.x, p.y, 3 / zoom, 0, Math.PI * 2); ctx.fill() }
      if (opts?.penCursor && pp.length > 2) {
        const d = Math.hypot(opts.penCursor.x - pp[0].x, opts.penCursor.y - pp[0].y)
        if (d < 10 / zoom) { ctx.beginPath(); ctx.arc(pp[0].x, pp[0].y, 8 / zoom, 0, Math.PI * 2); ctx.stroke() }
      }
      ctx.restore()
    }
  }, [selection, penPoints, activeTool, canvasWidth, canvasHeight, zoom, foregroundColor, brushSize, xformVersion])

  useEffect(() => { composite() }, [composite, repaintVersion])
  useEffect(() => { drawOverlay() }, [drawOverlay])

  // ── Helpers ───────────────────────────────────────────────────────────────
  const clientToCanvas = useCallback((e: PointerEvent | React.PointerEvent): { x: number; y: number } => {
    const dc = displayRef.current; if (!dc) return { x: 0, y: 0 }
    const rect = dc.getBoundingClientRect()
    return { x: (e.clientX - rect.left) * (canvasWidth / rect.width), y: (e.clientY - rect.top) * (canvasHeight / rect.height) }
  }, [canvasWidth, canvasHeight])

  const getActiveCtx = useCallback((): OffscreenCanvasRenderingContext2D | null => {
    if (!activeLayerId) return null
    const layer = layers.find(l => l.id === activeLayerId)
    if (!layer || layer.locked) return null
    return layerCanvases.get(activeLayerId)?.getContext('2d') ?? null
  }, [activeLayerId, layers])

  // ── Crop init ─────────────────────────────────────────────────────────────
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (activeTool === 'crop' && !cropRectRef.current) {
      cropRectRef.current = { x: 0, y: 0, w: canvasWidth, h: canvasHeight }
    }
    if (activeTool !== 'crop') cropRectRef.current = null
    if (activeTool !== 'select') { xformRef.current = null; xformSnapshotRef.current = null }
    setXformVersionRef.current(v => v + 1)
  }, [activeTool, canvasWidth, canvasHeight])

  // ── Clear transform on undo/redo ──────────────────────────────────────────
  useEffect(() => {
    xformRef.current = null; xformSnapshotRef.current = null; xformDragRef.current = null
    setXformVersionRef.current(v => v + 1)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [historyVersion])

  // ── Pointer Down ──────────────────────────────────────────────────────────
  const onPointerDown = useCallback((e: PointerEvent) => {
    e.preventDefault()
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)

    if (e.button === 1 || isSpaceRef.current) {
      isPanningRef.current = true
      panStartRef.current = { x: e.clientX, y: e.clientY, px: panX, py: panY }
      return
    }
    if (e.button !== 0) return

    const pos = clientToCanvas(e)
    isDrawingRef.current = true
    lastPosRef.current = pos

    // ── eyedropper ──
    if (activeTool === 'eyedropper') {
      const dc = displayRef.current; if (!dc) return
      const d = dc.getContext('2d')!.getImageData(Math.floor(pos.x), Math.floor(pos.y), 1, 1).data
      setForegroundColor('#' + [d[0], d[1], d[2]].map(v => v.toString(16).padStart(2, '0')).join(''))
      isDrawingRef.current = false; return
    }

    // ── fill ──
    if (activeTool === 'fill') {
      const ctx = getActiveCtx(); if (ctx) { pushHistory(); floodFill(ctx.canvas as OffscreenCanvas, pos.x, pos.y, foregroundColor); composite() }
      isDrawingRef.current = false; return
    }

    // ── move ──
    if (activeTool === 'move') {
      const c = activeLayerId ? layerCanvases.get(activeLayerId) : null
      const layer = layers.find(l => l.id === activeLayerId)
      if (c && layer && !layer.locked) {
        const ctx = c.getContext('2d')!
        moveSnapshotRef.current = ctx.getImageData(0, 0, canvasWidth, canvasHeight)
        moveStartRef.current = pos; pushHistory()
      }
      return
    }

    // ── select (pointer tool with transform) ──
    if (activeTool === 'select') {
      const xf = xformRef.current
      if (xf) {
        const hit = hitTransformBox(pos, xf, zoom)
        if (hit) {
          xformDragRef.current = {
            handle: hit, startBounds: { ...xf.bounds },
            startPos: pos, startRotation: xf.rotation,
            ...(hit === 'rotate' ? {
              startAngle: Math.atan2(pos.y - (xf.bounds.y + xf.bounds.h / 2), pos.x - (xf.bounds.x + xf.bounds.w / 2))
            } : {}),
          }
          if (hit !== 'rotate' && hit !== 'move') {
            // Start scale: capture snapshot
            const c = layerCanvases.get(xf.layerId)
            if (c) {
              const snap = new OffscreenCanvas(c.width, c.height)
              snap.getContext('2d')!.drawImage(c, 0, 0)
              xformSnapshotRef.current = snap
              pushHistory()
            }
          } else if (hit === 'move') {
            const c = activeLayerId ? layerCanvases.get(activeLayerId) : null
            if (c) {
              const snap = new OffscreenCanvas(c.width, c.height)
              snap.getContext('2d')!.drawImage(c, 0, 0)
              xformSnapshotRef.current = snap
              pushHistory()
            }
          }
          return
        }
      }

      // Click on canvas: show transform for active layer (PS "Auto-Select OFF" behavior).
      // Only switch layers if clicking a non-transparent pixel on a DIFFERENT layer.
      const x = Math.floor(pos.x), y = Math.floor(pos.y)
      const activeCvs = activeLayerId ? layerCanvases.get(activeLayerId) : null
      const activeLayer = layers.find(l => l.id === activeLayerId)

      // Check if click hits a different (higher) layer first
      let switchedLayer = false
      for (let i = layers.length - 1; i >= 0; i--) {
        const l = layers[i]
        if (!l.visible || l.id === activeLayerId) continue
        const c = layerCanvases.get(l.id); if (!c) continue
        const px = c.getContext('2d')!.getImageData(Math.max(0, Math.min(c.width - 1, x)), Math.max(0, Math.min(c.height - 1, y)), 1, 1).data
        if (px[3] > 8 && layers.indexOf(l) > layers.indexOf(activeLayer!)) {
          // Only auto-switch to a layer above the current one
          setActiveLayer(l.id)
          const bounds = getContentBounds(c) ?? { x: 0, y: 0, w: canvasWidth, h: canvasHeight }
          xformRef.current = { layerId: l.id, bounds, rotation: 0, originalBounds: bounds }
          bumpXform()
          switchedLayer = true
          break
        }
      }

      if (!switchedLayer && activeCvs && activeLayer?.visible) {
        // Show transform on active layer regardless of where click landed
        const bounds = getContentBounds(activeCvs) ?? { x: 0, y: 0, w: canvasWidth, h: canvasHeight }
        xformRef.current = { layerId: activeLayerId!, bounds, rotation: 0, originalBounds: bounds }
        bumpXform()
      } else if (!switchedLayer) {
        xformRef.current = null; bumpXform()
      }
      return
    }

    // ── marquee ──
    if (activeTool === 'marquee') {
      if (selection) {
        // Check existing selection resize/move handles
        const hs = 8 / zoom
        const corners = [
          { x: selection.x, y: selection.y, id: 'tl' }, { x: selection.x + selection.w, y: selection.y, id: 'tr' },
          { x: selection.x, y: selection.y + selection.h, id: 'bl' }, { x: selection.x + selection.w, y: selection.y + selection.h, id: 'br' },
        ]
        const hit = corners.find(c => Math.abs(pos.x - c.x) < hs && Math.abs(pos.y - c.y) < hs)
        if (hit) { marqueeDragRef.current = hit.id; marqueeDragBaseRef.current = { ...selection }; return }
        // Inside selection = move
        if (pos.x >= selection.x && pos.x <= selection.x + selection.w && pos.y >= selection.y && pos.y <= selection.y + selection.h) {
          marqueeDragRef.current = 'move'; marqueeDragBaseRef.current = { ...selection }; return
        }
      }
      selStartRef.current = pos; setSelection(null); return
    }

    // ── lasso ──
    if (activeTool === 'lasso') { lassoPathRef.current = [pos]; return }

    // ── crop ──
    if (activeTool === 'crop') {
      const cr = cropRectRef.current
      if (cr) {
        const hs = 10 / zoom
        const handles = [
          { x: cr.x, y: cr.y, id: 'tl' }, { x: cr.x + cr.w, y: cr.y, id: 'tr' },
          { x: cr.x, y: cr.y + cr.h, id: 'bl' }, { x: cr.x + cr.w, y: cr.y + cr.h, id: 'br' },
          { x: cr.x + cr.w / 2, y: cr.y, id: 'tc' }, { x: cr.x + cr.w / 2, y: cr.y + cr.h, id: 'bc' },
          { x: cr.x, y: cr.y + cr.h / 2, id: 'ml' }, { x: cr.x + cr.w, y: cr.y + cr.h / 2, id: 'mr' },
        ]
        const hit = handles.find(h => Math.abs(pos.x - h.x) < hs && Math.abs(pos.y - h.y) < hs)
        if (hit) { marqueeDragRef.current = hit.id; marqueeDragBaseRef.current = { ...cr }; return }
        if (pos.x >= cr.x && pos.x <= cr.x + cr.w && pos.y >= cr.y && pos.y <= cr.y + cr.h) {
          marqueeDragRef.current = 'move'; marqueeDragBaseRef.current = { ...cr }; return
        }
      }
      cropStartRef.current = pos
      cropRectRef.current = { x: pos.x, y: pos.y, w: 0, h: 0 }
      return
    }

    // ── pen ──
    if (activeTool === 'pen') {
      if (penPoints.length > 2) {
        const d = Math.hypot(pos.x - penPoints[0].x, pos.y - penPoints[0].y)
        if (d < 10 / zoom) { setPenPoints(penPoints, true); drawOverlay({ penPts: penPoints }); return }
      }
      setPenPoints([...penPoints, pos]); return
    }

    // ── text ──
    if (activeTool === 'text') { setTextPos(pos); isDrawingRef.current = false; return }

    // ── shape ──
    if (activeTool === 'shape') { shapeStartRef.current = pos; pushHistory(); return }

    // ── clone stamp ──
    if (activeTool === 'clone') {
      if (e.altKey) {
        // Alt+click = sample source point
        setCloneSource({ x: pos.x, y: pos.y, layerId: activeLayerId ?? '' })
        cloneOffsetRef.current = null
        return
      }
      if (!cloneSource) return
      const ctx = getActiveCtx(); if (!ctx) return
      const srcCvs = layerCanvases.get(cloneSource.layerId); if (!srcCvs) return
      if (!cloneOffsetRef.current) {
        cloneOffsetRef.current = { dx: pos.x - cloneSource.x, dy: pos.y - cloneSource.y }
        pushHistory()
      }
      const { dx, dy } = cloneOffsetRef.current
      const srcX = pos.x - dx, srcY = pos.y - dy
      const r = brushSize / 2
      const cloneTmp = new OffscreenCanvas(brushSize, brushSize)
      const cCtx = cloneTmp.getContext('2d')!
      cCtx.drawImage(srcCvs, srcX - r, srcY - r, brushSize, brushSize, 0, 0, brushSize, brushSize)
      const grad = ctx.createRadialGradient(brushSize / 2, brushSize / 2, 0, brushSize / 2, brushSize / 2, r)
      grad.addColorStop(Math.min(brushHardness / 100, 0.99), 'rgba(0,0,0,1)')
      grad.addColorStop(1, 'rgba(0,0,0,0)')
      cCtx.globalCompositeOperation = 'destination-in'
      cCtx.fillStyle = grad; cCtx.fillRect(0, 0, brushSize, brushSize)
      ctx.save(); ctx.globalAlpha = brushOpacity / 100
      ctx.drawImage(cloneTmp, pos.x - r, pos.y - r)
      ctx.restore()
      composite()
      return
    }

    // ── gradient ──
    if (activeTool === 'gradient') {
      gradientStartRef.current = pos; pushHistory(); return
    }

    // ── dodge / burn ──
    if (activeTool === 'dodge' || activeTool === 'burn') {
      const ctx = getActiveCtx(); if (!ctx) return
      pushHistory()
      dodgeBurnDot(ctx, pos.x, pos.y, brushSize, burnDodgeStrength, activeTool === 'dodge')
      composite()
      return
    }

    // ── brush / eraser ──
    const ctx = getActiveCtx()
    if (ctx) {
      pushHistory()
      drawBrushDot(ctx, pos.x, pos.y, brushSize, brushHardness, foregroundColor, brushOpacity, activeTool === 'eraser')
      composite()
    }
  }, [
    activeTool, foregroundColor, backgroundColor, brushSize, brushHardness, brushOpacity,
    burnDodgeStrength, gradientType,
    panX, panY, canvasWidth, canvasHeight, zoom, activeLayerId, layers,
    selection, penPoints, composite, drawOverlay, pushHistory,
    cloneSource, setCloneSource,
    setForegroundColor, setSelection, setPenPoints, setActiveLayer, getActiveCtx, clientToCanvas, bumpXform,
  ])

  // ── Pointer Move ──────────────────────────────────────────────────────────
  const onPointerMove = useCallback((e: PointerEvent) => {
    e.preventDefault()

    if (isPanningRef.current && panStartRef.current) {
      setPan(panStartRef.current.px + e.clientX - panStartRef.current.x, panStartRef.current.py + e.clientY - panStartRef.current.y)
      return
    }

    const pos = clientToCanvas(e)

    // Update cursor
    if (activeTool === 'select') {
      const xf = xformRef.current
      const hit = xf ? hitTransformBox(pos, xf, zoom) : null
      setCursor(getTransformCursor(hit, false))
    } else {
      setCursor('default')
    }

    // Pen preview
    if (activeTool === 'pen' && !isDrawingRef.current) {
      drawOverlay({ penCursor: pos }); return
    }

    if (!isDrawingRef.current) return

    // ── Move tool ──
    if (activeTool === 'move' && moveSnapshotRef.current && moveStartRef.current) {
      const dx = Math.round(pos.x - moveStartRef.current.x), dy = Math.round(pos.y - moveStartRef.current.y)
      const c = activeLayerId ? layerCanvases.get(activeLayerId) : null
      if (c) {
        const ctx = c.getContext('2d')!
        ctx.clearRect(0, 0, canvasWidth, canvasHeight)
        ctx.putImageData(new ImageData(moveSnapshotRef.current.data.slice(), moveSnapshotRef.current.width, moveSnapshotRef.current.height), dx, dy)
        composite()
      }
      return
    }

    // ── Select transform drag ──
    if (activeTool === 'select' && xformDragRef.current && xformRef.current) {
      const { handle, startBounds, startPos, startRotation, startAngle } = xformDragRef.current
      const xf = xformRef.current

      if (handle === 'rotate') {
        const cx = xf.bounds.x + xf.bounds.w / 2, cy = xf.bounds.y + xf.bounds.h / 2
        const currentAngle = Math.atan2(pos.y - cy, pos.x - cx)
        const delta = currentAngle - (startAngle ?? 0)
        xf.rotation = startRotation + delta
        bumpXform()
        // Apply rotation
        const snap = xformSnapshotRef.current ?? layerCanvases.get(xf.layerId)
        if (snap) {
          const layerC = layerCanvases.get(xf.layerId)
          if (layerC) {
            applyTransformToLayer(layerC, snap, xf.originalBounds, xf.bounds, xf.rotation)
            composite()
          }
        }
        return
      }

      if (handle === 'move') {
        const dx = pos.x - startPos.x, dy = pos.y - startPos.y
        xf.bounds = { ...startBounds, x: startBounds.x + dx, y: startBounds.y + dy }
        // originalBounds stays fixed — it's the source content location in the snapshot
        bumpXform()
        const snap = xformSnapshotRef.current
        if (snap) {
          const layerC = layerCanvases.get(xf.layerId)
          if (layerC) {
            applyTransformToLayer(layerC, snap, xf.originalBounds, xf.bounds, xf.rotation)
            composite()
          }
        }
        return
      }

      // Scale handle: compute delta in local (unrotated) space
      const cos = Math.cos(-startRotation), sin = Math.sin(-startRotation)
      const dx = pos.x - startPos.x, dy = pos.y - startPos.y
      const ldx = dx * cos - dy * sin, ldy = dx * sin + dy * cos

      let { x, y, w, h } = startBounds
      if (handle.includes('r')) w = Math.max(10, w + ldx)
      if (handle.includes('l')) { x += ldx; w = Math.max(10, w - ldx) }
      if (handle.includes('b')) h = Math.max(10, h + ldy)
      if (handle.includes('t')) { y += ldy; h = Math.max(10, h - ldy) }

      xf.bounds = { x, y, w, h }
      bumpXform()
      const snap = xformSnapshotRef.current
      if (snap) {
        const layerC = layerCanvases.get(xf.layerId)
        if (layerC) {
          applyTransformToLayer(layerC, snap, xf.originalBounds, xf.bounds, xf.rotation)
          composite()
        }
      }
      return
    }

    // ── Marquee drag ──
    if (activeTool === 'marquee') {
      if (marqueeDragRef.current && marqueeDragBaseRef.current && lastPosRef.current) {
        const base = marqueeDragBaseRef.current
        const dx = pos.x - lastPosRef.current.x, dy = pos.y - lastPosRef.current.y
        let { x, y, w, h } = base
        const id = marqueeDragRef.current
        if (id === 'move') { setSelection({ x: x + dx, y: y + dy, w, h }); lastPosRef.current = pos; return }
        if (id.includes('r')) w = Math.max(2, w + dx); if (id.includes('l')) { x += dx; w = Math.max(2, w - dx) }
        if (id.includes('b')) h = Math.max(2, h + dy); if (id.includes('t')) { y += dy; h = Math.max(2, h - dy) }
        setSelection({ x, y, w, h }); lastPosRef.current = pos; return
      }
      if (selStartRef.current) {
        const x0 = Math.min(selStartRef.current.x, pos.x), y0 = Math.min(selStartRef.current.y, pos.y)
        const x1 = Math.max(selStartRef.current.x, pos.x), y1 = Math.max(selStartRef.current.y, pos.y)
        drawOverlay({ sel: { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } }); return
      }
    }

    // ── Lasso ──
    if (activeTool === 'lasso') { lassoPathRef.current.push(pos); drawOverlay({ lassoPath: lassoPathRef.current }); return }

    // ── Crop drag ──
    if (activeTool === 'crop') {
      if (marqueeDragRef.current && marqueeDragBaseRef.current && lastPosRef.current) {
        const base = marqueeDragBaseRef.current
        const dx = pos.x - lastPosRef.current.x, dy = pos.y - lastPosRef.current.y
        let { x, y, w, h } = base
        const id = marqueeDragRef.current
        if (id === 'move') { x += dx; y += dy }
        else {
          if (id.includes('r')) w = Math.max(10, w + dx); if (id.includes('l')) { x += dx; w = Math.max(10, w - dx) }
          if (id.includes('b')) h = Math.max(10, h + dy); if (id.includes('t')) { y += dy; h = Math.max(10, h - dy) }
          if (id === 'tc' || id === 'bc') { x = base.x; w = base.w }
          if (id === 'ml' || id === 'mr') { y = base.y; h = base.h }
        }
        cropRectRef.current = { x, y, w: Math.max(10, w), h: Math.max(10, h) }
        lastPosRef.current = pos; drawOverlay(); return
      }
      if (cropStartRef.current) {
        const x0 = Math.min(cropStartRef.current.x, pos.x), y0 = Math.min(cropStartRef.current.y, pos.y)
        const x1 = Math.max(cropStartRef.current.x, pos.x), y1 = Math.max(cropStartRef.current.y, pos.y)
        cropRectRef.current = { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }; drawOverlay(); return
      }
    }

    // ── Shape preview ──
    if (activeTool === 'shape' && shapeStartRef.current) {
      drawOverlay({ shapePreview: { type: shapeType, x0: shapeStartRef.current.x, y0: shapeStartRef.current.y, x1: pos.x, y1: pos.y, sides: polygonSides } }); return
    }

    // ── Clone ──
    if (activeTool === 'clone' && isDrawingRef.current && cloneSource && cloneOffsetRef.current) {
      const ctx = getActiveCtx(); if (ctx && lastPosRef.current) {
        const { dx, dy } = cloneOffsetRef.current
        const srcCvs = layerCanvases.get(cloneSource.layerId)
        if (srcCvs) {
          const r = brushSize / 2
          const srcX = pos.x - dx, srcY = pos.y - dy
          const cloneTmp = new OffscreenCanvas(brushSize, brushSize)
          const cCtx = cloneTmp.getContext('2d')!
          cCtx.drawImage(srcCvs, srcX - r, srcY - r, brushSize, brushSize, 0, 0, brushSize, brushSize)
          const grad = ctx.createRadialGradient(r, r, 0, r, r, r)
          grad.addColorStop(Math.min(brushHardness / 100, 0.99), 'rgba(0,0,0,1)')
          grad.addColorStop(1, 'rgba(0,0,0,0)')
          cCtx.globalCompositeOperation = 'destination-in'
          cCtx.fillStyle = grad; cCtx.fillRect(0, 0, brushSize, brushSize)
          ctx.save(); ctx.globalAlpha = brushOpacity / 100
          ctx.drawImage(cloneTmp, pos.x - r, pos.y - r)
          ctx.restore()
          composite()
        }
      }
    }

    // ── Gradient ──
    if (activeTool === 'gradient' && gradientStartRef.current) {
      drawOverlay({ shapePreview: { type: 'line' as never, x0: gradientStartRef.current.x, y0: gradientStartRef.current.y, x1: pos.x, y1: pos.y, sides: 0 } })
    }

    // ── Dodge / Burn ──
    if ((activeTool === 'dodge' || activeTool === 'burn') && isDrawingRef.current && lastPosRef.current) {
      const ctx = getActiveCtx()
      if (ctx) { dodgeBurnDot(ctx, pos.x, pos.y, brushSize, burnDodgeStrength, activeTool === 'dodge'); composite() }
    }

    // ── Brush / Eraser ──
    if ((activeTool === 'brush' || activeTool === 'eraser') && lastPosRef.current) {
      const ctx = getActiveCtx()
      if (ctx) { drawBrushSegment(ctx, lastPosRef.current.x, lastPosRef.current.y, pos.x, pos.y, brushSize, brushHardness, foregroundColor, brushOpacity, activeTool === 'eraser'); composite() }
    }

    lastPosRef.current = pos
  }, [
    activeTool, foregroundColor, brushSize, brushHardness, brushOpacity,
    burnDodgeStrength, cloneSource,
    canvasWidth, canvasHeight, zoom, activeLayerId, selection, penPoints, shapeType, polygonSides,
    composite, drawOverlay, setPan, setSelection, getActiveCtx, clientToCanvas, bumpXform,
  ])

  // ── Pointer Up ────────────────────────────────────────────────────────────
  const onPointerUp = useCallback((e: PointerEvent) => {
    if (isPanningRef.current) { isPanningRef.current = false; panStartRef.current = null; return }
    const pos = clientToCanvas(e)

    if (activeTool === 'select') {
      // Update originalBounds after any transform op
      if (xformRef.current && xformDragRef.current) {
        const xf = xformRef.current
        if (xformDragRef.current.handle === 'move') {
          // update original bounds to current position
          const c = layerCanvases.get(xf.layerId)
          if (c) { xf.originalBounds = getContentBounds(c) ?? xf.bounds }
          xformSnapshotRef.current = null
        } else if (xformDragRef.current.handle !== 'rotate') {
          // After scale: rescan content bounds
          const c = layerCanvases.get(xf.layerId)
          if (c) { xf.originalBounds = xf.bounds }
          xformSnapshotRef.current = null
        }
      }
      xformDragRef.current = null
      bumpXform()
    }

    if (activeTool === 'marquee') {
      if (selStartRef.current) {
        const x0 = Math.min(selStartRef.current.x, pos.x), y0 = Math.min(selStartRef.current.y, pos.y)
        const x1 = Math.max(selStartRef.current.x, pos.x), y1 = Math.max(selStartRef.current.y, pos.y)
        if (x1 - x0 > 2 && y1 - y0 > 2) setSelection({ x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
        else setSelection(null)
        selStartRef.current = null
      }
      marqueeDragRef.current = null; marqueeDragBaseRef.current = null
    }

    if (activeTool === 'lasso' && lassoPathRef.current.length > 3) {
      const path = lassoPathRef.current
      const xs = path.map(p => p.x), ys = path.map(p => p.y)
      setSelection({ x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) })
      lassoPathRef.current = []; drawOverlay()
    }

    if (activeTool === 'crop') {
      cropStartRef.current = null; marqueeDragRef.current = null; marqueeDragBaseRef.current = null; drawOverlay()
    }

    if (activeTool === 'shape' && shapeStartRef.current) {
      const ctx = getActiveCtx()
      if (ctx) {
        const { x: x0, y: y0 } = shapeStartRef.current; const x1 = pos.x, y1 = pos.y
        const lw = Math.max(1, brushSize * 0.15)
        ctx.save(); ctx.strokeStyle = foregroundColor; ctx.lineWidth = lw; ctx.globalAlpha = brushOpacity / 100; ctx.fillStyle = 'transparent'
        if (shapeType === 'rect') ctx.strokeRect(x0, y0, x1 - x0, y1 - y0)
        else if (shapeType === 'ellipse') {
          const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2
          ctx.beginPath(); ctx.ellipse(cx, cy, Math.abs(x1 - x0) / 2, Math.abs(y1 - y0) / 2, 0, 0, Math.PI * 2); ctx.stroke()
        } else if (shapeType === 'triangle') { drawTriangle(ctx, x0, y0, x1, y1); ctx.stroke() }
        else if (shapeType === 'polygon') {
          const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2
          drawPolygon(ctx, cx, cy, Math.abs(x1 - x0) / 2, Math.abs(y1 - y0) / 2, polygonSides); ctx.stroke()
        }
        ctx.restore(); composite()
      }
      shapeStartRef.current = null; drawOverlay()
    }

    // ── Gradient apply ──
    if (activeTool === 'gradient' && gradientStartRef.current) {
      const ctx = getActiveCtx()
      if (ctx) {
        const { x: x0, y: y0 } = gradientStartRef.current; const x1 = pos.x, y1 = pos.y
        const cvs = ctx.canvas as OffscreenCanvas
        const g = ctx.createLinearGradient(x0, y0, x1, y1)
        g.addColorStop(0, foregroundColor); g.addColorStop(1, backgroundColor)
        ctx.fillStyle = g; ctx.fillRect(0, 0, cvs.width, cvs.height)
        composite()
      }
      gradientStartRef.current = null; drawOverlay()
    }

    // ── Clone: reset offset on up (ready for next stroke) ──
    if (activeTool === 'clone') cloneOffsetRef.current = null

    isDrawingRef.current = false
    moveSnapshotRef.current = null; moveStartRef.current = null; lastPosRef.current = null
  }, [activeTool, foregroundColor, backgroundColor, brushSize, brushOpacity, shapeType, polygonSides, composite, drawOverlay, setSelection, getActiveCtx, clientToCanvas, bumpXform])

  // ── Wheel zoom ────────────────────────────────────────────────────────────
  const onWheel = useCallback((e: WheelEvent) => {
    e.preventDefault()
    const f = e.deltaY < 0 ? 1.12 : 0.88
    const newZoom = Math.max(0.05, Math.min(32, zoom * f))
    const cont = containerRef.current; if (!cont) return
    const rect = cont.getBoundingClientRect()
    const cx = e.clientX - rect.left, cy = e.clientY - rect.top
    setPan(cx - ((cx - panX) / zoom) * newZoom, cy - ((cy - panY) / zoom) * newZoom)
    setZoom(newZoom)
  }, [zoom, panX, panY, setZoom, setPan])

  // ── Text commit ───────────────────────────────────────────────────────────
  const commitText = useCallback((text: string) => {
    setTextPos(null)
    if (!text.trim()) return
    const layerId = addLayer({ name: 'Text Layer' })
    const canvas = layerCanvases.get(layerId)
    if (!canvas) return
    const ctx = canvas.getContext('2d')!
    const fontSize = Math.max(8, brushSize * 0.6)
    ctx.save(); ctx.font = `${fontSize}px sans-serif`; ctx.fillStyle = foregroundColor
    ctx.fillText(text, textPos?.x ?? 0, textPos?.y ?? 0); ctx.restore()
    updateLayerTextData(layerId, { text, font: 'sans-serif', fontSize, color: foregroundColor, bold: false, italic: false, x: textPos?.x ?? 0, y: textPos?.y ?? 0 })
    pushHistory(); composite()
  }, [addLayer, brushSize, foregroundColor, textPos, composite, pushHistory, updateLayerTextData])

  // ── Key handlers ─────────────────────────────────────────────────────────
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).tagName === 'INPUT' || (e.target as HTMLElement).tagName === 'TEXTAREA') return
      if (e.code === 'Space' && !e.repeat) { isSpaceRef.current = true; e.preventDefault() }
      if (e.key === 'Escape') {
        clearPen(); setSelection(null); setTextPos(null)
        xformRef.current = null; bumpXform()
        if (activeTool === 'crop') { cropRectRef.current = null; drawOverlay() }
      }
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key === 'z') { e.preventDefault(); useImageEditor.getState().undo() }
      if ((e.metaKey || e.ctrlKey) && (e.key === 'y' || (e.shiftKey && e.key === 'z'))) { e.preventDefault(); useImageEditor.getState().redo() }
      if (e.key === 'Enter' && activeTool === 'crop' && cropRectRef.current) {
        const { x, y, w, h } = cropRectRef.current
        cropToRect(x, y, w, h); cropRectRef.current = null; drawOverlay()
      }
      // Commit transform with Enter
      if (e.key === 'Enter' && activeTool === 'select' && xformRef.current) {
        xformRef.current = null; xformSnapshotRef.current = null; bumpXform()
      }
      if (e.key === 'Delete' || e.key === 'Backspace') {
        if (activeTool === 'pen') { const pts = [...penPoints]; pts.pop(); setPenPoints(pts) }
      }
    }
    const onKeyUp = (e: KeyboardEvent) => { if (e.code === 'Space') isSpaceRef.current = false }
    window.addEventListener('keydown', onKeyDown); window.addEventListener('keyup', onKeyUp)
    return () => { window.removeEventListener('keydown', onKeyDown); window.removeEventListener('keyup', onKeyUp) }
  }, [activeTool, penPoints, clearPen, setPenPoints, setSelection, cropToRect, drawOverlay, bumpXform])

  // ── Attach canvas events ──────────────────────────────────────────────────
  useEffect(() => {
    const dc = displayRef.current; if (!dc) return
    dc.addEventListener('pointerdown', onPointerDown); dc.addEventListener('pointermove', onPointerMove)
    dc.addEventListener('pointerup', onPointerUp); dc.addEventListener('pointercancel', onPointerUp)
    return () => {
      dc.removeEventListener('pointerdown', onPointerDown); dc.removeEventListener('pointermove', onPointerMove)
      dc.removeEventListener('pointerup', onPointerUp); dc.removeEventListener('pointercancel', onPointerUp)
    }
  }, [onPointerDown, onPointerMove, onPointerUp])

  useEffect(() => {
    const el = containerRef.current; if (!el) return
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [onWheel])

  // ── Drag-drop ─────────────────────────────────────────────────────────────
  useEffect(() => {
    const el = containerRef.current; if (!el) return
    const onDragOver = (e: DragEvent) => { e.preventDefault(); if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy' }
    const onDrop = (e: DragEvent) => {
      e.preventDefault()
      const file = e.dataTransfer?.files[0]; if (!file || !file.type.startsWith('image/')) return
      const reader = new FileReader()
      reader.onload = () => useImageEditor.getState().addImageLayerAbove(reader.result as string)
      reader.readAsDataURL(file)
    }
    el.addEventListener('dragover', onDragOver); el.addEventListener('drop', onDrop)
    return () => { el.removeEventListener('dragover', onDragOver); el.removeEventListener('drop', onDrop) }
  }, [])

  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const item = Array.from(e.clipboardData?.items ?? []).find(i => i.type.startsWith('image/')); if (!item) return
      const file = item.getAsFile(); if (!file) return
      const reader = new FileReader()
      reader.onload = () => useImageEditor.getState().addImageLayerAbove(reader.result as string)
      reader.readAsDataURL(file)
    }
    window.addEventListener('paste', onPaste); return () => window.removeEventListener('paste', onPaste)
  }, [])

  // Container size for rulers
  useEffect(() => {
    const el = containerRef.current; if (!el) return
    const ro = new ResizeObserver(entries => {
      const e = entries[0]; if (e) setContainerSize({ w: e.contentRect.width, h: e.contentRect.height })
    })
    ro.observe(el)
    const r = el.getBoundingClientRect()
    setContainerSize({ w: r.width, h: r.height })
    return () => ro.disconnect()
  }, [])

  // Cursor info for info bar
  useEffect(() => {
    const el = containerRef.current; if (!el) return
    const onMM = (e: MouseEvent) => {
      const pos = clientToCanvas(e as unknown as PointerEvent)
      if (pos.x >= 0 && pos.x < canvasWidth && pos.y >= 0 && pos.y < canvasHeight) {
        const activeCanvas = activeLayerId ? layerCanvases.get(activeLayerId) : null
        let color = 'transparent'
        if (activeCanvas) {
          const px = activeCanvas.getContext('2d')!.getImageData(Math.floor(pos.x), Math.floor(pos.y), 1, 1).data
          color = `rgb(${px[0]},${px[1]},${px[2]})`
        }
        setCursorInfo({ x: Math.floor(pos.x), y: Math.floor(pos.y), color })
      } else {
        setCursorInfo(null)
      }
    }
    el.addEventListener('mousemove', onMM)
    return () => el.removeEventListener('mousemove', onMM)
  }, [clientToCanvas, canvasWidth, canvasHeight, activeLayerId])

  return (
    <div
      ref={containerRef}
      className="flex-1 overflow-hidden bg-[#1a1a1a] relative"
      style={{ touchAction: 'none' }}
      onPointerDown={e => {
        // Click on container background (not the canvas) → deselect transform
        if (e.target === containerRef.current && xformRef.current) {
          xformRef.current = null; xformSnapshotRef.current = null; xformDragRef.current = null
          setXformVersionRef.current(v => v + 1)
        }
      }}
    >
      <div style={{ position: 'absolute', transform: `translate(${panX}px, ${panY}px)`, transformOrigin: '0 0' }}>
        <canvas
          ref={displayRef} width={canvasWidth} height={canvasHeight}
          style={{ transform: `scale(${zoom})`, transformOrigin: '0 0', display: 'block', imageRendering: zoom > 2 ? 'pixelated' : 'auto', cursor: cursor }}
        />
        <canvas
          ref={overlayRef} width={canvasWidth} height={canvasHeight}
          style={{ transform: `scale(${zoom})`, transformOrigin: '0 0', position: 'absolute', top: 0, left: 0, pointerEvents: 'none', display: 'block' }}
        />
      </div>

      {textPos && (
        <TextOverlay pos={textPos} zoom={zoom} panX={panX} panY={panY} onCommit={commitText} onCancel={() => setTextPos(null)} />
      )}

      {/* Crop confirm bar */}
      {activeTool === 'crop' && cropRectRef.current && (
        <div className="absolute bottom-8 left-1/2 -translate-x-1/2 flex items-center gap-2 bg-zinc-900/95 border border-zinc-700 rounded-lg px-4 py-2 shadow-xl text-[11px]">
          <span className="text-zinc-400">Press</span>
          <kbd className="px-1.5 py-0.5 bg-zinc-700 rounded text-[10px] text-white">Enter</kbd>
          <span className="text-zinc-400">to crop ·</span>
          <kbd className="px-1.5 py-0.5 bg-zinc-700 rounded text-[10px] text-white">Esc</kbd>
          <span className="text-zinc-400">to cancel</span>
          <button onClick={() => { const cr = cropRectRef.current; if (cr) { cropToRect(cr.x, cr.y, cr.w, cr.h); cropRectRef.current = null; drawOverlay() } }} className="ml-2 px-2.5 py-1 bg-blue-600 hover:bg-blue-500 text-white rounded text-[10px]">Apply</button>
        </div>
      )}

      {/* Transform bar */}
      {activeTool === 'select' && xformRef.current && (
        <div className="absolute bottom-8 left-1/2 -translate-x-1/2 flex items-center gap-2 bg-zinc-900/95 border border-zinc-700 rounded-lg px-4 py-2 shadow-xl text-[11px]">
          <span className="text-zinc-400">
            W: {Math.round(xformRef.current.bounds.w)} H: {Math.round(xformRef.current.bounds.h)}
            {xformRef.current.rotation !== 0 && ` · ${Math.round(xformRef.current.rotation * 180 / Math.PI)}°`}
          </span>
          <button onClick={() => { xformRef.current = null; xformSnapshotRef.current = null; bumpXform() }} className="ml-2 px-2.5 py-1 bg-blue-600 hover:bg-blue-500 text-white rounded text-[10px]">Done</button>
        </div>
      )}

      {/* Pen actions */}
      {activeTool === 'pen' && penPoints.length > 1 && (
        <div className="absolute bottom-8 left-1/2 -translate-x-1/2 flex items-center gap-2 bg-zinc-900/95 border border-zinc-700 rounded-lg px-4 py-2 shadow-xl text-[11px]">
          <button onClick={() => {
            const ctx = getActiveCtx(); if (!ctx || penPoints.length < 2) return
            ctx.save(); ctx.strokeStyle = foregroundColor; ctx.lineWidth = brushSize * 0.15 + 1; ctx.globalAlpha = brushOpacity / 100
            ctx.beginPath(); ctx.moveTo(penPoints[0].x, penPoints[0].y)
            for (const p of penPoints.slice(1)) ctx.lineTo(p.x, p.y)
            ctx.stroke(); ctx.restore(); composite(); clearPen(); drawOverlay()
          }} className="px-2 py-1 rounded hover:bg-zinc-700 text-zinc-200">Stroke</button>
          <button onClick={() => {
            const ctx = getActiveCtx(); if (!ctx || penPoints.length < 3) return
            ctx.save(); ctx.fillStyle = foregroundColor; ctx.globalAlpha = brushOpacity / 100
            ctx.beginPath(); ctx.moveTo(penPoints[0].x, penPoints[0].y)
            for (const p of penPoints.slice(1)) ctx.lineTo(p.x, p.y)
            ctx.closePath(); ctx.fill(); ctx.restore(); composite(); clearPen(); drawOverlay()
          }} className="px-2 py-1 rounded hover:bg-zinc-700 text-zinc-200">Fill</button>
          <button onClick={() => {
            const pts = penPoints
            const xs = pts.map(p => p.x), ys = pts.map(p => p.y)
            setSelection({ x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) })
            clearPen(); drawOverlay()
          }} className="px-2 py-1 rounded hover:bg-zinc-700 text-zinc-200">To Selection</button>
          <button onClick={() => { clearPen(); drawOverlay() }} className="text-zinc-500 hover:text-zinc-300 ml-1">✕</button>
        </div>
      )}

      {/* Rulers */}
      <Rulers zoom={zoom} panX={panX} panY={panY} canvasWidth={canvasWidth} canvasHeight={canvasHeight}
        containerWidth={containerSize.w} containerHeight={containerSize.h} />

      {/* Clone source indicator */}
      {activeTool === 'clone' && cloneSource && (
        <div className="absolute top-6 left-6 text-[10px] text-blue-300/60 font-mono pointer-events-none">
          ⊕ Clone source: {cloneSource.x}×{cloneSource.y} · Alt+click to resample
        </div>
      )}

      {/* Status bar */}
      <div className="absolute bottom-1 left-2 flex items-center gap-3 text-[10px] text-white/30 font-mono pointer-events-none select-none">
        {cursorInfo && (
          <>
            <span>{cursorInfo.x}, {cursorInfo.y}</span>
            <span className="flex items-center gap-1">
              <span className="w-3 h-3 rounded-sm border border-white/20 inline-block" style={{ background: cursorInfo.color }} />
              {cursorInfo.color}
            </span>
          </>
        )}
      </div>
      <div className="absolute bottom-1 right-2 text-[10px] text-white/25 font-mono pointer-events-none select-none">
        {Math.round(zoom * 100)}% · {canvasWidth}×{canvasHeight}
      </div>
    </div>
  )
}
