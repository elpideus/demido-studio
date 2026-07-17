import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Mail, MoreVertical, RefreshCw, Search } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { clampToViewport } from '@/lib/utils'

interface EmailSummary {
  id: string
  subject: string
  from: string
  date: string
  snippet: string
  unread: boolean
}

interface EmailPage {
  emails: EmailSummary[]
  next_page_token: string | null
  result_size_estimate: number | null
}

function decodeHtmlEntities(str: string) {
  const txt = document.createElement('textarea')
  txt.innerHTML = str
  return txt.value
}

// ponytail: module-scope cache (not a store) so reopening the window shows
// the last inbox instantly instead of a blank loading state; refetches in
// the background to stay fresh. Upgrade to a Zustand store if other windows
// need to read/invalidate this too.
let emailCache: EmailPage | null = null

export function EmailWindow() {
  const [emails, setEmails] = useState<EmailSummary[]>(emailCache?.emails ?? [])
  const [nextPageToken, setNextPageToken] = useState<string | null>(emailCache?.next_page_token ?? null)
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [selected, setSelected] = useState<EmailSummary | null>(null)
  const [parsed, setParsed] = useState<{ from: string; to: string; subject: string; date: string; body: string } | null>(null)
  const [bodyLoading, setBodyLoading] = useState(false)
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; email: EmailSummary } | null>(null)
  const ctxMenuRef = useRef<HTMLDivElement>(null)
  const [checkedIds, setCheckedIds] = useState<Set<string>>(new Set())
  const [anchorIndex, setAnchorIndex] = useState<number | null>(null)
  const [resultEstimate, setResultEstimate] = useState<number | null>(emailCache?.result_size_estimate ?? null)
  const [selectingAll, setSelectingAll] = useState(false)

  function parseEmailResponse(raw: string) {
    const sep = raw.indexOf('\n\n')
    const headerBlock = sep === -1 ? raw : raw.slice(0, sep)
    const body = sep === -1 ? '' : raw.slice(sep + 2)
    const get = (name: string) => {
      const m = headerBlock.match(new RegExp(`^${name}:\\s*(.*)$`, 'm'))
      return m?.[1]?.trim() ?? ''
    }
    return { from: get('From'), to: get('To'), subject: get('Subject'), date: get('Date'), body }
  }

  const load = async (q = query) => {
    // Skip the spinner when we already have cached results to show, this
    // call just refreshes them in the background.
    if (!(emailCache && q === '')) setLoading(true)
    setError(null)
    try {
      const page = await invoke<EmailPage>('fetch_emails', { query: q || undefined, maxResults: 20 })
      setEmails(page.emails)
      setNextPageToken(page.next_page_token)
      setResultEstimate(page.result_size_estimate)
      setCheckedIds(new Set())
      if (q === '') emailCache = page
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  const loadMore = async () => {
    if (!nextPageToken || loadingMore) return
    setLoadingMore(true)
    try {
      const page = await invoke<EmailPage>('fetch_emails', { query: query || undefined, maxResults: 20, pageToken: nextPageToken })
      setEmails(prev => [...prev, ...page.emails])
      setNextPageToken(page.next_page_token)
    } catch {
      // silently fail: user can refresh
    } finally {
      setLoadingMore(false)
    }
  }

  const onScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 120) loadMore()
  }

  useEffect(() => { load() }, [])

  useEffect(() => {
    if (!ctxMenu) return
    requestAnimationFrame(() => ctxMenuRef.current && clampToViewport(ctxMenuRef.current))
    const dismiss = () => setCtxMenu(null)
    window.addEventListener('mousedown', dismiss)
    return () => window.removeEventListener('mousedown', dismiss)
  }, [ctxMenu])

  const patchEmail = (id: string, patch: Partial<EmailSummary>) => {
    setEmails(prev => prev.map(e => e.id === id ? { ...e, ...patch } : e))
    if (emailCache) emailCache.emails = emailCache.emails.map(e => e.id === id ? { ...e, ...patch } : e)
  }

  const setReadState = async (email: EmailSummary, unread: boolean) => {
    setCtxMenu(null)
    patchEmail(email.id, { unread })
    try {
      await invoke('set_email_read', { id: email.id, read: !unread })
    } catch (e) {
      patchEmail(email.id, { unread: !unread })
      setError(String(e))
    }
  }

  const trashEmail = async (email: EmailSummary) => {
    setCtxMenu(null)
    const prevEmails = emails
    setEmails(prev => prev.filter(e => e.id !== email.id))
    if (emailCache) emailCache.emails = emailCache.emails.filter(e => e.id !== email.id)
    if (selected?.id === email.id) setSelected(null)
    try {
      await invoke('trash_email', { id: email.id })
    } catch (e) {
      setEmails(prevEmails)
      if (emailCache) emailCache.emails = prevEmails
      setError(String(e))
    }
  }

  const setReadStateBatch = async (ids: string[], unread: boolean) => {
    ids.forEach(id => patchEmail(id, { unread }))
    await Promise.all(ids.map(id => invoke('set_email_read', { id, read: !unread }).catch(() => {})))
  }

  const trashEmailBatch = async (ids: string[]) => {
    const idSet = new Set(ids)
    const prevEmails = emails
    setEmails(prev => prev.filter(e => !idSet.has(e.id)))
    if (emailCache) emailCache.emails = emailCache.emails.filter(e => !idSet.has(e.id))
    if (selected && idSet.has(selected.id)) setSelected(null)
    setCheckedIds(new Set())
    const results = await Promise.all(ids.map(id => invoke('trash_email', { id }).then(() => true).catch(() => false)))
    if (results.some(ok => !ok)) {
      setEmails(prevEmails)
      if (emailCache) emailCache.emails = prevEmails
      setError('Some emails failed to delete.')
    }
  }

  const clickEmail = (email: EmailSummary, index: number, e: React.MouseEvent) => {
    if (e.ctrlKey || e.metaKey) {
      setCheckedIds(prev => {
        const next = new Set(prev)
        next.has(email.id) ? next.delete(email.id) : next.add(email.id)
        return next
      })
      setAnchorIndex(index)
      return
    }
    if (e.shiftKey && anchorIndex !== null) {
      const [lo, hi] = anchorIndex < index ? [anchorIndex, index] : [index, anchorIndex]
      setCheckedIds(prev => {
        const next = new Set(prev)
        for (let i = lo; i <= hi; i++) next.add(emails[i].id)
        return next
      })
      return
    }
    if (checkedIds.size > 0) {
      setCheckedIds(new Set())
      return
    }
    openEmail(email)
  }

  const toggleChecked = (id: string, index: number) => {
    setCheckedIds(prev => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
    setAnchorIndex(index)
  }

  const allLoadedChecked = emails.length > 0 && emails.every(e => checkedIds.has(e.id))
  const moreThanLoaded = resultEstimate !== null && resultEstimate > emails.length

  const toggleSelectAllLoaded = () => {
    setCheckedIds(allLoadedChecked ? new Set() : new Set(emails.map(e => e.id)))
  }

  const selectAllMatching = async () => {
    setSelectingAll(true)
    try {
      const ids = new Set(emails.map(e => e.id))
      let token = nextPageToken
      while (token) {
        const page = await invoke<EmailPage>('fetch_emails', { query: query || undefined, maxResults: 50, pageToken: token })
        page.emails.forEach(e => ids.add(e.id))
        token = page.next_page_token
      }
      setCheckedIds(ids)
    } catch (e) {
      setError(String(e))
    } finally {
      setSelectingAll(false)
    }
  }

  const openEmail = async (email: EmailSummary) => {
    setSelected(email)
    setParsed(null)
    setBodyLoading(true)
    try {
      const raw = await invoke<string>('get_email_body', { id: email.id })
      setParsed(parseEmailResponse(raw))
    } catch (e) {
      setParsed({ from: '', to: '', subject: email.subject, date: email.date, body: `Error: ${e}` })
    } finally {
      setBodyLoading(false)
    }
  }

  return (
    <div className="flex h-full overflow-hidden">
      {/* Sidebar: email list */}
      <div className="flex flex-col w-80 shrink-0 border-r border-border overflow-hidden">
        <div className="flex items-center gap-2 px-3 py-2 border-b border-border">
          <div className="flex-1 flex items-center gap-1 bg-secondary rounded-md px-2 py-1">
            <Search size={13} className="text-muted-foreground shrink-0" />
            <input
              value={query}
              onChange={e => setQuery(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && load()}
              placeholder="Search emails…"
              className="bg-transparent text-xs text-foreground placeholder:text-muted-foreground outline-none flex-1"
            />
          </div>
          <Button variant="ghost" size="icon-xs" onClick={() => load()} disabled={loading} title="Refresh">
            <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
          </Button>
        </div>

        {checkedIds.size > 0 ? (
          <div className="flex flex-col border-b border-border">
            <div className="flex items-center gap-2 px-3 py-1.5">
              <Checkbox
                checked={allLoadedChecked}
                onCheckedChange={toggleSelectAllLoaded}
                title="Select all loaded"
              />
              <span className="text-xs text-muted-foreground flex-1">{checkedIds.size} selected</span>
              <button
                className="text-[11px] text-foreground/80 hover:text-foreground"
                onClick={() => setReadStateBatch([...checkedIds], true)}
              >
                Mark read
              </button>
              <button
                className="text-[11px] text-foreground/80 hover:text-foreground"
                onClick={() => setReadStateBatch([...checkedIds], false)}
              >
                Mark unread
              </button>
              <button
                className="text-[11px] text-red-400 hover:text-red-300"
                onClick={() => trashEmailBatch([...checkedIds])}
              >
                Delete
              </button>
              <button
                className="text-[11px] text-muted-foreground hover:text-foreground"
                onClick={() => setCheckedIds(new Set())}
              >
                Cancel
              </button>
            </div>
            {allLoadedChecked && moreThanLoaded && (
              <div className="px-3 pb-1.5 text-[11px] text-muted-foreground">
                All {emails.length} loaded selected.{' '}
                <button
                  className="text-blue-400 hover:text-blue-300 disabled:opacity-50"
                  onClick={selectAllMatching}
                  disabled={selectingAll}
                >
                  {selectingAll ? 'Selecting…' : `Select all ~${resultEstimate} emails`}
                </button>
              </div>
            )}
          </div>
        ) : null}

        <div className="flex-1 overflow-y-auto" onScroll={onScroll}>
          {error && (
            <div className="m-3 px-3 py-2 rounded-lg bg-red-950/30 border border-red-800/40 text-red-400 text-xs">
              {error.includes('No email account') ? (
                <span>No email account connected. Add one in <strong>Accounts</strong>.</span>
              ) : error}
            </div>
          )}
          {!loading && !error && emails.length === 0 && (
            <div className="flex flex-col items-center justify-center h-32 gap-2 text-muted-foreground">
              <Mail size={32} className="opacity-20" />
              <p className="text-xs">No emails found.</p>
            </div>
          )}
          {emails.map((email, index) => (
            <div
              key={email.id}
              role="button"
              tabIndex={0}
              onClick={e => clickEmail(email, index, e)}
              onContextMenu={e => { e.preventDefault(); setCtxMenu({ x: e.clientX, y: e.clientY, email }) }}
              className={`group relative w-full text-left pl-8 pr-4 py-3 border-b border-border/50 transition-colors cursor-pointer ${
                selected?.id === email.id ? 'bg-accent/50' : 'hover:bg-accent/30'
              }`}
            >
              <Checkbox
                checked={checkedIds.has(email.id)}
                onClick={e => e.stopPropagation()}
                onCheckedChange={() => toggleChecked(email.id, index)}
                className={`absolute top-3 left-3 transition-opacity ${
                  checkedIds.has(email.id) || checkedIds.size > 0 ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
                }`}
              />
              <button
                onClick={e => { e.stopPropagation(); setCtxMenu({ x: e.clientX, y: e.clientY, email }) }}
                className="absolute top-2 right-2 p-0.5 rounded opacity-0 group-hover:opacity-100 hover:bg-accent text-muted-foreground"
                title="More"
              >
                <MoreVertical size={13} />
              </button>
              <div className="flex items-baseline justify-between gap-2 pr-5">
                <span className="flex items-center gap-1.5 min-w-0">
                  {email.unread && (
                    <span className="size-1.5 rounded-full bg-blue-500 shrink-0" title="Unread" />
                  )}
                  <span className={`text-xs truncate ${email.unread ? 'font-semibold text-foreground' : 'font-medium text-foreground'}`}>{email.from}</span>
                </span>
                <span className="text-[10px] text-muted-foreground shrink-0">{email.date.split(' ').slice(0, 4).join(' ')}</span>
              </div>
              <p className={`text-xs truncate mt-0.5 ${email.unread ? 'font-semibold text-foreground' : 'font-medium text-foreground'}`}>{email.subject || '(no subject)'}</p>
              <p className="text-[11px] text-muted-foreground truncate mt-0.5">{decodeHtmlEntities(email.snippet)}</p>
            </div>
          ))}
          {loadingMore && (
            <div className="flex justify-center py-3 text-muted-foreground">
              <RefreshCw size={13} className="animate-spin" />
            </div>
          )}
        </div>
      </div>

      {ctxMenu && createPortal(
        <div
          ref={ctxMenuRef}
          className="fixed z-50 min-w-[150px] bg-popover border border-border rounded-md shadow-md py-1 text-[12px]"
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          onMouseDown={e => e.stopPropagation()}
        >
          <button
            className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80"
            onClick={() => setReadState(ctxMenu.email, !ctxMenu.email.unread)}
          >
            Mark as {ctxMenu.email.unread ? 'read' : 'unread'}
          </button>
          <div className="border-t border-border/40 my-1" />
          <button
            className="w-full text-left px-3 py-1.5 hover:bg-red-500/20 text-red-400"
            onClick={() => trashEmail(ctxMenu.email)}
          >
            Delete
          </button>
        </div>,
        document.body
      )}

      {/* Right pane: email content */}
      <div className="flex flex-col flex-1 overflow-hidden">
        {selected ? (
          <>
            <div className="px-6 py-5 border-b border-border shrink-0 space-y-3">
              <h2 className="text-base font-semibold text-foreground leading-snug">
                {parsed?.subject || selected.subject || '(no subject)'}
              </h2>
              <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
                <span className="text-muted-foreground font-medium">From</span>
                <span className="text-foreground">{parsed?.from || selected.from}</span>
                {parsed?.to && (
                  <>
                    <span className="text-muted-foreground font-medium">To</span>
                    <span className="text-foreground truncate">{parsed.to}</span>
                  </>
                )}
                <span className="text-muted-foreground font-medium">Date</span>
                <span className="text-foreground">{parsed?.date || selected.date}</span>
              </div>
            </div>
            <div className="flex-1 overflow-hidden">
              {bodyLoading ? (
                <span className="px-6 py-5 block text-sm text-muted-foreground">Loading…</span>
              ) : (
                <iframe
                  title="email"
                  sandbox=""
                  className="w-full h-full border-0 bg-white"
                  srcDoc={parsed?.body ?? ''}
                />
              )}
            </div>
          </>
        ) : (
          <div className="flex flex-col items-center justify-center h-full gap-2 text-muted-foreground">
            <Mail size={40} className="opacity-15" />
            <p className="text-xs">Select an email to read</p>
          </div>
        )}
      </div>
    </div>
  )
}
