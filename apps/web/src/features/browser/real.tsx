/* The Browser, for real: Demido's own browser, which a model drives (DECISIONS Z).

   It is Demido's own copy of Chrome for Testing's headless shell, downloaded from Google's storage when the
   person asks. Its profile holds nothing of theirs, each session has its own cookies, and everything it loads
   goes through Demido, which lets it reach only the public internet. What a model saw is shown under each step
   of its session; here are the pages open now, with their last picture, and what the browser was let through. */
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { AppWindow, Download, ExternalLink, RefreshCw, Trash2, X } from 'lucide-react'
import { coreGet, coreSend, onCoreEvent } from '@/data/core'
import { toast } from '@/data/store'
import { Button, Dialog, Glyph, Icon, NotBuilt, Page, Section, Signal } from '@/ui'

interface Kept { version: string; path: string; size: number; checked: string; from: string; kept_at: number }
interface Downloading { version: string; done: number; total: number | null }
interface OpenPage { session: string; address: string; title: string; picture: string | null; idle_ms: number }
interface Status { offered: boolean; kept: Kept | null; downloading: Downloading | null; running: boolean; pages: OpenPage[]; through: number; refused: number }
interface Latest { version: string; url: string; size: number | null }

const mb = (n: number) => `${Math.round(n / 1_000_000)} MB`

export function RealBrowser() {
  const navigate = useNavigate()
  const [status, setStatus] = useState<Status | null>(null)
  const [latest, setLatest] = useState<Latest | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [progress, setProgress] = useState<Downloading | null>(null)
  const [removing, setRemoving] = useState(false)
  const read = () => coreGet<Status>('/browser').then((s) => { setStatus(s); setError(null) }, (e: Error) => setError(e.message))
  useEffect(() => {
    void read()
    const offs = [
      onCoreEvent('browser.download', (e) => setProgress(e.data as unknown as Downloading)),
      ...['browser.downloaded', 'browser.download.failed', 'browser.removed', 'browser.opened', 'browser.pressed', 'browser.typed', 'browser.started', 'browser.stopped'].map((k) => onCoreEvent(k, () => { setProgress(null); void read() })),
    ]
    const every = setInterval(() => void read(), 15_000)
    return () => { offs.forEach((off) => off()); clearInterval(every) }
  }, [])
  useEffect(() => {
    if (status && status.offered && !status.kept) coreGet<Latest>('/browser/latest').then(setLatest, () => setLatest(null))
  }, [status?.offered, status?.kept])

  const download = async () => {
    try {
      const kept = await coreSend<Kept>('POST', '/browser')
      toast(`Demido's browser is downloaded: version ${kept.version}, checked by ${kept.checked}.`, undefined, 'task-done')
    } catch (e) {
      toast(`Not downloaded. ${(e as Error).message}`, undefined, 'task-failed')
    }
    setProgress(null)
    void read()
  }
  const close = async (session: string) => {
    await coreSend('DELETE', `/sessions/${encodeURIComponent(session)}/browser`).catch(() => {})
    void read()
  }
  const remove = async () => {
    setRemoving(false)
    try {
      await coreSend('DELETE', '/browser')
      toast('Demido\'s browser is deleted, with its profile.', undefined, 'task-done')
    } catch (e) {
      toast(`Not deleted. ${(e as Error).message}`, undefined, 'task-failed')
    }
    void read()
  }

  const going = progress ?? status?.downloading ?? null
  return (
    <Page title="Browser" lead="The browser a model drives: it opens a page, runs its scripts, and can press its links and buttons and type into its boxes. What it saw is shown under each step in the session that used it.">
      {error && <Signal level="caution">The core did not say. {error}</Signal>}
      {!status && !error && <p className="app-empty"><Glyph name="task-running" busy /> Reading the browser…</p>}
      {status && !status.offered && <Signal level="note">Demido's browser is built for Linux and macOS. On this system it is not offered yet.</Signal>}
      {status && status.offered && (
        <Section title="Demido's browser" hint="Its own copy of Chrome for Testing's headless shell, made by Google, downloaded from Google's storage and used under Google's terms. Nothing is installed on your system, and none of your own browsers is touched.">
          <article className="d-card">
            <header className="d-card__head">
              <Glyph name={status.kept ? 'task-done' : going ? 'task-running' : 'task-pending'} busy={!!going} size="1.25rem" style={{ marginTop: '0.125rem' }} label={status.kept ? 'Downloaded' : 'Not downloaded'} />
              <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
                <h3 className="d-card__title">Chrome for Testing, headless shell{status.kept ? ` ${status.kept.version}` : latest ? ` ${latest.version}` : ''}</h3>
                <span className="d-card__sub">
                  {status.kept
                    ? `${status.running ? 'running' : 'not running: it starts when a model opens a page'} · ${status.through} let through, ${status.refused} refused since it started`
                    : going
                      ? `downloading ${mb(going.done)}${going.total ? ` of ${mb(going.total)}` : ''}`
                      : `not downloaded${latest?.size ? ` · ${mb(latest.size)}` : ''}`}
                </span>
              </div>
              {!status.kept && !going && <Button size="sm" onClick={() => void download()}><Icon icon={Download} size="sm" />Download{latest?.size ? ` (${mb(latest.size)})` : ''}</Button>}
              {going && <Button size="sm" onClick={() => void coreSend('DELETE', '/browser/download')}>Stop</Button>}
              {status.kept && <Button size="sm" variant="ghost" onClick={() => setRemoving(true)}><Icon icon={Trash2} size="sm" />Delete</Button>}
            </header>
            <div className="d-card__body app-stack">
              {going && going.total && <div className="d-meter" role="meter" aria-valuenow={Math.round((going.done / going.total) * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="Downloaded"><span style={{ '--d-meter': `${(going.done / going.total) * 100}%` } as React.CSSProperties} /></div>}
              <p className="d-text-secondary">Its profile is empty: none of your sign-ins, passwords or history, and each session's cookies are its own, gone when its page is closed. Everything it loads goes through Demido, which lets it reach only the public internet, never this computer or your network; it says it is Demido. WebRTC is shut in it. Opening an address asks as reading a page does, pressing asks only at Cautious, typing the model's own words asks at Balanced, and it never types into a password box.</p>
              {status.kept && <p className="d-type-caption d-text-secondary">Checked by {status.kept.checked}. Kept in <span className="d-mono">{status.kept.path}</span>.</p>}
              {!status.kept && <p className="d-type-caption d-text-secondary">Chrome for Testing publishes no checksum: Demido checks the download against its size and the MD5 Google's storage gives with it, which proves it arrived whole; that it is Google's rests on the connection to Google's storage.</p>}
            </div>
          </article>
        </Section>
      )}
      {status && status.kept && (
        <Section title="Pages open now" hint="One for each session that is using the browser. A page left alone for ten minutes is closed, with its cookies, and the browser with the last one.">
          <div><Button size="sm" variant="ghost" onClick={() => void read()}><Icon icon={RefreshCw} size="sm" />Read again</Button></div>
          {status.pages.length === 0
            ? <p className="app-empty"><Icon icon={AppWindow} /> No page is open. A model opens one with open_page, in a session.</p>
            : (
              <div className="app-stack">
                {status.pages.map((p) => (
                  <article className="d-card" key={p.session}>
                    <header className="d-card__head">
                      <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1, minWidth: 0 }}>
                        <h3 className="d-card__title">{p.title || p.address}</h3>
                        <span className="d-card__sub d-mono" style={{ overflowWrap: 'anywhere' }}>{p.address}</span>
                      </div>
                      <Button size="sm" variant="ghost" onClick={() => navigate(`/sessions/${encodeURIComponent(p.session)}`)}><Icon icon={ExternalLink} size="sm" />Its session</Button>
                      <Button size="sm" variant="ghost" onClick={() => void close(p.session)}><Icon icon={X} size="sm" />Close</Button>
                    </header>
                    {p.picture && <div className="d-card__body"><img className="browser__shot" alt={`The page ${p.title || p.address}, as the model last saw it`} src={`/api/v1/sessions/${encodeURIComponent(p.session)}/browser/pictures/${encodeURIComponent(p.picture)}`} /></div>}
                  </article>
                ))}
              </div>
            )}
        </Section>
      )}
      <NotBuilt block>You driving the browser from this window, several tabs, a model's own scripts in a page, the network and console for a model, pictures for a model that reads them, the ad blocker and marking up a page (shown in the preview), and Windows.</NotBuilt>
      {removing && <Dialog open onOpenChange={(o) => !o && setRemoving(false)} title="Delete Demido's browser?" text="Its pages are closed and its profile goes with it. It can be downloaded again." actions={<><Button onClick={() => setRemoving(false)}>Cancel</Button><Button variant="danger" onClick={() => void remove()}>Delete</Button></>} />}
    </Page>
  )
}
