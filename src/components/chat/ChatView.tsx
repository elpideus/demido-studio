import { useEffect, useRef } from 'react'
import { ChatHeader } from './ChatHeader'
import { MessageList } from './MessageList'
import { InputBar } from './InputBar'
import { ModelSelector } from './ModelSelector'
import { useConversations } from '../../stores/conversations'
import { useMessages } from '../../stores/messages'
import { useProviders } from '../../stores/providers'

function ConstellationCanvas({ zoneRef }: { zoneRef: React.RefObject<HTMLDivElement | null> }) {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    let raf: number
    const N = 180
    type Star = { x: number; y: number; vx: number; vy: number; ox: number; oy: number; alpha: number }
    let stars: Star[] = []

    let mouse = { x: -9999, y: -9999 }
    const onMouse = (e: MouseEvent) => {
      const r = canvas.getBoundingClientRect()
      mouse = { x: e.clientX - r.left, y: e.clientY - r.top }
    }
    window.addEventListener('mousemove', onMouse)

    const spawnAtEdge = (w: number, h: number) => {
      const edge = Math.floor(Math.random() * 4)
      const vx = (Math.random() - 0.5) * 0.25
      const vy = (Math.random() - 0.5) * 0.25
      const pos = edge === 0 ? { x: Math.random() * w, y: 0 }
                : edge === 1 ? { x: Math.random() * w, y: h }
                : edge === 2 ? { x: 0, y: Math.random() * h }
                :               { x: w, y: Math.random() * h }
      return { ...pos, vx, vy, ox: vx, oy: vy, alpha: 0 }
    }

    const resize = () => {
      canvas.width = canvas.offsetWidth
      canvas.height = canvas.offsetHeight
      stars = Array.from({ length: N }, () => {
        const vx = (Math.random() - 0.5) * 0.25
        const vy = (Math.random() - 0.5) * 0.25
        return { x: Math.random() * canvas.width, y: Math.random() * canvas.height, vx, vy, ox: vx, oy: vy, alpha: 1 }
      })
    }

    const REPEL = 100
    const PAD = 40 // extra fade padding around zone rect

    const draw = () => {
      const w = canvas.width, h = canvas.height
      ctx.clearRect(0, 0, w, h)

      // Compute zone rect in canvas-local coords
      let zone = { left: w/2 - 100, top: h/2 - 80, right: w/2 + 100, bottom: h/2 + 80 }
      if (zoneRef.current) {
        const cr = canvas.getBoundingClientRect()
        const zr = zoneRef.current.getBoundingClientRect()
        zone = { left: zr.left - cr.left, top: zr.top - cr.top, right: zr.right - cr.left, bottom: zr.bottom - cr.top }
      }

      for (const s of stars) {
        // mouse repel
        const mdx = s.x - mouse.x, mdy = s.y - mouse.y
        const md = Math.sqrt(mdx * mdx + mdy * mdy)
        if (md < REPEL && md > 0) {
          const force = (REPEL - md) / REPEL * 0.8
          s.vx += (mdx / md) * force
          s.vy += (mdy / md) * force
        }
        s.vx += (s.ox - s.vx) * 0.04
        s.vy += (s.oy - s.vy) * 0.04
        s.x += s.vx
        s.y += s.vy

        // how far inside the outer fade boundary (zone rect expanded by PAD)
        const fromLeft   = s.x - (zone.left   - PAD)
        const fromRight  = (zone.right  + PAD) - s.x
        const fromTop    = s.y - (zone.top    - PAD)
        const fromBottom = (zone.bottom + PAD) - s.y
        const minDist = Math.min(fromLeft, fromRight, fromTop, fromBottom)
        // minDist <= 0: outside fade zone → alpha 1
        // 0 < minDist < PAD: fading
        // minDist >= PAD: inside core → alpha 0
        const targetAlpha = minDist <= 0 ? 1 : Math.max(0, 1 - minDist / PAD)
        s.alpha += (targetAlpha - s.alpha) * 0.05

        // inside core rect → respawn
        const inCore = s.x > zone.left && s.x < zone.right && s.y > zone.top && s.y < zone.bottom
        if ((s.alpha < 0.01 && inCore) || s.x < -10 || s.x > w + 10 || s.y < -10 || s.y > h + 10) {
          Object.assign(s, spawnAtEdge(w, h))
        }
      }

      // Draw edges
      for (let i = 0; i < stars.length; i++) {
        for (let j = i + 1; j < stars.length; j++) {
          const dx = stars[i].x - stars[j].x
          const dy = stars[i].y - stars[j].y
          const d = Math.sqrt(dx * dx + dy * dy)
          if (d < 160) {
            const a = Math.min(stars[i].alpha, stars[j].alpha)
            ctx.beginPath()
            ctx.moveTo(stars[i].x, stars[i].y)
            ctx.lineTo(stars[j].x, stars[j].y)
            ctx.strokeStyle = `rgba(150,150,180,${a * 0.15 * (1 - d / 160)})`
            ctx.lineWidth = 0.6
            ctx.stroke()
          }
        }
      }

      // Draw dots
      for (const s of stars) {
        ctx.beginPath()
        ctx.arc(s.x, s.y, 1.2, 0, Math.PI * 2)
        ctx.fillStyle = `rgba(180,180,210,${s.alpha * 0.35})`
        ctx.fill()
      }

      raf = requestAnimationFrame(draw)
    }

    const ro = new ResizeObserver(resize)
    ro.observe(canvas)
    resize()
    draw()

    return () => { cancelAnimationFrame(raf); ro.disconnect(); window.removeEventListener('mousemove', onMouse) }
  }, [])

  return <canvas ref={canvasRef} className="absolute inset-0 w-full h-full pointer-events-none" />
}

function EmptyState() {
  const zoneRef = useRef<HTMLDivElement>(null)
  return (
    <div className="flex-1 flex flex-col overflow-hidden relative">
      <ConstellationCanvas zoneRef={zoneRef} />
      <div data-tauri-drag-region className="flex-1 relative z-10" />
      <div ref={zoneRef} className="flex flex-col items-center w-full max-w-xl self-center px-4 pb-4 relative z-10">
        <h1 className="text-2xl font-semibold text-foreground mb-3 select-none">
          What do you want to do?
        </h1>
        <div className="mb-5">
          <ModelSelector />
        </div>
        <div className="w-full max-w-xl">
          <InputBar />
        </div>
      </div>
      <div data-tauri-drag-region className="flex-1 relative z-10" />
    </div>
  )
}

export function ChatView() {
  const { activeId, conversations } = useConversations()
  const { load, startListening } = useMessages()
  const setSelected = useProviders(s => s.setSelected)

  useEffect(() => {
    if (!activeId) return
    load(activeId)
    const conv = conversations.find(c => c.id === activeId)
    if (conv) setSelected(conv.provider_id, conv.model_id)
  }, [activeId])

  useEffect(() => {
    // Track whether this effect instance is still alive.
    // In React StrictMode, effects mount→cleanup→mount rapidly.
    // Without this flag, the first async completes after cleanup
    // and leaves a dangling listener alongside the second mount's listener.
    let alive = true
    let unlisten: (() => void) | undefined

    startListening().then(fn => {
      if (alive) {
        unlisten = fn
      } else {
        fn() // effect already cleaned up — unlisten immediately
      }
    })

    return () => {
      alive = false
      unlisten?.()
    }
  }, [])

  if (!activeId) {
    return <EmptyState />
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <ChatHeader />
      <MessageList />
      <InputBar />
    </div>
  )
}
