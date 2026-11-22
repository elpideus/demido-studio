/* A PDF, drawn page by page with pdf.js (Mozilla, Apache-2.0): many browsers a window can be in have no PDF
   viewer of their own, and offer the file as a download instead. pdf.js is loaded only when a PDF is opened.
   Only its drawing library is used, which runs nothing a PDF carries; its viewer, which can, is not. */
import { useEffect, useRef, useState } from 'react'
import { Glyph, Signal } from '@/ui'

/** The most pages drawn: a longer PDF says how many more it has. */
const MOST_PAGES = 60

export function PdfPages({ url, name }: { url: string; name: string }) {
  const holder = useRef<HTMLDivElement>(null)
  const [pages, setPages] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    let task: { destroy: () => Promise<void> } | null = null
    const place = holder.current
    if (!place) return
    place.replaceChildren()
    setPages(null)
    setError(null)
    void (async () => {
      try {
        const [pdfjs, worker] = await Promise.all([import('pdfjs-dist'), import('pdfjs-dist/build/pdf.worker.min.mjs?url')])
        pdfjs.GlobalWorkerOptions.workerSrc = worker.default
        const loading = pdfjs.getDocument({ url, enableXfa: false })
        task = loading
        const doc = await loading.promise
        if (!alive) return
        setPages(doc.numPages)
        const width = Math.max(320, Math.min(place.clientWidth - 32, 960))
        for (let n = 1; n <= Math.min(doc.numPages, MOST_PAGES) && alive; n++) {
          const page = await doc.getPage(n)
          const scale = width / page.getViewport({ scale: 1 }).width
          const viewport = page.getViewport({ scale: scale * (window.devicePixelRatio || 1) })
          const canvas = document.createElement('canvas')
          canvas.width = Math.floor(viewport.width)
          canvas.height = Math.floor(viewport.height)
          canvas.style.width = `${Math.floor(viewport.width / (window.devicePixelRatio || 1))}px`
          canvas.setAttribute('aria-label', `${name}, page ${n} of ${doc.numPages}`)
          canvas.setAttribute('role', 'img')
          place.append(canvas)
          await page.render({ canvas, viewport }).promise
        }
      } catch (e) {
        if (alive) setError((e as Error).message)
      }
    })()
    return () => {
      alive = false
      void task?.destroy()
    }
  }, [url, name])
  return (
    <div className="files__preview files__pdf">
      {error && <Signal level="caution">The PDF could not be shown: {error}</Signal>}
      {pages === null && !error && <p className="app-empty"><Glyph name="task-running" busy /> Opening the PDF…</p>}
      <div ref={holder} className="files__pages" />
      {pages !== null && pages > MOST_PAGES && <p className="app-empty">The first {MOST_PAGES} of its {pages} pages are shown.</p>}
    </div>
  )
}
