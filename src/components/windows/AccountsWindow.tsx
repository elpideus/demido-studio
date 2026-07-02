import { useState, useEffect } from 'react'
import { Plus, Mail, Calendar, Users, Trash2, AlertCircle, ChevronDown, Check } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { invoke } from '@tauri-apps/api/core'

// Module-level ref so title bar component can call into AccountsWindow state
export const accountsWindowRef = { triggerGoogle: () => {} }

interface Account {
  id: string
  provider: string
  email: string
  name: string
  picture?: string
  services: string[]
}

function CredentialModal({ onClose, onSave }: { onClose: () => void; onSave: (id: string, secret: string) => void }) {
  const [clientId, setClientId] = useState('')
  const [clientSecret, setClientSecret] = useState('')

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-background border border-border rounded-xl shadow-2xl max-w-md w-full mx-4 p-6 flex flex-col gap-4">
        <h2 className="text-base font-semibold text-foreground">Google OAuth Credentials</h2>
        <p className="text-xs text-muted-foreground leading-relaxed">
          Enter your Google Cloud OAuth 2.0 credentials. Create them at{' '}
          <span className="text-primary">console.cloud.google.com</span> under APIs & Services → Credentials. Add <code className="text-[10px] bg-secondary px-1 rounded">http://localhost</code> as an authorized redirect URI.
        </p>
        <div className="flex flex-col gap-2">
          <input
            type="text"
            placeholder="Client ID"
            value={clientId}
            onChange={e => setClientId(e.target.value)}
            className="bg-secondary border border-border rounded-md px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground outline-none focus:border-ring/50"
          />
          <input
            type="password"
            placeholder="Client Secret"
            value={clientSecret}
            onChange={e => setClientSecret(e.target.value)}
            className="bg-secondary border border-border rounded-md px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground outline-none focus:border-ring/50"
          />
        </div>
        <div className="flex gap-2 justify-end">
          <Button variant="secondary" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" disabled={!clientId.trim()} onClick={() => onSave(clientId.trim(), clientSecret.trim())}>
            Save & Continue
          </Button>
        </div>
      </div>
    </div>
  )
}


function ServiceToggleModal({ account, excludeService, onConfirm }: {
  account: Account
  excludeService: string
  onConfirm: (services: string[]) => void
}) {
  const ALL_SERVICES = ['email', 'calendar', 'contacts'].filter(s => s !== excludeService)
  const [selected, setSelected] = useState<string[]>(ALL_SERVICES)

  const toggle = (s: string) => setSelected(prev =>
    prev.includes(s) ? prev.filter(x => x !== s) : [...prev, s]
  )

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-background border border-border rounded-xl shadow-2xl max-w-sm w-full mx-4 p-6 flex flex-col gap-4">
        <h2 className="text-base font-semibold text-foreground">Add to other services?</h2>
        <p className="text-xs text-muted-foreground">
          <strong>{account.email}</strong> was added. Also connect it to these services?
        </p>
        <div className="flex flex-col gap-2">
          {ALL_SERVICES.map(s => {
            const icons: Record<string, React.ReactNode> = {
              email: <Mail size={14} />,
              calendar: <Calendar size={14} />,
              contacts: <Users size={14} />,
            }
            const active = selected.includes(s)
            return (
              <button
                key={s}
                onClick={() => toggle(s)}
                className={`flex items-center gap-3 px-3 py-2.5 rounded-lg border text-sm text-left transition-colors ${
                  active
                    ? 'border-primary/60 bg-primary/10 text-foreground hover:bg-primary/15'
                    : 'border-border bg-secondary/30 text-muted-foreground hover:bg-secondary/60'
                }`}
              >
                <span className={active ? 'text-primary' : 'text-muted-foreground'}>{icons[s]}</span>
                <span className="capitalize flex-1">{s}</span>
                <span className={`w-4 h-4 rounded-full border flex items-center justify-center shrink-0 ${
                  active ? 'border-primary bg-primary text-primary-foreground' : 'border-border'
                }`}>
                  {active && <Check size={10} strokeWidth={3} />}
                </span>
              </button>
            )
          })}
        </div>
        <div className="flex gap-2 justify-end">
          <Button variant="secondary" size="sm" onClick={() => onConfirm([])}>Skip</Button>
          <Button size="sm" onClick={() => onConfirm(selected)}>Add to Selected</Button>
        </div>
      </div>
    </div>
  )
}

type Tab = 'email' | 'calendar' | 'contacts'

export function AccountsWindow() {
  const [tab, setTab] = useState<Tab>('email')
  const [accounts, setAccounts] = useState<Account[]>([])
  const [showCredModal, setShowCredModal] = useState(false)
  const [newAccount, setNewAccount] = useState<Account | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadAccounts = async () => {
    try {
      const list = await invoke<Account[]>('list_accounts')
      setAccounts(list)
    } catch {}
  }

  useEffect(() => { loadAccounts() }, [])

  useEffect(() => {
    accountsWindowRef.triggerGoogle = handleGoogleClick
    return () => { accountsWindowRef.triggerGoogle = () => {} }
  })

  const handleGoogleClick = async () => {
    // Check if credentials exist
    try {
      const has = await invoke<boolean>('has_google_credentials')
      if (!has) {
        setShowCredModal(true)
        return
      }
      startOAuth()
    } catch (e) {
      setError(String(e))
    }
  }

  const handleSaveCredentials = async (clientId: string, clientSecret: string) => {
    try {
      await invoke('set_google_credentials', { clientId, clientSecret })
      setShowCredModal(false)
      startOAuth()
    } catch (e) {
      setError(String(e))
    }
  }

  const startOAuth = async () => {
    setLoading(true)
    setError(null)
    try {
      const account = await invoke<Account>('initiate_google_oauth')
      setNewAccount(account)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  const handleServiceConfirm = async (services: string[]) => {
    if (!newAccount) return
    try {
      // Always include the originating tab's service
      const merged = Array.from(new Set([tab, ...services]))
      await invoke('update_account_services', { accountId: newAccount.id, services: merged })
      setNewAccount(null)
      loadAccounts()
    } catch (e) {
      setError(String(e))
    }
  }

  const handleDelete = async (acc: Account) => {
    try {
      const remaining = acc.services.filter(s => s !== tab)
      if (remaining.length === 0) {
        await invoke('delete_account', { accountId: acc.id })
      } else {
        await invoke('update_account_services', { accountId: acc.id, services: remaining })
      }
      loadAccounts()
    } catch (e) {
      setError(String(e))
    }
  }

  const filtered = accounts.filter(a => a.services.includes(tab))

  const TABS: { id: Tab; label: string; icon: React.ReactNode }[] = [
    { id: 'email', label: 'Email', icon: <Mail size={13} /> },
    { id: 'calendar', label: 'Calendar', icon: <Calendar size={13} /> },
    { id: 'contacts', label: 'Contacts', icon: <Users size={13} /> },
  ]

  return (
    <div className="flex h-full text-foreground overflow-hidden">
      {/* Vertical Tabs */}
      <div className="w-44 border-r border-border p-3 space-y-0.5 shrink-0 bg-[#1b1b1b] flex flex-col">
        {TABS.map(t => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`w-full text-left px-3 py-2 rounded-md text-sm transition-colors flex items-center gap-2 ${
              tab === t.id
                ? 'bg-primary/20 text-foreground'
                : 'text-muted-foreground hover:bg-accent hover:text-foreground'
            }`}
          >
            {t.icon} {t.label}
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-4">
        {error && (
          <div className="mb-3 flex items-center gap-2 px-3 py-2 rounded-lg bg-red-950/30 border border-red-800/40 text-red-400 text-xs">
            <AlertCircle size={13} className="shrink-0" />
            {error}
          </div>
        )}

        {loading && (
          <div className="flex items-center justify-center h-24 text-sm text-muted-foreground">
            Waiting for Google authorization in browser…
          </div>
        )}

        {!loading && filtered.length === 0 && (
          <div className="flex flex-col items-center justify-center h-32 gap-2">
            <p className="text-sm text-muted-foreground">No accounts connected for {tab}.</p>
            <p className="text-xs text-muted-foreground/60">Click "Add Account" to connect one.</p>
          </div>
        )}

        {!loading && filtered.map(acc => (
          <div key={acc.id} className="flex items-center gap-3 p-3 rounded-lg border border-border hover:bg-accent/30 transition-colors mb-2">
            {acc.picture ? (
              <img src={acc.picture} alt={acc.name} className="w-8 h-8 rounded-full object-cover" />
            ) : (
              <div className="w-8 h-8 rounded-full bg-primary/20 flex items-center justify-center text-xs font-semibold text-primary">
                {acc.name?.[0]?.toUpperCase() ?? '?'}
              </div>
            )}
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium truncate">{acc.name || acc.email}</p>
              <p className="text-[11px] text-muted-foreground truncate">{acc.email}</p>
            </div>
            <Button
              variant="ghost"
              size="icon-xs"
              className="text-muted-foreground hover:text-red-400 shrink-0"
              onClick={() => handleDelete(acc)}
              title="Remove account"
            >
              <Trash2 size={13} />
            </Button>
          </div>
        ))}
      </div>

      {showCredModal && (
        <CredentialModal
          onClose={() => setShowCredModal(false)}
          onSave={handleSaveCredentials}
        />
      )}
      {newAccount && (
        <ServiceToggleModal
          account={newAccount}
          excludeService={tab}
          onConfirm={handleServiceConfirm}
        />
      )}
    </div>
  )
}

export function AccountsTitleBarActions() {
  const [open, setOpen] = useState(false)
  return (
    <>
      {open && <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />}
      <div className="relative z-20">
      <Button
        size="sm"
        variant="secondary"
        className="gap-1.5 text-muted-foreground h-7 px-2 text-xs bg-[#2a2a2a] hover:bg-[#333] border-0"
        onClick={() => setOpen(v => !v)}
      >
        <Plus size={13} /> Add Account <ChevronDown size={11} />
      </Button>
      {open && (
        <div className="absolute top-full mt-1 right-0 z-20 bg-popover border border-border rounded-lg shadow-xl min-w-[160px] p-1">
          <button
            className="w-full text-left px-3 py-2 text-sm text-foreground hover:bg-accent rounded-md transition-colors flex items-center gap-2"
            onClick={() => { setOpen(false); accountsWindowRef.triggerGoogle() }}
          >
            <img src="https://www.google.com/favicon.ico" alt="Google" className="w-4 h-4" onError={e => { (e.target as HTMLImageElement).style.display='none' }} />
            Google
          </button>
          <button
            disabled
            className="w-full text-left px-3 py-2 text-sm text-muted-foreground/40 cursor-not-allowed rounded-md flex items-center gap-2"
            title="Coming soon"
          >
            <AlertCircle size={14} className="text-muted-foreground/30" />
            Custom (soon)
          </button>
        </div>
      )}
      </div>
    </>
  )
}
