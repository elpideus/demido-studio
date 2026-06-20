import { useState } from 'react'
import { db } from '../../lib/tauri'

interface Props { onUnlock: () => void }

export function AuthGate({ onUnlock }: Props) {
  const [pin, setPin] = useState('')
  const [error, setError] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    const stored = await db.getSecret('auth_pin')
    if (pin === stored) {
      onUnlock()
    } else {
      setError('Incorrect PIN')
      setPin('')
    }
  }

  return (
    <div className="flex h-screen bg-background items-center justify-center">
      <div className="bg-card border border-border rounded-2xl p-8 w-80 space-y-6">
        <div className="text-center">
          <h1 className="text-lg font-semibold text-foreground">Demido Studio</h1>
          <p className="text-sm text-muted-foreground mt-1">Enter your PIN to continue</p>
        </div>
        <form onSubmit={handleSubmit} className="space-y-4">
          <input
            type="password"
            value={pin}
            onChange={e => { setPin(e.target.value); setError('') }}
            placeholder="PIN"
            autoFocus
            className="w-full bg-background border border-border rounded-xl px-4 py-3 text-sm text-foreground placeholder:text-muted-foreground outline-none focus:border-ring/50 text-center tracking-widest"
          />
          {error && <p className="text-xs text-red-400 text-center">{error}</p>}
          <button
            type="submit"
            className="w-full py-2.5 rounded-xl bg-primary text-white text-sm font-medium hover:bg-primary/90 transition-colors"
          >
            Unlock
          </button>
        </form>
      </div>
    </div>
  )
}
