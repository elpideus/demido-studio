import { useEffect, useState } from 'react'
import { Mail, RefreshCw, Search } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'

interface EmailSummary {
  id: string
  subject: string
  from: string
  date: string
  snippet: string
}

interface EmailPage {
  emails: EmailSummary[]
  next_page_token: string | null
}

function decodeHtmlEntities(str: string) {
  const txt = document.createElement('textarea')
  txt.innerHTML = str
  return txt.value
}

export function EmailWindow() {
  const [emails, setEmails] = useState<EmailSummary[]>([])
  const [nextPageToken, setNextPageToken] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [selected, setSelected] = useState<EmailSummary | null>(null)
  const [parsed, setParsed] = useState<{ from: string; to: string; subject: string; date: string; body: string } | null>(null)
  const [bodyLoading, setBodyLoading] = useState(false)

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
    setLoading(true)
    setError(null)
    try {
      const page = await invoke<EmailPage>('fetch_emails', { query: q || undefined, maxResults: 20 })
      setEmails(page.emails)
      setNextPageToken(page.next_page_token)
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
      // silently fail — user can refresh
    } finally {
      setLoadingMore(false)
    }
  }

  const onScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 120) loadMore()
  }

  useEffect(() => { load() }, [])

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
      {/* Sidebar — email list */}
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
          {emails.map(email => (
            <button
              key={email.id}
              onClick={() => openEmail(email)}
              className={`w-full text-left px-4 py-3 border-b border-border/50 transition-colors ${
                selected?.id === email.id ? 'bg-accent/50' : 'hover:bg-accent/30'
              }`}
            >
              <div className="flex items-baseline justify-between gap-2">
                <span className="text-xs font-medium text-foreground truncate">{email.from}</span>
                <span className="text-[10px] text-muted-foreground shrink-0">{email.date.split(' ').slice(0, 4).join(' ')}</span>
              </div>
              <p className="text-xs font-medium text-foreground truncate mt-0.5">{email.subject || '(no subject)'}</p>
              <p className="text-[11px] text-muted-foreground truncate mt-0.5">{decodeHtmlEntities(email.snippet)}</p>
            </button>
          ))}
          {loadingMore && (
            <div className="flex justify-center py-3 text-muted-foreground">
              <RefreshCw size={13} className="animate-spin" />
            </div>
          )}
        </div>
      </div>

      {/* Right pane — email content */}
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
