import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useSourcePanel } from './sourcePanel'
import { links } from '../lib/tauri'
import type { Source } from '../lib/parseSources'

vi.mock('../lib/tauri', () => ({ links: { fetchPreviews: vi.fn() } }))

const fetchPreviews = vi.mocked(links.fetchPreviews)

const src = (url: string, label = 'Site'): Source => ({
  label,
  url,
  domain: new URL(url).hostname,
})

const preview = (url: string) => ({
  url,
  title: 'T',
  description: 'D',
  image: null,
  siteName: 'S',
  error: null,
})

describe('useSourcePanel', () => {
  beforeEach(() => {
    useSourcePanel.setState({
      sources: [],
      messageId: null,
      loading: false,
      previews: {},
      inFlight: new Set(),
    })
    fetchPreviews.mockReset()
  })

  it('opens with sources and stores fetched previews', async () => {
    const s = [src('https://a.com/1')]
    fetchPreviews.mockResolvedValue([preview('https://a.com/1')])

    await useSourcePanel.getState().open('m1', s)

    expect(useSourcePanel.getState().messageId).toBe('m1')
    expect(useSourcePanel.getState().sources).toEqual(s)
    expect(useSourcePanel.getState().previews['https://a.com/1'].title).toBe('T')
    expect(useSourcePanel.getState().loading).toBe(false)
  })

  it('reopening the same message closes the panel', async () => {
    fetchPreviews.mockResolvedValue([preview('https://a.com/1')])
    const s = [src('https://a.com/1')]

    await useSourcePanel.getState().open('m1', s)
    await useSourcePanel.getState().open('m1', s)

    expect(useSourcePanel.getState().messageId).toBeNull()
    expect(useSourcePanel.getState().sources).toEqual([])
  })

  it('only fetches urls it has not already cached', async () => {
    fetchPreviews.mockResolvedValue([preview('https://a.com/1')])
    await useSourcePanel.getState().open('m1', [src('https://a.com/1')])
    fetchPreviews.mockClear()

    // A different message citing one cached url and one new one.
    fetchPreviews.mockResolvedValue([preview('https://b.com/2')])
    await useSourcePanel.getState().open('m2', [src('https://a.com/1'), src('https://b.com/2')])

    expect(fetchPreviews).toHaveBeenCalledTimes(1)
    expect(fetchPreviews).toHaveBeenCalledWith(['https://b.com/2'])
  })

  it('skips the call entirely when every url is cached', async () => {
    fetchPreviews.mockResolvedValue([preview('https://a.com/1')])
    await useSourcePanel.getState().open('m1', [src('https://a.com/1')])
    fetchPreviews.mockClear()

    await useSourcePanel.getState().open('m2', [src('https://a.com/1')])

    expect(fetchPreviews).not.toHaveBeenCalled()
  })

  it('a failed fetch leaves the panel open with per-row errors', async () => {
    fetchPreviews.mockRejectedValue(new Error('backend exploded'))

    await useSourcePanel.getState().open('m1', [src('https://a.com/1')])

    const state = useSourcePanel.getState()
    expect(state.messageId).toBe('m1')
    expect(state.loading).toBe(false)
    expect(state.previews['https://a.com/1'].error).toBe('backend exploded')
  })

  it('does not retry a url whose fetch already failed', async () => {
    fetchPreviews.mockRejectedValue(new Error('nope'))
    await useSourcePanel.getState().open('m1', [src('https://a.com/1')])
    fetchPreviews.mockClear()

    await useSourcePanel.getState().open('m2', [src('https://a.com/1')])

    expect(fetchPreviews).not.toHaveBeenCalled()
  })

  it('prefetch loads previews without opening the panel', async () => {
    fetchPreviews.mockResolvedValue([preview('https://a.com/1')])

    await useSourcePanel.getState().prefetch([src('https://a.com/1')])

    expect(useSourcePanel.getState().messageId).toBeNull()
    expect(useSourcePanel.getState().sources).toEqual([])
    expect(useSourcePanel.getState().previews['https://a.com/1'].title).toBe('T')
  })

  it('prefetch shows no spinner — the user has not asked for the panel yet', async () => {
    let seenLoading = false
    fetchPreviews.mockImplementation(async () => {
      seenLoading = useSourcePanel.getState().loading
      return [preview('https://a.com/1')]
    })

    await useSourcePanel.getState().prefetch([src('https://a.com/1')])

    expect(seenLoading).toBe(false)
  })

  it('a click landing mid-prefetch does not fetch the same url twice', async () => {
    let resolve!: (v: ReturnType<typeof preview>[]) => void
    fetchPreviews.mockReturnValue(new Promise(r => { resolve = r }))
    const s = [src('https://a.com/1')]

    const prefetching = useSourcePanel.getState().prefetch(s)
    const opening = useSourcePanel.getState().open('m1', s)

    resolve([preview('https://a.com/1')])
    await Promise.all([prefetching, opening])

    expect(fetchPreviews).toHaveBeenCalledTimes(1)
    expect(useSourcePanel.getState().messageId).toBe('m1')
  })

  it('clears in-flight urls once a fetch settles, so a retry is possible', async () => {
    fetchPreviews.mockResolvedValue([preview('https://a.com/1')])
    await useSourcePanel.getState().prefetch([src('https://a.com/1')])

    expect(useSourcePanel.getState().inFlight.size).toBe(0)
  })
})
