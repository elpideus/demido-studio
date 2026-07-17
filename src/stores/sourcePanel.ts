import { create } from 'zustand'
import { links as linksApi } from '../lib/tauri'
import type { Source } from '../lib/parseSources'
import type { LinkPreview } from '../types'

/** Warm the browser's image cache so a thumbnail is decoded before the panel ever shows it —
 *  otherwise the first open renders text, then pops each image in as it arrives. Fire-and-forget:
 *  a thumbnail that fails to preload just loads normally (or hides) in the row. */
function preloadImages(previews: LinkPreview[]) {
  for (const p of previews) {
    if (!p.image) continue
    const img = new Image()
    img.referrerPolicy = 'no-referrer'
    img.src = p.image
  }
}

interface SourcePanelState {
  /** Sources of the message whose Details panel is open; empty when closed. */
  sources: Source[]
  /** Which message opened the panel — reopening the same one is a toggle. */
  messageId: string | null
  loading: boolean
  /** Previews keyed by url. Cached across opens: metadata for a cited page does not change
   *  within a session, and refetching on every open means re-hitting eight sites. */
  previews: Record<string, LinkPreview>
  /** Urls with a request in flight, so hover-prefetch and a fast click don't both fetch. */
  inFlight: Set<string>
  /** Fetch metadata + warm thumbnails without opening the panel. Called on Details hover. */
  prefetch(sources: Source[]): Promise<void>
  open(messageId: string, sources: Source[]): Promise<void>
  close(): void
}

export const useSourcePanel = create<SourcePanelState>((set, get) => {
  /** Fetch previews for any url not already cached or in flight, then warm their thumbnails. */
  async function loadPreviews(sources: Source[], showLoading: boolean) {
    const { previews, inFlight } = get()
    const missing = sources.map(s => s.url).filter(url => !previews[url] && !inFlight.has(url))
    if (!missing.length) return

    set(state => ({
      inFlight: new Set([...state.inFlight, ...missing]),
      ...(showLoading ? { loading: true } : {}),
    }))

    try {
      const fetched = await linksApi.fetchPreviews(missing)
      set(state => ({
        previews: { ...state.previews, ...Object.fromEntries(fetched.map(p => [p.url, p])) },
      }))
      preloadImages(fetched)
    } catch (e) {
      // One failed batch must not blank the panel — rows fall back to the footer's own label,
      // and each row shows its error. Cache the failure so the panel doesn't retry on every open.
      const message = e instanceof Error ? e.message : String(e)
      set(state => ({
        previews: {
          ...state.previews,
          ...Object.fromEntries(
            missing.map(url => [
              url,
              { url, title: null, description: null, image: null, siteName: null, error: message },
            ])
          ),
        },
      }))
    } finally {
      set(state => {
        const inFlight = new Set(state.inFlight)
        for (const url of missing) inFlight.delete(url)
        return { inFlight, ...(showLoading ? { loading: false } : {}) }
      })
    }
  }

  return {
    sources: [],
    messageId: null,
    loading: false,
    previews: {},
    inFlight: new Set(),

    // No spinner: this runs on hover, for a panel the user has not asked for yet.
    prefetch: sources => loadPreviews(sources, false),

    open: async (messageId, sources) => {
      if (get().messageId === messageId) return get().close()
      set({ messageId, sources })
      await loadPreviews(sources, true)
    },

    close: () => set({ sources: [], messageId: null }),
  }
})
