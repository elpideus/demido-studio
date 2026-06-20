import { useEffect, useRef } from 'react'

const RULER_SIZE = 20

interface RulersProps {
  zoom: number
  panX: number
  panY: number
  canvasWidth: number
  canvasHeight: number
  containerWidth: number
  containerHeight: number
}

function drawRuler(
  ctx: CanvasRenderingContext2D,
  length: number,
  thickness: number,
  isHorizontal: boolean,
  zoom: number,
  pan: number,
  _canvasSize: number,
) {
  ctx.clearRect(0, 0, isHorizontal ? length : thickness, isHorizontal ? thickness : length)
  ctx.fillStyle = '#1e1e1e'
  ctx.fillRect(0, 0, isHorizontal ? length : thickness, isHorizontal ? thickness : length)

  // Decide tick spacing based on zoom
  const minPx = 40 // minimum pixels between major ticks on screen
  const candidates = [1, 2, 5, 10, 25, 50, 100, 200, 500, 1000]
  let step = candidates.find(s => s * zoom >= minPx) ?? 1000

  ctx.strokeStyle = '#555'
  ctx.fillStyle = '#888'
  ctx.font = `9px monospace`
  ctx.textBaseline = isHorizontal ? 'bottom' : 'middle'

  const start = Math.floor(-pan / zoom / step) * step
  const end = start + Math.ceil(length / zoom / step + 1) * step + step

  for (let val = start; val <= end; val += step) {
    const screen = val * zoom + pan
    if (screen < -10 || screen > length + 10) continue
    const major = true
    const tickLen = major ? thickness * 0.55 : thickness * 0.3

    ctx.lineWidth = 0.5
    ctx.beginPath()
    if (isHorizontal) {
      ctx.moveTo(screen, thickness - tickLen)
      ctx.lineTo(screen, thickness)
    } else {
      ctx.moveTo(thickness - tickLen, screen)
      ctx.lineTo(thickness, screen)
    }
    ctx.stroke()

    if (major) {
      ctx.save()
      if (isHorizontal) {
        ctx.fillText(String(val), screen + 2, thickness - 2)
      } else {
        ctx.translate(thickness - 2, screen - 1)
        ctx.rotate(-Math.PI / 2)
        ctx.fillText(String(val), 0, 0)
      }
      ctx.restore()
    }

    // Sub-ticks
    const sub = step / 5
    if (sub * zoom > 8) {
      for (let sv = val + sub; sv < val + step; sv += sub) {
        const ss = sv * zoom + pan
        if (ss < 0 || ss > length) continue
        const stickLen = thickness * 0.2
        ctx.beginPath()
        if (isHorizontal) {
          ctx.moveTo(ss, thickness - stickLen); ctx.lineTo(ss, thickness)
        } else {
          ctx.moveTo(thickness - stickLen, ss); ctx.lineTo(thickness, ss)
        }
        ctx.stroke()
      }
    }
  }

  // Corner square
  if (isHorizontal) {
    ctx.fillStyle = '#1e1e1e'
    ctx.fillRect(0, 0, RULER_SIZE, thickness)
  }

  // Border
  ctx.strokeStyle = '#333'
  ctx.lineWidth = 1
  ctx.beginPath()
  if (isHorizontal) { ctx.moveTo(0, thickness - 0.5); ctx.lineTo(length, thickness - 0.5) }
  else { ctx.moveTo(thickness - 0.5, 0); ctx.lineTo(thickness - 0.5, length) }
  ctx.stroke()
}

export function Rulers({ zoom, panX, panY, canvasWidth, canvasHeight, containerWidth, containerHeight }: RulersProps) {
  const hRef = useRef<HTMLCanvasElement>(null)
  const vRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    if (hRef.current) {
      const ctx = hRef.current.getContext('2d')!
      drawRuler(ctx, containerWidth, RULER_SIZE, true, zoom, panX + RULER_SIZE, canvasWidth)
    }
    if (vRef.current) {
      const ctx = vRef.current.getContext('2d')!
      drawRuler(ctx, containerHeight, RULER_SIZE, false, zoom, panY + RULER_SIZE, canvasHeight)
    }
  }, [zoom, panX, panY, containerWidth, containerHeight, canvasWidth, canvasHeight])

  return (
    <>
      {/* Top ruler */}
      <canvas
        ref={hRef}
        width={containerWidth}
        height={RULER_SIZE}
        className="absolute top-0 left-0 pointer-events-none z-10"
        style={{ width: containerWidth, height: RULER_SIZE }}
      />
      {/* Left ruler */}
      <canvas
        ref={vRef}
        width={RULER_SIZE}
        height={containerHeight}
        className="absolute top-0 left-0 pointer-events-none z-10"
        style={{ width: RULER_SIZE, height: containerHeight }}
      />
      {/* Corner square */}
      <div
        className="absolute top-0 left-0 z-20 pointer-events-none"
        style={{ width: RULER_SIZE, height: RULER_SIZE, background: '#1e1e1e', borderRight: '1px solid #333', borderBottom: '1px solid #333' }}
      />
    </>
  )
}
