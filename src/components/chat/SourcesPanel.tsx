import { useEffect, useRef, useState } from 'react'
import { X, ExternalLink, Globe, AlertCircle, Loader2 } from 'lucide-react'
import { useSourcePanel } from '../../stores/sourcePanel'
import type { Source } from '../../lib/parseSources'
import type { LinkPreview } from '../../types'

function PreviewRow({ source, preview }: { source: Source; preview?: LinkPreview }) {
  const [imageLoaded, setImageLoaded] = useState(false)
  const [imageFailed, setImageFailed] = useState(false)

  // Until the fetch lands, the footer's own label and domain already give a usable row — so the
  // panel never shows a bare skeleton for a link it can already name.
  const title = preview?.title || source.label
  const site = preview?.siteName || source.domain

  return (
    <a
      href={source.url}
      target="_blank"
      rel="noreferrer noopener"
      className="group flex gap-2.5 p-2 rounded-lg border border-border bg-secondary/40 hover:bg-secondary hover:border-primary/40 transition-colors"
    >
      {preview?.image && !imageFailed && (
        <img
          src={preview.image}
          alt=""
          referrerPolicy="no-referrer"
          // No `loading="lazy"`: the store preloads these, and lazy defers the decode back to
          // scroll time, which is the pop-in the preload exists to prevent.
          decoding="async"
          // 1.91:1 is the Open Graph card ratio these images are authored for, so cropping to a
          // square cut the sides off most of them. Fixed width, not a full-width banner: at 340px
          // the banner ate the card and pushed the description out of view.
          className={`w-24 aspect-[1200/627] self-center shrink-0 rounded object-cover object-center bg-background/40
            transition-opacity duration-200 motion-reduce:transition-none ${imageLoaded ? 'opacity-100' : 'opacity-0'}`}
          // Preloaded images are decoded already and this fires on the first paint, so the fade
          // only shows on a cold open — where it beats a hard pop.
          onLoad={() => setImageLoaded(true)}
          onError={() => setImageFailed(true)}
        />
      )}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 mb-0.5">
          <Globe size={10} className="shrink-0 text-muted-foreground" />
          <span className="text-[10px] text-muted-foreground truncate">{site}</span>
          <ExternalLink
            size={10}
            className="ml-auto shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity"
          />
        </div>
        <p className="text-xs font-medium text-foreground leading-snug line-clamp-2">{title}</p>
        {preview?.description && (
          <p className="mt-1 text-[11px] text-muted-foreground leading-snug line-clamp-3">
            {preview.description}
          </p>
        )}
        {preview?.error && (
          <p className="mt-1 flex items-center gap-1 text-[10px] text-muted-foreground/70">
            <AlertCircle size={9} className="shrink-0" />
            {preview.error}
          </p>
        )}
        <p className="mt-1 text-[10px] text-muted-foreground/50 truncate">{source.url}</p>
      </div>
    </a>
  )
}

/** Matches the transition duration below — the panel unmounts once the slide-out has finished. */
const ANIM_MS = 200

export function SourcesPanel() {
  const { sources, previews, loading, close } = useSourcePanel()
  const open = sources.length > 0
  const panelRef = useRef<HTMLDivElement>(null)

  // Closing clears the store's sources, so the exiting panel would slide out empty. Hold the last
  // non-empty set until the animation is done and the panel actually unmounts.
  const lastSources = useRef(sources)
  if (sources.length) lastSources.current = sources
  const shown = sources.length ? sources : lastSources.current

  // `mounted` outlives `open` by one animation so the exit can play; `entered` drives the slide.
  const [mounted, setMounted] = useState(false)
  const [entered, setEntered] = useState(false)

  // Mount first, and *only* mount. Flipping `entered` from this same effect scheduled the frame
  // before the panel existed in the DOM, so the browser painted the mount and the open state
  // together — the element appeared already-open with nothing to transition from.
  useEffect(() => {
    if (open) return setMounted(true)
    setEntered(false)
    const t = setTimeout(() => setMounted(false), ANIM_MS)
    return () => clearTimeout(t)
  }, [open])

  // Then, once the closed position has actually been committed, flip to open. Two frames: the
  // first guarantees the browser has painted `translate-x-full`, the second starts the transition
  // from it. One frame is enough in principle and flaky in practice.
  useEffect(() => {
    if (!mounted || !open) return
    let raf2 = 0
    const raf1 = requestAnimationFrame(() => { raf2 = requestAnimationFrame(() => setEntered(true)) })
    return () => { cancelAnimationFrame(raf1); cancelAnimationFrame(raf2) }
  }, [mounted, open])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') close() }
    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as HTMLElement
      if (panelRef.current?.contains(target)) return
      // A Details button owns the panel's open state and toggles it itself. Closing here would
      // land first and its click would reopen, so the toggle would never appear to close.
      if (target.closest('[data-sources-toggle]')) return
      close()
    }
    window.addEventListener('keydown', onKey)
    document.addEventListener('mousedown', onPointerDown)
    return () => {
      window.removeEventListener('keydown', onKey)
      document.removeEventListener('mousedown', onPointerDown)
    }
  }, [open, close])

  if (!mounted) return null

  return (
    <div
      ref={panelRef}
      className={`absolute top-0 right-0 h-full w-[340px] z-40 flex flex-col bg-background/95 backdrop-blur-sm border-l border-border shadow-2xl
        transition-transform duration-200 ease-out motion-reduce:transition-none
        ${entered ? 'translate-x-0' : 'translate-x-full'}`}
    >
      <div className="flex items-center gap-2 px-3 py-2.5 border-b border-border shrink-0">
        <span className="text-xs font-medium text-foreground">Sources</span>
        <span className="text-[10px] text-muted-foreground">{shown.length}</span>
        {loading && <Loader2 size={11} className="animate-spin text-muted-foreground" />}
        <button
          onClick={close}
          aria-label="Close sources"
          className="ml-auto p-1 rounded hover:bg-secondary text-muted-foreground hover:text-foreground transition-colors"
        >
          <X size={13} />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-2.5 space-y-2">
        {shown.map(s => (
          <PreviewRow key={s.url} source={s} preview={previews[s.url]} />
        ))}
      </div>
    </div>
  )
}
