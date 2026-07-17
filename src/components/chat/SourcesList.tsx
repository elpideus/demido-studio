import { useState } from 'react'
import { Globe, ChevronRight, PanelRight } from 'lucide-react'
import { useSourcePanel } from '../../stores/sourcePanel'
import type { Source } from '../../lib/parseSources'

/** Favicon straight from the site — no third-party favicon proxy, so no extra party learns
 *  which pages the user's answers cite. Sites that serve none fall back to the globe. */
function Favicon({ domain }: { domain: string }) {
  const [failed, setFailed] = useState(false)
  if (!domain || failed) return <Globe size={11} className="shrink-0 text-muted-foreground" />
  return (
    <img
      src={`https://${domain}/favicon.ico`}
      alt=""
      width={11}
      height={11}
      loading="lazy"
      referrerPolicy="no-referrer"
      className="shrink-0 rounded-[2px]"
      onError={() => setFailed(true)}
    />
  )
}

export function SourcesList({ sources, messageId }: { sources: Source[]; messageId: string }) {
  const [open, setOpen] = useState(true)
  const openPanel = useSourcePanel(s => s.open)
  const prefetch = useSourcePanel(s => s.prefetch)
  const panelMessageId = useSourcePanel(s => s.messageId)
  if (!sources.length) return null

  return (
    <div className="not-prose mt-3 pt-2.5 border-t border-border/60">
      <div className="flex items-center gap-2">
        <button
          onClick={() => setOpen(o => !o)}
          aria-expanded={open}
          className="flex items-center gap-1 text-[11px] font-medium text-muted-foreground hover:text-foreground transition-colors"
        >
          <ChevronRight size={11} className={`transition-transform ${open ? 'rotate-90' : ''}`} />
          Sources
          <span className="text-muted-foreground/60">{sources.length}</span>
        </button>
        <button
          onClick={() => openPanel(messageId, sources)}
          // Start fetching on intent, not on click: the metadata round-trip plus image decode is
          // most of a second, so a cold click renders text and pops thumbnails in afterwards.
          onMouseEnter={() => prefetch(sources)}
          onFocus={() => prefetch(sources)}
          // The panel's click-outside handler must ignore this button, or its mousedown closes
          // the panel a beat before this onClick reopens it — and Details stops toggling.
          data-sources-toggle=""
          aria-pressed={panelMessageId === messageId}
          className={`flex items-center gap-1 text-[11px] transition-colors ${
            panelMessageId === messageId
              ? 'text-primary'
              : 'text-muted-foreground/70 hover:text-foreground'
          }`}
        >
          <PanelRight size={10} />
          Details
        </button>
      </div>

      {open && (
        <div className="flex flex-wrap gap-1.5 mt-2">
          {sources.map(s => (
            <a
              key={s.url}
              href={s.url}
              target="_blank"
              rel="noreferrer noopener"
              title={s.url}
              className="group flex items-center gap-1.5 max-w-[16rem] pl-1.5 pr-2 py-1 rounded-full border border-border bg-secondary hover:bg-secondary/70 hover:border-primary/40 transition-colors"
            >
              <span className="flex items-center justify-center w-4 h-4 shrink-0 rounded-full bg-background/60">
                <Favicon domain={s.domain} />
              </span>
              <span className="text-[11px] text-foreground truncate">{s.label}</span>
            </a>
          ))}
        </div>
      )}
    </div>
  )
}
