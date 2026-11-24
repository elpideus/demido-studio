/* WhatsApp, Telegram and Discord, for real (DECISIONS G12 to G16; research note 32).

   What is built is the services' own allowed way, a bot: the person makes one, gives Demido its token, and pairs
   it by sending it a code; from then on what they write to it from the chat app goes into a session of its own,
   and the answer comes back there. Nobody else is answered. Every message the bot got and sent is shown here.
   Linking the person's own account (their chats, marked as seen, replies drafted) needs their phone, and for
   WhatsApp and Discord breaks the service's terms: not built, and said so. */
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { Bot, ExternalLink, KeyRound, Trash2 } from 'lucide-react'
import { coreGet, coreSend, onCoreEvent } from '@/data/core'
import { toast } from '@/data/store'
import { Button, Dialog, Glyph, Icon, NotBuilt, Page, Section, Signal } from '@/ui'
import type { Service } from './data'

interface KeyFacts { ends_with: string; kept_in: 'keyring' | 'file' }
interface CoreBot { id: string; service: string; name: string; code: string | null; owner: string | null; owner_name: string | null; session_id: string | null; token: KeyFacts | null; running: boolean; problem: string | null; created_at: number }
interface Said { bot: string; chat: string; way: 'in' | 'out'; from: string; text: string; state: 'answered' | 'ignored' | 'paired' | 'sent'; at: number }

const MADE_AT: Record<string, { where: string; link: string; how: string }> = {
  telegram: { where: '@BotFather', link: 'https://t.me/BotFather', how: 'In Telegram, write to @BotFather, send /newbot, and give it a name. It answers with the bot\'s token: paste that here.' },
  discord: { where: 'the Developer Portal', link: 'https://discord.com/developers/applications', how: 'In the Discord Developer Portal, make an application, open Bot, and copy its token (Reset Token shows it once). Then write to the bot in a direct message: Demido reads only direct messages, which needs no special permission.' },
}

function AddBot({ svc, onClose }: { svc: Service; onClose: () => void }) {
  const [token, setToken] = useState('')
  const [busy, setBusy] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  const made = MADE_AT[svc.id]
  const add = async () => {
    setBusy(true); setProblem(null)
    try {
      const bot = await coreSend<CoreBot>('POST', '/bots', { service: svc.id, token: token.trim() })
      toast(`${bot.name} is connected. Pair it by sending it the code ${bot.code}.`, undefined, 'task-done')
      onClose()
    } catch (e) {
      setProblem((e as Error).message)
    }
    setBusy(false)
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={`Add a ${svc.name} bot`} text={made.how}
      actions={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={busy || !token.trim()} onClick={() => void add()}>{busy ? 'Asking ' + svc.name + '…' : 'Check and add'}</Button></>}>
      <div className="app-stack" style={{ marginBlockStart: 'var(--d-space-16)' }}>
        <label className="d-field"><span className="d-field__label">Bot token</span><input className="d-input d-input--mono" type="password" autoComplete="off" autoFocus value={token} onChange={(e) => setToken(e.target.value)} />
          <span className="d-field__help">Made at <a href={made.link} target="_blank" rel="noreferrer noopener">{made.where}</a>. Kept in the vault, never shown again. It is the bot's, not your account's.</span></label>
        {problem && <Signal level="caution">{problem}</Signal>}
      </div>
    </Dialog>
  )
}

function BotCard({ bot, svc }: { bot: CoreBot; svc: Service }) {
  const navigate = useNavigate()
  const [said, setSaid] = useState<Said[]>([])
  const [removing, setRemoving] = useState(false)
  const read = () => coreGet<Said[]>(`/bots/${encodeURIComponent(bot.id)}/messages`).then(setSaid, () => {})
  useEffect(() => {
    void read()
    const off = onCoreEvent('bot.message', (e) => { if (e.subject === bot.id) void read() })
    return off
  }, [bot.id])
  const remove = async () => {
    setRemoving(false)
    await coreSend('DELETE', `/bots/${encodeURIComponent(bot.id)}`).then(() => toast(`${bot.name} is no longer in Demido. Its session stays.`, undefined, 'task-done'), (e: Error) => toast(`Not removed. ${e.message}`, undefined, 'task-failed'))
  }
  return (
    <article className="d-card">
      <header className="d-card__head">
        <Glyph name={bot.running ? (bot.owner ? 'task-done' : 'task-needs-input') : 'task-failed'} size="1.25rem" style={{ marginTop: '0.125rem' }} label={bot.running ? (bot.owner ? 'Paired and running' : 'Waiting to be paired') : 'Not running'} />
        <div className="app-stack" style={{ gap: 'var(--d-space-2)', flex: 1 }}>
          <h3 className="d-card__title">{bot.name}</h3>
          <span className="d-card__sub">{bot.running ? 'connected' : `not connected${bot.problem ? `: ${bot.problem}` : ''}`}{bot.owner ? ` · answers only ${bot.owner_name ?? bot.owner}` : ''}{bot.token ? ` · token …${bot.token.ends_with}` : ''}</span>
        </div>
        {bot.session_id && <Button size="sm" variant="ghost" onClick={() => navigate(`/sessions/${encodeURIComponent(bot.session_id!)}`)}><Icon icon={ExternalLink} size="sm" />Its session</Button>}
        <Button size="sm" variant="ghost" onClick={() => setRemoving(true)}><Icon icon={Trash2} size="sm" />Remove</Button>
      </header>
      <div className="d-card__body app-stack">
        {!bot.owner && bot.code && (
          <Signal level="note">To pair it, send it {svc.id === 'telegram' ? <>the message <span className="d-mono">/start {bot.code}</span></> : <>a direct message with <span className="d-mono">{bot.code}</span></>} from your own {svc.name} account. That account alone is answered from then on; anyone else who writes to it is told it answers only its owner, and nothing they write reaches a model.</Signal>
        )}
        {said.length > 0 ? (
          <ul className="d-list">
            {said.slice(-30).map((s, i) => (
              <li key={i} className="d-row d-row--compact">
                <span className="d-row__text">
                  <span className="d-row__title">{s.way === 'in' ? s.from : bot.name}<span className="d-type-caption d-text-tertiary"> · {new Date(s.at).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })}{s.state === 'ignored' ? ' · not answered: not the owner' : s.state === 'paired' ? ' · paired' : ''}</span></span>
                  <span className="d-row__sub" style={{ whiteSpace: 'pre-wrap' }}>{s.text}</span>
                </span>
              </li>
            ))}
          </ul>
        ) : <p className="d-type-caption d-text-secondary">Nothing has come to it yet.</p>}
      </div>
      {removing && <Dialog open onOpenChange={(o) => !o && setRemoving(false)} title={`Remove ${bot.name}?`} text={`Demido stops answering it and forgets its token and its messages. The bot itself stays at ${svc.name} until you delete it there; its session in Demido stays.`} actions={<><Button onClick={() => setRemoving(false)}>Cancel</Button><Button variant="danger" onClick={() => void remove()}>Remove</Button></>} />}
    </article>
  )
}

export function RealMessenger({ svc }: { svc: Service }) {
  const [bots, setBots] = useState<CoreBot[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const read = () => coreGet<CoreBot[]>('/bots').then((b) => { setBots(b.filter((x) => x.service === svc.id)); setError(null) }, (e: Error) => setError(e.message))
  useEffect(() => {
    void read()
    const offs = ['bot.added', 'bot.removed', 'bot.paired', 'bot.stopped'].map((k) => onCoreEvent(k, () => void read()))
    return () => offs.forEach((off) => off())
  }, [svc.id])
  return (
    <Page title={svc.name} lead={svc.bot ? `Talk to Demido from ${svc.name}, through a bot of your own: ${svc.name}'s own, allowed way for a program. A bot is its own account: it cannot read your chats.` : `WhatsApp has no bots for people, and no allowed way for a program to use a personal account.`}>
      {error && <Signal level="caution">The core did not say. {error}</Signal>}
      {svc.bot && (
        <Section title="Bots" hint="What the owner writes goes into a session of its own, with a note of where it came from; the answer comes back as plain text, a message a second. A use of a tool that needs your yes is asked there in words: reply yes or no.">
          <div className="app-stack">
            {bots?.map((b) => <BotCard key={b.id} bot={b} svc={svc} />)}
            {bots && bots.length === 0 && <p className="app-empty"><Icon icon={Bot} /> No {svc.name} bot yet.</p>}
            <div><Button variant="primary" onClick={() => setAdding(true)}><Icon icon={KeyRound} />Add a {svc.name} bot</Button></div>
          </div>
        </Section>
      )}
      <Section title="Your own account" hint="Your chats in Demido, marked as seen, replies drafted for you.">
        <Signal level={svc.risk}>{svc.rule}</Signal>
        <NotBuilt block>Linking your own {svc.name} account. It needs your phone{svc.id === 'telegram' ? ', and the project its own api_id from Telegram (which needs a phone number), and Telegram\'s terms forbid using what is read there for AI in words that are not plain about a person\'s own model: yours to weigh' : svc.id === 'discord' ? ', and Discord forbids it outright ("self-bots")' : ', and WhatsApp forbids unofficial clients'}. Research note 32.</NotBuilt>
      </Section>
      {adding && <AddBot svc={svc} onClose={() => { setAdding(false); void read() }} />}
    </Page>
  )
}
