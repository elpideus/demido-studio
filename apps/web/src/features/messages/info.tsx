/* A chat's details, opened by pressing its name at the top, the way the chat apps do it:
   who it is, what was shared in it (media, links, files), who is in it, and the few switches a chat has.
   Everything shown here is made up for the preview; pictures are drawn, not real. */
import { useMemo, useState } from 'react'
import { FileText, Link2, Play } from 'lucide-react'
import { toast } from '@/data/store'
import { Button, Icon, NotBuilt, Segmented, Switch } from '@/ui'
import { Avatar } from './chat'
import { profile, shared } from './data'
import type { Chat, Service } from './data'

export function Info({ svc, chat }: { svc: Service; chat: Chat }) {
  const s = useMemo(() => shared(chat), [chat])
  const p = useMemo(() => profile(chat, svc.id), [chat, svc.id])
  const [tab, setTab] = useState<'media' | 'links' | 'docs'>('media')
  const [muted, setMuted] = useState(false)
  const whatsapp = svc.id === 'whatsapp'
  const files = whatsapp ? 'Docs' : 'Files'
  const open = (what: string) => toast(`Opens ${what}. Not built in the preview.`)
  return (
    <div className="msgs__info">
      <div className="msgs__info-head">
        <Avatar name={chat.name} group={chat.group} size="xl" />
        <strong className="d-type-title-3">{chat.name}</strong>
        <span className="d-mono d-text-secondary">{chat.group ? p.status : p.handle}</span>
        {!chat.group && <span className="d-type-caption d-text-tertiary">{p.status}</span>}
      </div>

      <section>
        <p className="app-insp__title">{chat.group ? 'Description' : svc.id === 'telegram' ? 'Bio' : 'About'}</p>
        <p>{p.about}</p>
      </section>

      <section>
        <p className="app-insp__title">{whatsapp ? 'Media, links and docs' : 'Shared in this chat'} <span>{s.media.length + s.links.length + s.docs.length}</span></p>
        <Segmented small fill label="What was shared" value={tab} onChange={setTab} options={[{ value: 'media', label: `Media ${s.media.length}` }, { value: 'links', label: `Links ${s.links.length}` }, { value: 'docs', label: `${files} ${s.docs.length}` }]} />
        {tab === 'media' && (s.media.length ? (
          <div className="msgs__media">
            {s.media.map((m, i) => (
              <span key={m.id} style={{ display: 'contents' }}>
                {(i === 0 || s.media[i - 1].when !== m.when) && <p className="msgs__when">{m.when.charAt(0).toUpperCase() + m.when.slice(1)}</p>}
                <button type="button" className="msgs__thumb" aria-label={`${m.video ? 'Video' : 'Picture'}, ${m.when}`} style={{ background: `linear-gradient(135deg, oklch(0.6 0.09 ${m.hue}), oklch(0.36 0.07 ${(m.hue + 50) % 360}))` }} onClick={() => open(m.video ? 'the video' : 'the picture')}>
                  {m.video && <Icon icon={Play} size="sm" />}
                </button>
              </span>
            ))}
          </div>
        ) : <p className="d-type-caption d-text-secondary">No pictures or videos in this chat.</p>)}
        {tab === 'links' && (s.links.length ? (
          <ul className="d-list">
            {s.links.map((l) => (
              <li key={l.title}><button type="button" className="d-row d-row--compact" onClick={() => open(`${l.host} in the built-in browser`)}>
                <span className="d-row__lead"><Icon icon={Link2} /></span>
                <span className="d-row__text"><span className="d-row__title">{l.title}</span><span className="d-row__sub">{l.host} · {l.when} · from {l.by}</span></span>
              </button></li>
            ))}
          </ul>
        ) : <p className="d-type-caption d-text-secondary">No links in this chat.</p>)}
        {tab === 'docs' && (s.docs.length ? (
          <ul className="d-list">
            {s.docs.map((d) => (
              <li key={d.name}><button type="button" className="d-row d-row--compact" onClick={() => open(d.name)}>
                <span className="d-row__lead"><Icon icon={FileText} /></span>
                <span className="d-row__text"><span className="d-row__title">{d.name}</span><span className="d-row__sub">{d.size} · {d.when} · from {d.by}</span></span>
              </button></li>
            ))}
          </ul>
        ) : <p className="d-type-caption d-text-secondary">No files in this chat.</p>)}
        <span className="d-untrusted">Shared by other people · treated as data</span>
      </section>

      {chat.group && (
        <section>
          <p className="app-insp__title">{chat.name.startsWith('#') ? 'Wrote here' : 'Members'} <span>{p.members.length}</span></p>
          <ul className="d-list">
            {p.members.map((m, i) => (
              <li key={m} className="msgs__member"><Avatar name={m} /><span>{m}</span>{i === 0 && m !== 'you' && !chat.name.startsWith('#') && <span className="d-badge d-badge--outline">admin</span>}</li>
            ))}
          </ul>
        </section>
      )}

      <section>
        <label className="d-check"><Switch label="Mute notifications" checked={muted} onChange={setMuted} /><span className="d-check__text">Mute notifications<small>{muted ? 'Demido still reads it; it just stays quiet.' : 'New messages are shown as they arrive.'}</small></span></label>
        <div className="app-row">
          <Button size="sm" variant="ghost" onClick={() => open('the starred messages')}>Starred messages · {p.starred}</Button>
          <Button size="sm" variant="ghost" onClick={() => open('a search inside this chat')}>Search in this chat</Button>
        </div>
        <div className="app-row">
          {chat.group ? <Button size="sm" variant="ghost" onClick={() => open('the confirmation to leave')}>Leave</Button> : <Button size="sm" variant="ghost" onClick={() => open('the confirmation to block')}>Block</Button>}
          <Button size="sm" variant="ghost" onClick={() => open(`the report to ${svc.name}`)}>Report</Button>
        </div>
        <NotBuilt>Drawn for the preview: no real pictures or files</NotBuilt>
      </section>
    </div>
  )
}
