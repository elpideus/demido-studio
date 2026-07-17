import { useEffect, useMemo, useRef, useState } from 'react'
import { Share2, RefreshCw, Play, Loader2, Search, GitFork, FileText, FolderX } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useConversations } from '../../stores/conversations'
import { useGraphify } from '../../stores/graphify'

type QueryKind = 'query' | 'path' | 'explain'

const KIND_LABEL: Record<QueryKind, string> = {
  query: 'Query',
  path: 'Path',
  explain: 'Explain',
}

/** The active conversation's working directory, or null when agent mode is off / unset. */
function useActiveFolder(): string | null {
  return useConversations(s => {
    const conv = s.conversations.find(c => c.id === s.activeId)
    if (!conv || conv.agent_mode === 'off') return null
    return conv.working_directory ?? null
  })
}

export function GraphifyWindow() {
  const folder = useActiveFolder()
  const {
    statusByFolder, installing, installStage, building, buildLog,
    refreshStatus, build, ensureListeners, setAutoBuild,
  } = useGraphify()
  const status = folder ? statusByFolder[folder] : undefined

  useEffect(() => {
    ensureListeners()
    if (folder) refreshStatus(folder)
  }, [folder, ensureListeners, refreshStatus])

  if (!folder) {
    return (
      <Centered icon={<FolderX className="w-8 h-8" />}>
        <p className="text-sm">Graphify works on a conversation's working folder.</p>
        <p className="text-xs text-muted-foreground mt-1">Turn on agent mode and set a working folder to build a graph.</p>
      </Centered>
    )
  }

  const busy = installing || building
  if (busy) {
    return <ProgressView installing={installing} installStage={installStage} buildLog={buildLog} />
  }

  if (!status) {
    return <Centered icon={<Loader2 className="w-6 h-6 animate-spin" />}><p className="text-sm text-muted-foreground">Checking…</p></Centered>
  }

  if (!status.graphBuilt) {
    return (
      <InitView
        folder={folder}
        onBuild={() => build(folder, false)}
        autoBuild={status.autoBuild}
        onAutoBuildChange={(v) => setAutoBuild(folder, v)}
      />
    )
  }

  return <GraphView folder={folder} onRebuild={() => build(folder, true)} />
}

function Centered({ icon, children }: { icon: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="h-full flex flex-col items-center justify-center text-center px-8 gap-3">
      <div className="text-muted-foreground">{icon}</div>
      <div>{children}</div>
    </div>
  )
}

function ProgressView({ installing, installStage, buildLog }: { installing: boolean; installStage: string; buildLog: string[] }) {
  const logRef = useRef<HTMLDivElement>(null)
  useEffect(() => { logRef.current?.scrollTo(0, logRef.current.scrollHeight) }, [buildLog.length])
  return (
    <div className="h-full flex flex-col p-4 gap-3">
      <div className="flex items-center gap-2 text-sm">
        <Loader2 className="w-4 h-4 animate-spin text-primary" />
        {installing ? `Installing graphify — ${installStage}` : 'Building graph…'}
      </div>
      <div ref={logRef} className="flex-1 overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-xs whitespace-pre-wrap">
        {buildLog.length === 0 ? <span className="text-muted-foreground">Starting…</span> : buildLog.join('\n')}
      </div>
    </div>
  )
}

function InitView({ folder, onBuild, autoBuild, onAutoBuildChange }: {
  folder: string
  onBuild: () => void
  autoBuild: boolean
  onAutoBuildChange: (v: boolean) => void
}) {
  return (
    <Centered icon={<GitFork className="w-9 h-9" />}>
      <h2 className="text-base font-medium">No knowledge graph yet</h2>
      <p className="text-sm text-muted-foreground mt-1 max-w-md">
        Build a queryable code graph for this folder. Graphify parses the source with tree-sitter and
        writes a <code className="text-xs">graphify-out/</code> directory. The first run installs the
        bundled Python package and can take a few minutes.
      </p>
      <p className="text-[11px] text-muted-foreground/70 mt-1 break-all max-w-md">{folder}</p>
      <Button className="mt-3" onClick={onBuild}>
        <Share2 className="w-4 h-4 mr-1.5" /> Build graph
      </Button>
      <label className="mt-3 flex items-center justify-center gap-2 text-xs text-muted-foreground cursor-pointer select-none">
        <Switch size="sm" checked={autoBuild} onCheckedChange={onAutoBuildChange} />
        <span>Automatically build graph on new projects</span>
      </label>
      <p className="text-[11px] text-muted-foreground/60 mt-0.5 max-w-xs text-center mx-auto">
        When on, Demido builds the graph before working in this folder (or after the first files
        land, for a brand-new project) and navigates the code with it.
      </p>
    </Centered>
  )
}

/** Placeholder the backend leaves before the graph's position-cache hook (see graphify.rs). */
const POS_MARKER = '<!--GRAPHIFY_POS-->'

function GraphView({ folder, onRebuild }: { folder: string; onRebuild: () => void }) {
  const { query, graphHtml, setPositions, loadPositions } = useGraphify()
  const [srcDoc, setSrcDoc] = useState<string | null>(null)
  const [htmlError, setHtmlError] = useState<string | null>(null)

  const [kind, setKind] = useState<QueryKind>('query')
  const [a, setA] = useState('')
  const [b, setB] = useState('')
  const [running, setRunning] = useState(false)
  const [result, setResult] = useState<string>('')

  // Build the iframe document once per open: cached HTML + a __GRAPHIFY_POS__ script when positions
  // are cached (so the hook skips stabilization). Positions are hydrated from disk first (they
  // survive an app restart), then read once here — not as a reactive dep — so a mid-session report
  // never remounts the settled graph.
  useEffect(() => {
    let alive = true
    setSrcDoc(null); setHtmlError(null)
    ;(async () => {
      try {
        const [positions, h] = await Promise.all([loadPositions(folder), graphHtml(folder)])
        if (!alive) return
        const inject = positions
          ? `<script>window.__GRAPHIFY_POS__=${JSON.stringify(positions)}</script>`
          : ''
        setSrcDoc(h.replace(POS_MARKER, inject))
      } catch (e) {
        if (alive) setHtmlError(String(e))
      }
    })()
    return () => { alive = false }
  }, [folder, graphHtml, loadPositions])

  // Capture the settled node positions the iframe reports, so the next open paints instantly.
  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      const d = e.data
      if (d && d.__graphify === 'positions' && d.positions) setPositions(folder, d.positions)
    }
    window.addEventListener('message', onMessage)
    return () => window.removeEventListener('message', onMessage)
  }, [folder, setPositions])

  const canRun = useMemo(() => {
    if (kind === 'path') return a.trim() !== '' && b.trim() !== ''
    return a.trim() !== ''
  }, [kind, a, b])

  async function run() {
    if (!canRun || running) return
    setRunning(true)
    setResult('')
    try {
      const args = kind === 'path' ? [a.trim(), b.trim()] : [a.trim()]
      setResult(await query(folder, kind, args))
    } catch (e) {
      setResult(String(e))
    } finally {
      setRunning(false)
    }
  }

  return (
    <div className="h-full flex flex-col">
      {/* Graph visualisation */}
      <div className="relative flex-1 min-h-0 border-b border-border">
        {srcDoc && (
          <iframe
            title="Knowledge graph"
            srcDoc={srcDoc}
            sandbox="allow-scripts"
            className="w-full h-full border-0 bg-background"
          />
        )}
        {!srcDoc && !htmlError && (
          <div className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
            <Loader2 className="w-5 h-5 animate-spin mr-2" /> Loading graph…
          </div>
        )}
        {htmlError && (
          <div className="absolute inset-0 flex items-center justify-center text-sm text-destructive px-6 text-center">{htmlError}</div>
        )}
        <Button
          size="sm" variant="secondary"
          className="absolute top-2 right-2 shadow-sm"
          onClick={onRebuild}
          title="Re-extract changed files into the graph"
        >
          <RefreshCw className="w-3.5 h-3.5 mr-1.5" /> Update
        </Button>
      </div>

      {/* Query panel */}
      <div className="shrink-0 p-3 flex flex-col gap-2" style={{ maxHeight: '45%' }}>
        <div className="flex items-center gap-1.5">
          {(Object.keys(KIND_LABEL) as QueryKind[]).map(k => (
            <button
              key={k}
              onClick={() => { setKind(k); setResult('') }}
              className={`px-2.5 py-1 rounded-md text-xs transition-colors ${
                kind === k ? 'bg-accent text-foreground' : 'text-muted-foreground hover:bg-accent/60'
              }`}
            >
              {KIND_LABEL[k]}
            </button>
          ))}
          <span className="text-[11px] text-muted-foreground ml-1">
            {kind === 'query' && 'Ask a question about the codebase'}
            {kind === 'path' && 'Shortest link between two concepts'}
            {kind === 'explain' && 'Plain-language explanation of a node'}
          </span>
        </div>

        <div className="flex items-center gap-2">
          <div className="relative flex-1">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground" />
            <input
              value={a}
              onChange={e => setA(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') run() }}
              placeholder={kind === 'path' ? 'From (concept A)' : kind === 'explain' ? 'Node name' : 'Your question'}
              className="w-full pl-8 pr-3 py-1.5 rounded-md bg-muted/40 border border-border text-sm outline-none focus:ring-1 focus:ring-ring"
            />
          </div>
          {kind === 'path' && (
            <input
              value={b}
              onChange={e => setB(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') run() }}
              placeholder="To (concept B)"
              className="flex-1 px-3 py-1.5 rounded-md bg-muted/40 border border-border text-sm outline-none focus:ring-1 focus:ring-ring"
            />
          )}
          <Button size="sm" onClick={run} disabled={!canRun || running}>
            {running ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Play className="w-3.5 h-3.5" />}
          </Button>
        </div>

        <div className="flex-1 min-h-0 overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-xs whitespace-pre-wrap">
          {result
            ? result
            : <span className="text-muted-foreground flex items-center gap-1.5"><FileText className="w-3.5 h-3.5" /> Results appear here.</span>}
        </div>
      </div>
    </div>
  )
}
