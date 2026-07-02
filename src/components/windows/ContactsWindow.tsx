import { useEffect, useMemo, useRef, useState } from 'react'
import {
  Users, RefreshCw, Search, Phone, Mail, ArrowUpAZ, ArrowDownAZ, ArrowLeft,
  Copy, Check, Cake, Pencil, X, Plus, Trash2, Building2, Globe, MapPin, FileText, Save
} from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import Fuse from 'fuse.js'

interface LabeledValue {
  value: string
  label: string
}

interface ContactAddress {
  street: string
  city: string
  region: string
  postal_code: string
  country: string
  label: string
}

interface Contact {
  id: string
  etag: string
  display_name: string
  given_name: string
  family_name: string
  middle_name: string
  name_prefix: string
  name_suffix: string
  nickname: string
  emails: LabeledValue[]
  phones: LabeledValue[]
  addresses: ContactAddress[]
  organization: string
  job_title: string
  department: string
  birthday: string | null
  anniversary: string | null
  website: string
  notes: string
  photo_url: string | null
}

interface ContactsPage {
  contacts: Contact[]
  next_page_token: string | null
}

let cache: Contact[] | null = null

const PHONE_LABELS = ['Mobile', 'Home', 'Work', 'Fax Home', 'Fax Work', 'Other']
const EMAIL_LABELS = ['Home', 'Work', 'Other']
const ADDR_LABELS = ['Home', 'Work', 'Other']

function nameHue(name: string): number {
  let h = 0
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xffff
  return h % 360
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)
  const copy = () => {
    navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }
  return (
    <button
      onClick={copy}
      className="flex items-center gap-1 px-2 py-0.5 rounded text-[10px] text-muted-foreground hover:text-foreground hover:bg-accent/40 transition-colors"
      title="Copy"
    >
      {copied ? <Check size={10} className="text-green-400" /> : <Copy size={10} />}
    </button>
  )
}

function ContactRow({ contact: c, onClick }: { contact: Contact; onClick: () => void }) {
  const hue = nameHue(c.display_name)
  const initial = c.display_name[0]?.toUpperCase() ?? '?'
  const sub = c.emails[0]?.value ?? c.phones[0]?.value ?? null
  return (
    <div
      onClick={onClick}
      className="flex items-center gap-2.5 px-3 py-2 hover:bg-accent/20 transition-colors cursor-pointer border-b border-border/20 last:border-0"
    >
      <div
        className="w-8 h-8 rounded-full flex items-center justify-center shrink-0 text-xs font-semibold overflow-hidden relative"
        style={{ background: `hsl(${hue} 45% 28%)`, color: `hsl(${hue} 60% 78%)` }}
      >
        {initial}
        {c.photo_url && (
          <img src={c.photo_url} alt={c.display_name} className="absolute inset-0 w-full h-full object-cover"
            onError={e => { (e.target as HTMLImageElement).remove() }} />
        )}
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-xs font-medium text-foreground leading-tight">{c.display_name}</p>
        {sub && <p className="text-[11px] text-muted-foreground truncate mt-0.5">{sub}</p>}
      </div>
    </div>
  )
}

// ── Edit helpers ──────────────────────────────────────────────────────────────

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="mb-3">
      <p className="text-[10px] uppercase tracking-wider text-muted-foreground mb-1">{label}</p>
      {children}
    </div>
  )
}

function TextInput({ value, onChange, placeholder, className }: { value: string; onChange: (v: string) => void; placeholder?: string; className?: string }) {
  return (
    <input
      value={value}
      onChange={e => onChange(e.target.value)}
      placeholder={placeholder}
      className={`w-full bg-secondary/60 border border-border/40 rounded px-2 py-1 text-xs text-foreground placeholder:text-muted-foreground/50 outline-none focus:border-primary/50 ${className ?? ''}`}
    />
  )
}

function LabelSelect({ value, onChange, options }: { value: string; onChange: (v: string) => void; options: string[] }) {
  return (
    <select
      value={value}
      onChange={e => onChange(e.target.value)}
      className="bg-secondary/60 border border-border/40 rounded px-1.5 py-1 text-[11px] text-muted-foreground outline-none focus:border-primary/50 shrink-0"
    >
      {options.map(o => <option key={o} value={o}>{o}</option>)}
      {!options.includes(value) && <option value={value}>{value}</option>}
    </select>
  )
}

function LabeledList({
  items, onChange, labels, placeholder
}: {
  items: LabeledValue[]
  onChange: (items: LabeledValue[]) => void
  labels: string[]
  placeholder: string
}) {
  const add = () => onChange([...items, { value: '', label: labels[0] }])
  const remove = (i: number) => onChange(items.filter((_, idx) => idx !== i))
  const set = (i: number, patch: Partial<LabeledValue>) =>
    onChange(items.map((item, idx) => idx === i ? { ...item, ...patch } : item))

  return (
    <div className="space-y-1.5">
      {items.map((item, i) => (
        <div key={i} className="flex items-center gap-1.5">
          <LabelSelect value={item.label} onChange={v => set(i, { label: v })} options={labels} />
          <TextInput value={item.value} onChange={v => set(i, { value: v })} placeholder={placeholder} className="flex-1" />
          <button onClick={() => remove(i)} className="text-muted-foreground hover:text-red-400 transition-colors p-0.5">
            <Trash2 size={11} />
          </button>
        </div>
      ))}
      <button onClick={add} className="flex items-center gap-1 text-[11px] text-primary/70 hover:text-primary transition-colors">
        <Plus size={11} /> Add
      </button>
    </div>
  )
}

function AddressList({ items, onChange }: { items: ContactAddress[]; onChange: (items: ContactAddress[]) => void }) {
  const add = () => onChange([...items, { street: '', city: '', region: '', postal_code: '', country: '', label: 'Home' }])
  const remove = (i: number) => onChange(items.filter((_, idx) => idx !== i))
  const set = (i: number, patch: Partial<ContactAddress>) =>
    onChange(items.map((item, idx) => idx === i ? { ...item, ...patch } : item))

  return (
    <div className="space-y-3">
      {items.map((addr, i) => (
        <div key={i} className="border border-border/30 rounded p-2 space-y-1.5">
          <div className="flex items-center justify-between">
            <LabelSelect value={addr.label} onChange={v => set(i, { label: v })} options={ADDR_LABELS} />
            <button onClick={() => remove(i)} className="text-muted-foreground hover:text-red-400 transition-colors p-0.5">
              <Trash2 size={11} />
            </button>
          </div>
          <TextInput value={addr.street} onChange={v => set(i, { street: v })} placeholder="Street address" />
          <div className="flex gap-1.5">
            <TextInput value={addr.city} onChange={v => set(i, { city: v })} placeholder="City" className="flex-1" />
            <TextInput value={addr.region} onChange={v => set(i, { region: v })} placeholder="State" className="w-16" />
          </div>
          <div className="flex gap-1.5">
            <TextInput value={addr.postal_code} onChange={v => set(i, { postal_code: v })} placeholder="ZIP" className="w-20" />
            <TextInput value={addr.country} onChange={v => set(i, { country: v })} placeholder="Country" className="flex-1" />
          </div>
        </div>
      ))}
      <button onClick={add} className="flex items-center gap-1 text-[11px] text-primary/70 hover:text-primary transition-colors">
        <Plus size={11} /> Add address
      </button>
    </div>
  )
}

// ── Detail / Edit view ────────────────────────────────────────────────────────

function ContactDetail({
  contact: initial,
  onBack,
  onUpdate,
}: {
  contact: Contact
  onBack: () => void
  onUpdate: (c: Contact) => void
}) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState<Contact>(initial)
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)
  const contact = editing ? draft : initial
  const hue = nameHue(initial.display_name)
  const initials = initial.display_name.split(' ').map(w => w[0]).filter(Boolean).slice(0, 2).join('').toUpperCase() || '?'

  const startEdit = () => { setDraft(initial); setEditing(true); setSaveError(null) }
  const cancelEdit = () => { setEditing(false); setSaveError(null) }

  const save = async () => {
    setSaving(true)
    setSaveError(null)
    try {
      const updated = await invoke<Contact>('update_contact', { contact: draft })
      onUpdate(updated)
      setEditing(false)
    } catch (e) {
      setSaveError(String(e))
    } finally {
      setSaving(false)
    }
  }

  const p = <K extends keyof Contact>(k: K) => (v: Contact[K]) => setDraft(d => ({ ...d, [k]: v }))

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Hero */}
      <div className="relative shrink-0">
        <div
          className="h-32 overflow-hidden"
          style={{ background: `linear-gradient(135deg, hsl(${hue} 40% 18%), hsl(${hue} 30% 10%))` }}
        >
          {initial.photo_url && (
            <img src={initial.photo_url} alt="" className="w-full h-full object-cover scale-110 blur-md opacity-40" />
          )}
        </div>
        <Button variant="ghost" size="icon-xs" onClick={onBack}
          className="absolute top-2 left-2 bg-black/30 backdrop-blur-sm hover:bg-black/50" title="Back">
          <ArrowLeft size={13} />
        </Button>
        {!editing && (
          <Button variant="ghost" size="icon-xs" onClick={startEdit}
            className="absolute top-2 right-2 bg-black/30 backdrop-blur-sm hover:bg-black/50" title="Edit">
            <Pencil size={13} />
          </Button>
        )}
        {/* Avatar */}
        <div className="absolute -bottom-12 left-4">
          <div
            className="w-24 h-24 rounded-full border-[3px] border-background flex items-center justify-center text-2xl font-bold overflow-hidden relative shadow-lg"
            style={{ background: `hsl(${hue} 45% 30%)`, color: `hsl(${hue} 60% 80%)` }}
          >
            {initials}
            {initial.photo_url && (
              <img src={initial.photo_url} alt={initial.display_name}
                className="absolute inset-0 w-full h-full object-cover"
                onError={e => { (e.target as HTMLImageElement).remove() }} />
            )}
          </div>
        </div>
      </div>

      {/* Scrollable body */}
      <div className="flex-1 overflow-y-auto pt-14 pb-4">
        {saveError && (
          <div className="mx-4 mb-3 px-3 py-2 rounded bg-red-950/40 border border-red-800/40 text-red-400 text-xs">{saveError}</div>
        )}

        {editing ? (
          <div className="px-4">
            {/* Save/Cancel bar */}
            <div className="flex gap-2 mb-4">
              <Button size="sm" className="flex-1 h-7 text-xs" onClick={save} disabled={saving}>
                {saving ? <RefreshCw size={11} className="animate-spin mr-1" /> : <Save size={11} className="mr-1" />}
                Save
              </Button>
              <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={cancelEdit}>
                <X size={11} className="mr-1" /> Cancel
              </Button>
            </div>

            <Field label="Name">
              <div className="flex gap-1.5 mb-1.5">
                <TextInput value={draft.name_prefix} onChange={p('name_prefix')} placeholder="Prefix" className="w-16" />
                <TextInput value={draft.given_name} onChange={p('given_name')} placeholder="First" className="flex-1" />
                <TextInput value={draft.middle_name} onChange={p('middle_name')} placeholder="Middle" className="w-20" />
              </div>
              <div className="flex gap-1.5">
                <TextInput value={draft.family_name} onChange={p('family_name')} placeholder="Last" className="flex-1" />
                <TextInput value={draft.name_suffix} onChange={p('name_suffix')} placeholder="Suffix" className="w-16" />
              </div>
            </Field>

            <Field label="Nickname">
              <TextInput value={draft.nickname} onChange={p('nickname')} placeholder="Nickname" />
            </Field>

            <Field label="Company">
              <TextInput value={draft.organization} onChange={p('organization')} placeholder="Company" className="mb-1.5" />
              <div className="flex gap-1.5">
                <TextInput value={draft.job_title} onChange={p('job_title')} placeholder="Job title" className="flex-1" />
                <TextInput value={draft.department} onChange={p('department')} placeholder="Department" className="flex-1" />
              </div>
            </Field>

            <Field label="Phone">
              <LabeledList items={draft.phones} onChange={v => setDraft(d => ({ ...d, phones: v }))} labels={PHONE_LABELS} placeholder="Phone number" />
            </Field>

            <Field label="Email">
              <LabeledList items={draft.emails} onChange={v => setDraft(d => ({ ...d, emails: v }))} labels={EMAIL_LABELS} placeholder="Email address" />
            </Field>

            <Field label="Address">
              <AddressList items={draft.addresses} onChange={v => setDraft(d => ({ ...d, addresses: v }))} />
            </Field>

            <Field label="Birthday">
              <TextInput value={draft.birthday ?? ''} onChange={v => setDraft(d => ({ ...d, birthday: v || null }))} placeholder="YYYY-MM-DD or --MM-DD" />
            </Field>

            <Field label="Anniversary">
              <TextInput value={draft.anniversary ?? ''} onChange={v => setDraft(d => ({ ...d, anniversary: v || null }))} placeholder="YYYY-MM-DD or --MM-DD" />
            </Field>

            <Field label="Website">
              <TextInput value={draft.website} onChange={p('website')} placeholder="https://" />
            </Field>

            <Field label="Notes">
              <textarea
                value={draft.notes}
                onChange={e => setDraft(d => ({ ...d, notes: e.target.value }))}
                placeholder="Notes..."
                rows={4}
                className="w-full bg-secondary/60 border border-border/40 rounded px-2 py-1 text-xs text-foreground placeholder:text-muted-foreground/50 outline-none focus:border-primary/50 resize-none"
              />
            </Field>
          </div>
        ) : (
          <>
            {/* View mode */}
            <div className="px-4 pb-3 border-b border-border">
              <p className="text-base font-semibold text-foreground leading-tight">{contact.display_name}</p>
              {contact.nickname && <p className="text-[11px] text-muted-foreground mt-0.5">"{contact.nickname}"</p>}
              {(contact.job_title || contact.organization) && (
                <p className="text-[11px] text-muted-foreground mt-1">
                  {[contact.job_title, contact.organization].filter(Boolean).join(' · ')}
                </p>
              )}
              {contact.birthday && (
                <p className="flex items-center gap-1 text-[11px] text-muted-foreground mt-1">
                  <Cake size={10} className="opacity-60" /> {contact.birthday}
                </p>
              )}
            </div>

            {contact.phones.length > 0 && (
              <div className="px-4 py-3 border-b border-border/40">
                <p className="text-[10px] uppercase tracking-wider text-muted-foreground mb-2">Phone</p>
                {contact.phones.map((p, i) => (
                  <div key={i} className="group flex items-center gap-2.5 py-1.5">
                    <div className="w-7 h-7 rounded-full bg-accent/30 flex items-center justify-center shrink-0">
                      <Phone size={11} className="text-muted-foreground" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <a href={`tel:${p.value}`} className="text-xs text-foreground hover:underline decoration-muted-foreground/40">{p.value}</a>
                      <p className="text-[10px] text-muted-foreground/60">{p.label}</p>
                    </div>
                    <span className="opacity-0 group-hover:opacity-100 transition-opacity"><CopyButton text={p.value} /></span>
                  </div>
                ))}
              </div>
            )}

            {contact.emails.length > 0 && (
              <div className="px-4 py-3 border-b border-border/40">
                <p className="text-[10px] uppercase tracking-wider text-muted-foreground mb-2">Email</p>
                {contact.emails.map((e, i) => (
                  <div key={i} className="group flex items-center gap-2.5 py-1.5">
                    <div className="w-7 h-7 rounded-full bg-accent/30 flex items-center justify-center shrink-0">
                      <Mail size={11} className="text-muted-foreground" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <a href={`mailto:${e.value}`} className="text-xs text-foreground truncate block hover:underline decoration-muted-foreground/40">{e.value}</a>
                      <p className="text-[10px] text-muted-foreground/60">{e.label}</p>
                    </div>
                    <span className="opacity-0 group-hover:opacity-100 transition-opacity shrink-0"><CopyButton text={e.value} /></span>
                  </div>
                ))}
              </div>
            )}

            {contact.addresses.length > 0 && (
              <div className="px-4 py-3 border-b border-border/40">
                <p className="text-[10px] uppercase tracking-wider text-muted-foreground mb-2">Address</p>
                {contact.addresses.map((a, i) => (
                  <div key={i} className="group flex items-start gap-2.5 py-1.5">
                    <div className="w-7 h-7 rounded-full bg-accent/30 flex items-center justify-center shrink-0 mt-0.5">
                      <MapPin size={11} className="text-muted-foreground" />
                    </div>
                    <div className="flex-1">
                      {a.street && <p className="text-xs text-foreground">{a.street}</p>}
                      {(a.city || a.region || a.postal_code) && (
                        <p className="text-xs text-foreground">{[a.city, a.region, a.postal_code].filter(Boolean).join(', ')}</p>
                      )}
                      {a.country && <p className="text-xs text-foreground">{a.country}</p>}
                      <p className="text-[10px] text-muted-foreground/60">{a.label}</p>
                    </div>
                  </div>
                ))}
              </div>
            )}

            {contact.organization && (
              <div className="px-4 py-3 border-b border-border/40">
                <p className="text-[10px] uppercase tracking-wider text-muted-foreground mb-2">Work</p>
                <div className="flex items-start gap-2.5">
                  <div className="w-7 h-7 rounded-full bg-accent/30 flex items-center justify-center shrink-0">
                    <Building2 size={11} className="text-muted-foreground" />
                  </div>
                  <div>
                    <p className="text-xs text-foreground">{contact.organization}</p>
                    {contact.job_title && <p className="text-[11px] text-muted-foreground">{contact.job_title}</p>}
                    {contact.department && <p className="text-[11px] text-muted-foreground/60">{contact.department}</p>}
                  </div>
                </div>
              </div>
            )}

            {contact.website && (
              <div className="px-4 py-3 border-b border-border/40">
                <p className="text-[10px] uppercase tracking-wider text-muted-foreground mb-2">Website</p>
                <div className="group flex items-center gap-2.5">
                  <div className="w-7 h-7 rounded-full bg-accent/30 flex items-center justify-center shrink-0">
                    <Globe size={11} className="text-muted-foreground" />
                  </div>
                  <p className="text-xs text-foreground truncate flex-1">{contact.website}</p>
                  <span className="opacity-0 group-hover:opacity-100 transition-opacity shrink-0"><CopyButton text={contact.website} /></span>
                </div>
              </div>
            )}

            {contact.anniversary && (
              <div className="px-4 py-3 border-b border-border/40">
                <p className="text-[10px] uppercase tracking-wider text-muted-foreground mb-1">Anniversary</p>
                <p className="text-xs text-foreground">{contact.anniversary}</p>
              </div>
            )}

            {contact.notes && (
              <div className="px-4 py-3">
                <p className="text-[10px] uppercase tracking-wider text-muted-foreground mb-2">Notes</p>
                <div className="flex items-start gap-2.5">
                  <div className="w-7 h-7 rounded-full bg-accent/30 flex items-center justify-center shrink-0">
                    <FileText size={11} className="text-muted-foreground" />
                  </div>
                  <p className="text-xs text-foreground whitespace-pre-wrap flex-1">{contact.notes}</p>
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}

// ── Main window ───────────────────────────────────────────────────────────────

export function ContactsWindow() {
  const [allContacts, setAllContacts] = useState<Contact[]>(cache ?? [])
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [sortAsc, setSortAsc] = useState(true)
  const [selected, setSelected] = useState<Contact | null>(null)
  const loadingRef = useRef(false)

  const fuse = useMemo(
    () => new Fuse(allContacts, { keys: ['display_name', 'emails.value', 'phones.value'], threshold: 0.4 }),
    [allContacts]
  )

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
      const sorted = all.sort((a, b) => a.display_name.localeCompare(b.display_name))
      cache = sorted
      setAllContacts(sorted)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
      loadingRef.current = false
    }
  }

  const refresh = () => { cache = null; setAllContacts([]); fetchAll() }

  useEffect(() => { if (!cache) fetchAll() }, [])

  const handleUpdate = (updated: Contact) => {
    setAllContacts(prev => {
      const next = prev.map(c => c.id === updated.id ? updated : c)
      cache = next
      return next
    })
    setSelected(updated)
  }

  const grouped = useMemo(() => {
    if (query.trim()) return null
    const groups: Record<string, Contact[]> = {}
    for (const c of contacts) {
      const letter = (c.display_name[0] ?? '#').toUpperCase()
      ;(groups[letter] ??= []).push(c)
    }
    return groups
  }, [contacts, query])

  if (selected) {
    return (
      <ContactDetail
        contact={selected}
        onBack={() => setSelected(null)}
        onUpdate={handleUpdate}
      />
    )
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-border">
        <div className="flex-1 flex items-center gap-1.5 bg-secondary rounded-md px-2 py-1.5">
          <Search size={12} className="text-muted-foreground shrink-0" />
          <input
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder="Search contacts..."
            className="bg-transparent text-xs text-foreground placeholder:text-muted-foreground outline-none flex-1"
          />
        </div>
        <Button variant="ghost" size="icon-xs" onClick={() => setSortAsc(v => !v)} title={sortAsc ? 'A→Z' : 'Z→A'}>
          {sortAsc ? <ArrowUpAZ size={13} /> : <ArrowDownAZ size={13} />}
        </Button>
        <Button variant="ghost" size="icon-xs" onClick={refresh} disabled={loading} title="Refresh">
          <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto">
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

        {grouped
          ? Object.entries(grouped).map(([letter, group]) => (
              <div key={letter}>
                <div className="px-3 py-1 text-[10px] font-semibold text-muted-foreground/60 uppercase tracking-wider bg-secondary/30 border-b border-border/30 sticky top-0">
                  {letter}
                </div>
                {group.map((c, i) => (
                  <ContactRow key={i} contact={c} onClick={() => setSelected(c)} />
                ))}
              </div>
            ))
          : contacts.map((c, i) => (
              <ContactRow key={i} contact={c} onClick={() => setSelected(c)} />
            ))
        }

        {loading && (
          <div className="flex justify-center py-4">
            <RefreshCw size={14} className="animate-spin text-muted-foreground" />
          </div>
        )}
      </div>
    </div>
  )
}
