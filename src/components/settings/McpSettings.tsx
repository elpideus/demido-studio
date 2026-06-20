import { useEffect, useState } from 'react'
import { Plus, Trash2, Pencil, Loader2, CheckCircle, XCircle, Wrench } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { mcp } from '../../lib/tauri'
import { useMcpTools } from '../../stores/mcpTools'
import type { McpServer } from '../../types'

type TestState = 'idle' | 'testing' | 'ok' | 'error'

interface ServerFormProps {
  server: McpServer
  onSave: (srv: McpServer) => Promise<void>
  onCancel?: () => void
  onRemove: () => void
}

function ServerForm({ server, onSave, onCancel, onRemove }: ServerFormProps) {
  const [draft, setDraft] = useState(server)
  const [saving, setSaving] = useState(false)
  const [testState, setTestState] = useState<TestState>('idle')
  const [testError, setTestError] = useState('')
  const [toolCount, setToolCount] = useState<number | null>(null)
  const [argsStr, setArgsStr] = useState(() => server.args?.length ? JSON.stringify(server.args) : '')
  const [envStr, setEnvStr] = useState(() => server.env && Object.keys(server.env).length ? JSON.stringify(server.env) : '')

  useEffect(() => {
    setArgsStr(server.args?.length ? JSON.stringify(server.args) : '')
    setEnvStr(server.env && Object.keys(server.env).length ? JSON.stringify(server.env) : '')
  }, [server])

  const patch = (p: Partial<McpServer>) => setDraft(d => ({ ...d, ...p }))

  const syncArgs = () => {
    if (!argsStr.trim()) { patch({ args: [] }); return }
    try {
      const v = JSON.parse(argsStr)
      if (Array.isArray(v)) patch({ args: v })
    } catch { /* keep previous args */ }
  }

  const syncEnv = () => {
    if (!envStr.trim()) { patch({ env: {} }); return }
    try {
      const v = JSON.parse(envStr)
      if (typeof v === 'object' && !Array.isArray(v)) patch({ env: v })
    } catch { /* keep previous env */ }
  }

  const handleTest = async () => {
    syncArgs()
    syncEnv()
    setTestState('testing')
    setTestError('')
    setToolCount(null)
    try {
      const count = await mcp.testServer(draft)
      setToolCount(count)
      setTestState('ok')
    } catch (e) {
      setTestError(String(e))
      setTestState('error')
    }
  }

  const handleSave = async () => {
    syncArgs()
    syncEnv()
    setSaving(true)
    try {
      await onSave(draft)
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="border border-border rounded-xl p-4 space-y-3">
      <div className="flex items-center gap-3">
        <Input value={draft.name} onChange={e => patch({ name: e.target.value })} placeholder="Server name" className="flex-1 h-8 text-sm" />
        <select
          value={draft.transport}
          onChange={e => patch({ transport: e.target.value as 'stdio' | 'sse' })}
          className="bg-background border border-border rounded-lg px-3 py-1.5 text-sm text-foreground outline-none"
        >
          <option value="stdio">stdio</option>
          <option value="sse">SSE</option>
        </select>
        <Button onClick={onRemove} variant="ghost" size="icon-sm" className="text-muted-foreground hover:text-destructive"><Trash2 size={14} /></Button>
      </div>

      {draft.transport === 'stdio' ? (
        <>
          <Input value={draft.command ?? ''} onChange={e => patch({ command: e.target.value })} placeholder="Command (e.g. npx)" className="h-8 text-sm" />
          <Input
            value={argsStr}
            onChange={e => {
              setArgsStr(e.target.value)
              try {
                const v = JSON.parse(e.target.value)
                if (Array.isArray(v)) patch({ args: v })
              } catch {}
            }}
            placeholder='Arguments (array, e.g. ["-y", "mcp-searxng"])'
            className="h-8 text-sm"
          />
          <Input
            value={envStr}
            onChange={e => {
              setEnvStr(e.target.value)
              try {
                const v = JSON.parse(e.target.value)
                if (typeof v === 'object' && !Array.isArray(v)) patch({ env: v })
              } catch {}
            }}
            placeholder='Env vars (object, e.g. {"SEARXNG_URL": "..."})'
            className="h-8 text-sm"
          />
        </>
      ) : (
        <Input value={draft.url ?? ''} onChange={e => patch({ url: e.target.value })} placeholder="SSE endpoint URL" className="h-8 text-sm" />
      )}

      {testState === 'error' && (
        <p className="text-xs text-red-400">{testError}</p>
      )}
      {testState === 'ok' && toolCount !== null && (
        <p className="text-xs text-green-400">Connected — {toolCount} tool{toolCount !== 1 ? 's' : ''} found</p>
      )}

      <div className="flex items-center gap-2 pt-1">
        <Button onClick={handleTest} disabled={testState === 'testing'} variant="secondary" size="xs" className="gap-1.5">
          {testState === 'testing' ? <Loader2 size={12} className="animate-spin" /> : testState === 'ok' ? <CheckCircle size={12} className="text-green-400" /> : testState === 'error' ? <XCircle size={12} className="text-red-400" /> : <Wrench size={12} />}
          Test
        </Button>
        {onCancel && <Button onClick={onCancel} variant="secondary" size="xs">Cancel</Button>}
        <Button onClick={handleSave} disabled={saving || !draft.name} size="xs" className="ml-auto gap-1.5">
          {saving && <Loader2 size={12} className="animate-spin" />}
          Save
        </Button>
      </div>
    </div>
  )
}

interface SavedCardProps {
  server: McpServer
  toolCount: number | null
  onEdit: () => void
}

function SavedCard({ server, toolCount, onEdit }: SavedCardProps) {
  return (
    <div className="flex items-center gap-3 border border-border rounded-xl px-4 py-3">
      <div className="flex-1 min-w-0">
        <p className="text-sm text-foreground truncate">{server.name || 'Unnamed'}</p>
        <p className="text-xs text-muted-foreground mt-0.5">
          {server.transport === 'stdio' ? server.command : server.url}
          {toolCount !== null && <span className="ml-2 text-primary">{toolCount} tool{toolCount !== 1 ? 's' : ''}</span>}
        </p>
      </div>
      <Button onClick={onEdit} variant="secondary" size="xs" className="gap-1 text-muted-foreground"><Pencil size={11} /> Edit</Button>
    </div>
  )
}

interface ServerEntry {
  server: McpServer
  editing: boolean
  toolCount: number | null
}

export function McpSettings() {
  const [entries, setEntries] = useState<ServerEntry[]>([])
  const reloadMcpTools = useMcpTools(s => s.load)

  useEffect(() => {
    mcp.listServers().then(servers => {
      setEntries(servers.map(s => ({ server: s, editing: false, toolCount: null })))
    })
  }, [])

  const addNew = () => setEntries(e => [...e, {
    server: {
      id: crypto.randomUUID(),
      name: '',
      transport: 'stdio',
      command: '',
      args: [],
      enabled: true,
    },
    editing: true,
    toolCount: null,
  }])

  const handleSave = async (id: string, updated: McpServer) => {
    const newEntries = entries.map(e =>
      e.server.id === id ? { ...e, server: updated, editing: false } : e
    )
    await mcp.saveServers(newEntries.map(e => e.server))
    setEntries(newEntries)
    reloadMcpTools()
  }

  const handleRemove = async (id: string) => {
    const newEntries = entries.filter(e => e.server.id !== id)
    await mcp.saveServers(newEntries.map(e => e.server))
    setEntries(newEntries)
    reloadMcpTools()
  }

  const handleEdit = (id: string) =>
    setEntries(e => e.map(x => x.server.id === id ? { ...x, editing: true } : x))

  const handleCancel = (id: string) =>
    setEntries(e => {
      const entry = e.find(x => x.server.id === id)
      // If name is empty it was never saved — remove it
      if (entry && !entry.server.name) return e.filter(x => x.server.id !== id)
      return e.map(x => x.server.id === id ? { ...x, editing: false } : x)
    })

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-foreground">MCP Servers</h3>
        <Button onClick={addNew} variant="secondary" size="xs" className="gap-1"><Plus size={12} /> Add</Button>
      </div>

      {entries.map(({ server, editing, toolCount }) =>
        editing ? (
          <ServerForm
            key={server.id}
            server={server}
            onSave={updated => handleSave(server.id, updated)}
            onCancel={() => handleCancel(server.id)}
            onRemove={() => handleRemove(server.id)}
          />
        ) : (
          <SavedCard
            key={server.id}
            server={server}
            toolCount={toolCount}
            onEdit={() => handleEdit(server.id)}
          />
        )
      )}
    </div>
  )
}
