import { useEffect, useRef, useState, useCallback } from 'react'
import mermaid from 'mermaid'
import { Copy, Download, Maximize2, Shrink } from 'lucide-react'
import { useArtifacts } from '../../stores/artifacts'

mermaid.initialize({ startOnLoad: false, theme: 'dark', securityLevel: 'antiscript' })

let idCounter = 0

interface Props {
  code: string
  title?: string
}

export function MermaidBlock({ code, title = 'Diagram' }: Props) {
  const wrapperRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLDivElement>(null)
  const svgStringRef = useRef<string>('')
  const [error, setError] = useState<string | null>(null)
  const [ready, setReady] = useState(false)
  const setActiveArtifact = useArtifacts(s => s.setActive)

  const [scale, setScale] = useState(1)
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  const dragging = useRef(false)
  const dragStart = useRef({ x: 0, y: 0 })
  const offsetAtDragStart = useRef({ x: 0, y: 0 })

  // Non-passive wheel — prevents page scroll while hovering chart
  useEffect(() => {
    const el = wrapperRef.current
    if (!el) return
    const handler = (e: WheelEvent) => {
      e.preventDefault()
      e.stopPropagation()
      setScale(s => Math.min(5, Math.max(0.1, s - e.deltaY * 0.001)))
    }
    el.addEventListener('wheel', handler, { passive: false })
    return () => el.removeEventListener('wheel', handler)
  }, [])

  // mermaid.render() is per-instance isolated (vs run() which uses a shared queue)
  useEffect(() => {
    if (!canvasRef.current) return
    const id = `mermaid-${++idCounter}-${Date.now()}`
    setError(null)
    setReady(false)
    setScale(1)
    setOffset({ x: 0, y: 0 })

    let cancelled = false
    mermaid.render(id, code).then(({ svg }) => {
      if (cancelled || !canvasRef.current) return
      canvasRef.current.innerHTML = svg
      svgStringRef.current = svg
      // Fix SVG width: mermaid sets width="100%" with max-width in style.
      // Replace with explicit pixel width so centering math works correctly.
      const svgEl = canvasRef.current.querySelector('svg')
      if (svgEl) {
        const maxW = svgEl.style.maxWidth
        if (maxW) { svgEl.setAttribute('width', maxW); svgEl.style.maxWidth = '' }
        svgEl.style.height = 'auto'
      }
      setReady(true)
    }).catch(err => {
      if (cancelled) return
      setError(String(err?.message ?? err ?? 'Render error'))
    })

    return () => { cancelled = true }
  }, [code])

  const handleFit = useCallback(() => {
    setScale(1)
    setOffset({ x: 0, y: 0 })
  }, [])

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    dragging.current = true
    dragStart.current = { x: e.clientX, y: e.clientY }
    offsetAtDragStart.current = offset

    const onMove = (ev: MouseEvent) => {
      if (!dragging.current) return
      setOffset({
        x: offsetAtDragStart.current.x + (ev.clientX - dragStart.current.x),
        y: offsetAtDragStart.current.y + (ev.clientY - dragStart.current.y),
      })
    }
    const onUp = () => {
      dragging.current = false
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [offset])

  const handleCopy = () => {
    if (svgStringRef.current) navigator.clipboard.writeText(svgStringRef.current)
  }

  const handleDownload = () => {
    if (!svgStringRef.current) return
    const blob = new Blob([svgStringRef.current], { type: 'image/svg+xml' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `${title.toLowerCase().replace(/\s+/g, '-')}.svg`
    a.click()
    URL.revokeObjectURL(url)
  }

  const handleOpenArtifact = () => {
    setActiveArtifact({
      id: `mermaid-inline-${Date.now()}`,
      messageId: 'inline',
      type: 'mermaid',
      title,
      content: code,
    })
  }

  if (error) return (
    <pre className="text-red-400 text-xs whitespace-pre-wrap my-2 p-2 border border-red-900/40 rounded bg-red-950/20">
      {error}
    </pre>
  )

  return (
    <div
      className="relative my-3 rounded-lg border border-border bg-secondary/30 overflow-hidden group select-none"
      style={{ height: 300 }}
    >
      {/* action bar */}
      {ready && (
        <div className="absolute top-2 right-2 z-10 flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
          <button onClick={handleFit} title="Fit / reset view"
            className="p-1 rounded bg-background/80 border border-border text-muted-foreground hover:text-foreground transition-colors">
            <Shrink size={12} />
          </button>
          <button onClick={handleOpenArtifact} title="Open in artifact viewer"
            className="p-1 rounded bg-background/80 border border-border text-muted-foreground hover:text-foreground transition-colors">
            <Maximize2 size={12} />
          </button>
          <button onClick={handleCopy} title="Copy SVG"
            className="p-1 rounded bg-background/80 border border-border text-muted-foreground hover:text-foreground transition-colors">
            <Copy size={12} />
          </button>
          <button onClick={handleDownload} title="Download SVG"
            className="p-1 rounded bg-background/80 border border-border text-muted-foreground hover:text-foreground transition-colors">
            <Download size={12} />
          </button>
        </div>
      )}
      {ready && (
        <div className="absolute bottom-1 left-2 text-[10px] text-muted-foreground/40 pointer-events-none">
          scroll to zoom · drag to pan
        </div>
      )}
      {/* zoom/pan canvas */}
      <div
        ref={wrapperRef}
        className="w-full h-full overflow-hidden cursor-grab active:cursor-grabbing"
        onMouseDown={onMouseDown}
      >
        <div
          ref={canvasRef}
          style={{
            position: 'absolute',
            left: `calc(50% + ${offset.x}px)`,
            top: `${offset.y + 16}px`,
            transform: `translateX(-50%) scale(${scale})`,
            transformOrigin: 'top center',
          }}
        />
      </div>
    </div>
  )
}
