import { useEffect, useMemo, useRef, useState } from 'react'
import { Users, RefreshCw, Search, Phone, Mail, ArrowUpAZ, ArrowDownAZ } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import Fuse from 'fuse.js'

interface Contact {
  name: string
  emails: string[]
  phones: string[]
  photo_url?: string
}

interface ContactsPage {
  contacts: Contact[]
  next_page_token: string | null
}

// Module-level cache — persists across re-mounts, cleared on manual refresh
let cache: Contact[] | null = null

export function ContactsWindow() {
  const [allContacts, setAllContacts] = useState<Contact[]>(cache ?? [])
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [sortAsc, setSortAsc] = useState(true)
  const loadingRef = useRef(false)

  const fuse = useMemo(() => new Fuse(allContacts, { keys: ['name', 'emails', 'phones'], threshold: 0.4 }), [allContacts])

  const contacts = useMemo(() => {
    const base = query.trim() ? fuse.search(query.trim()).map(r => r.item) : allContacts
    return sortAsc ? base : [...base].reverse()
  }, [query, fuse, allContacts, sortAsc])

  const fetchAll = async () => {
    if (loadingRef.current) return
    loadingRef.current = true
    setLoading(true)
    setError(null)
    try {
      const all: Contact[] = []
      let token: string | null = null
      do {
        const page: ContactsPage = await invoke<ContactsPage>('fetch_contacts', {
          maxResults: 100,
          pageToken: token ?? undefined,
        })
        all.push(...page.contacts)
        token = page.next_page_token
      } while (token)
      const sorted = all.sort((a, b) => a.name.localeCompare(b.name))
      cache = sorted
      setAllContacts(sorted)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
      loadingRef.current = false
    }
  }

  const refresh = () => {
    cache = null
    setAllContacts([])
    fetchAll()
  }

  useEffect(() => {
    if (!cache) fetchAll()
  }, [])

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-border">
        <div className="flex-1 flex items-center gap-1 bg-secondary rounded-md px-2 py-1">
          <Search size={13} className="text-muted-foreground shrink-0" />
          <input
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder="Search contacts…"
            className="bg-transparent text-xs text-foreground placeholder:text-muted-foreground outline-none flex-1"
          />
        </div>
        <Button variant="ghost" size="icon-xs" onClick={() => setSortAsc(v => !v)} title={sortAsc ? 'A→Z' : 'Z→A'}>
          {sortAsc ? <ArrowUpAZ size={13} /> : <ArrowDownAZ size={13} />}
        </Button>
        <Button variant="ghost" size="icon-xs" onClick={() => refresh()} disabled={loading} title="Refresh">
          <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto p-2">
        {error && (
          <div className="m-2 px-3 py-2 rounded-lg bg-red-950/30 border border-red-800/40 text-red-400 text-xs">
            {error.includes('No contacts account') ? (
              <span>No contacts account connected. Add one in <strong>Accounts</strong>.</span>
            ) : error}
          </div>
        )}
        {!loading && !error && contacts.length === 0 && (
          <div className="flex flex-col items-center justify-center h-32 gap-2 text-muted-foreground">
            <Users size={32} className="opacity-20" />
            <p className="text-xs">No contacts found.</p>
          </div>
        )}
        {contacts.map((c, i) => (
          <div key={i} className="flex items-start gap-2.5 p-2.5 rounded-lg hover:bg-accent/30 transition-colors">
            <div className="w-8 h-8 rounded-full bg-primary/20 flex items-center justify-center shrink-0 text-xs font-semibold text-primary overflow-hidden relative">
              {c.name[0]?.toUpperCase() ?? '?'}
              {c.photo_url && <img src={c.photo_url} alt={c.name} className="absolute inset-0 w-full h-full object-cover" onError={e => { (e.target as HTMLImageElement).remove() }} />}
            </div>
            <div className="flex-1 min-w-0">
              <p className="text-xs font-medium text-foreground">{c.name}</p>
              {c.emails.map((e, j) => (
                <p key={j} className="flex items-center gap-1 text-[11px] text-muted-foreground">
                  <Mail size={9} /> {e}
                </p>
              ))}
              {c.phones.map((p, j) => (
                <p key={j} className="flex items-center gap-1 text-[11px] text-muted-foreground">
                  <Phone size={9} /> {p}
                </p>
              ))}
            </div>
          </div>
        ))}
        {loading && (
          <div className="flex justify-center py-3">
            <RefreshCw size={14} className="animate-spin text-muted-foreground" />
          </div>
        )}
      </div>
    </div>
  )
}
