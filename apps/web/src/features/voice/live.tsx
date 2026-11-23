/* Voice mode, for real (DECISIONS V1 to V7): the same screen as the preview's, driven by a conversation with
   the core instead of a script. The microphone is open while the screen is; Demido answers when your sentence
   is finished, and stops when you speak over it. */
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { useNavigate } from 'react-router'
import { ArrowLeft, CornerDownLeft, Mic, MicOff, ScrollText, Square } from 'lucide-react'
import { useSetting } from '@/data/store'
import { Conversation, type Snapshot } from '@/data/voice'
import { Button, Glyph, Icon, Signal, Tip } from '@/ui'
import { BARS, EYES, HALOS, STEMS } from './talk'

function stateOf(s: Snapshot): string {
  switch (s.phase) {
    case 'starting': return 'Starting: opening the microphone and loading the voice'
    case 'hearing': return 'Demido is listening to you'
    case 'waiting': return 'Demido waits for you to finish'
    case 'thinking': return 'Demido is thinking'
    case 'speaking': return 'Demido is speaking'
    case 'closed': return 'Voice mode has ended'
    default: return s.muted ? 'The microphone is off' : 'Demido is listening'
  }
}
const tail = (text: string) => text.split(/(?<=[.?!]) /).slice(-2).join(' ')

export function LiveVoiceMode() {
  const navigate = useNavigate()
  const voice = useSetting<string>('voice.real', '2181')
  const bargeIn = useSetting('voice.bargeIn', true)
  const model = useSetting<string>('voice.model', '')
  const talk = useMemo(() => new Conversation({ voice, bargeIn, model: model || undefined }), [voice, bargeIn, model])
  const s = useSyncExternalStore(talk.subscribe, talk.getSnapshot)
  const [log, setLog] = useState(false)
  const mark = useRef<SVGGElement>(null)
  const eyes = useRef<(SVGGElement | null)[]>([])
  const halos = useRef<(SVGGElement | null)[]>([])
  const line = useRef<HTMLDivElement>(null)
  const logBox = useRef<HTMLElement>(null)
  const leave = () => ((window.history.state as { idx?: number } | null)?.idx ? navigate(-1) : navigate('/voice'))

  useEffect(() => {
    void talk.start()
    return () => talk.close()
  }, [talk])

  // The loudness of each side, drawn straight onto the elements every frame, as in the preview.
  useEffect(() => {
    const calm = window.matchMedia('(prefers-reduced-motion: reduce)')
    const late = HALOS.map(() => 0)
    let slowUser = 0
    let last = performance.now()
    let raf = 0
    const frame = (now: number) => {
      const dt = Math.min(now - last, 100)
      last = now
      talk.tick()
      const still = calm.matches
      const m = talk.modelLevel
      const u = talk.userLevel
      HALOS.forEach((h, i) => {
        late[i] += (m - late[i]) * (1 - Math.exp(-dt / h.lag))
        const el = halos.current[i]
        if (!el) return
        el.style.transform = `scale(${still ? h.base : h.base + late[i] * h.spread})`
        el.style.opacity = String(late[i] * h.alpha)
      })
      if (mark.current) mark.current.style.transform = still ? '' : `scale(${1 + m * 0.05})`
      for (const el of eyes.current) if (el) el.style.transform = still ? '' : `scale(${1 + m * 0.28})`
      slowUser += (u - slowUser) * (1 - Math.exp(-dt / 400))
      const bars = line.current?.children
      if (bars) {
        for (let i = 0; i < bars.length; i++) {
          const b = BARS[i]
          const y = still ? slowUser * b.shape * 0.8 : u * b.shape * (0.55 + 0.45 * Math.sin(now / b.period + b.phase))
          ;(bars[i] as HTMLElement).style.transform = `scaleY(${0.07 + y * 0.93})`
        }
      }
      raf = requestAnimationFrame(frame)
    }
    raf = requestAnimationFrame(frame)
    return () => cancelAnimationFrame(raf)
  }, [talk])

  // Enter: answer now. Escape: leave.
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.key === 'Escape') leave()
      else if (e.key === 'Enter' && !(e.target instanceof HTMLElement && e.target.closest('button, a, input, textarea'))) { e.preventDefault(); talk.answerNow() }
    }
    window.addEventListener('keydown', down)
    return () => window.removeEventListener('keydown', down)
  })
  useEffect(() => { if (logBox.current) logBox.current.scrollTop = logBox.current.scrollHeight }, [s.lines, log])

  const said = s.lines.filter((l) => l.who !== 'event')
  const lastDemido = [...said].reverse().find((l) => l.who === 'demido')
  const lastYou = [...said].reverse().find((l) => l.who === 'you')
  return (
    <div className="talk" data-phase={s.phase === 'hearing' ? 'listening' : s.phase} data-talking={s.phase === 'hearing' ? '' : undefined} data-log={log ? '' : undefined}>
      <header className="talk__bar">
        <Tip label="Leave voice mode · Esc" side="bottom">
          <Button variant="ghost" iconOnly aria-label="Leave voice mode" onClick={leave}><Icon icon={ArrowLeft} /></Button>
        </Tip>
        <span className="d-type-headline">Voice mode</span>
        {s.model && <span className="talk__who"><Glyph name="model-loaded" tone="live" label="Model" />{s.model} · voice {voice}</span>}
        <span className="app-spacer" />
        <Button variant="ghost" size="sm" pressed={log} onClick={() => setLog(!log)}><Icon icon={ScrollText} size="sm" />Transcript</Button>
      </header>

      <section className="talk__stage" aria-label="Demido">
        <svg className="talk__mark" viewBox="0 0 256 256" role="img" aria-label={stateOf(s)}>
          {HALOS.map((_, i) => (
            <g key={i} ref={(el) => { halos.current[i] = el }} style={{ opacity: 0 }}>
              <circle className="talk__halo" cx="128" cy="128" r="118.28" />
            </g>
          ))}
          <g ref={mark} fill="none" stroke="currentColor" strokeWidth="17.37">
            <circle cx="128" cy="128" r="118.28" strokeWidth="15.44" />
            {STEMS.map((d) => <path key={d} d={d} />)}
            <path d="M74.19 160.54L103.87 141.77" strokeLinecap="round" />
            {EYES.map((d, i) => (
              <g key={d} className="talk__eye" ref={(el) => { eyes.current[i] = el }}>
                <path className="talk__lid" d={d} fill="currentColor" stroke="none" />
              </g>
            ))}
          </g>
        </svg>
        <p className="talk__state" role="status">{(s.phase === 'thinking' || s.phase === 'starting') && <Glyph name="task-running" busy />}{stateOf(s)}</p>
        <p className="talk__words" data-live={lastDemido?.live ? '' : undefined}>
          {lastDemido && <>{tail(lastDemido.text)}{lastDemido.cut && <> <span className="d-badge d-badge--outline">Stopped here</span></>}</>}
        </p>
      </section>

      <section className="talk__you" aria-label="You">
        {s.problem && <Signal level="caution">{s.problem}</Signal>}
        <p className="talk__heard" data-live={s.phase === 'hearing' ? '' : undefined}>
          {s.muted ? <><Icon icon={MicOff} size="sm" /> The microphone is off.</> : lastYou ? lastYou.text : 'Speak. Demido answers when your sentence is finished; if you stop to think, it waits.'}
        </p>
        <div className="talk__line" ref={line} aria-hidden="true">{BARS.map((_, i) => <span key={i} />)}</div>
        <div className="talk__controls">
          <Button size="lg" pressed={!s.muted} onClick={() => talk.setMuted(!s.muted)}><Icon icon={s.muted ? MicOff : Mic} />{s.muted ? 'Microphone off' : 'Microphone on'}</Button>
          <Tip label="Do not wait for the end of the sentence">
            <Button size="lg" variant="ghost" disabled={s.phase !== 'waiting' && s.phase !== 'hearing'} onClick={() => talk.answerNow()}><Icon icon={CornerDownLeft} />Answer now <kbd className="d-kbd">Enter</kbd></Button>
          </Tip>
          <Button size="lg" variant="ghost" disabled={s.phase !== 'speaking' && s.phase !== 'thinking'} onClick={() => talk.stopSpeaking()}><Icon icon={Square} size="sm" />Stop</Button>
        </div>
        <p className="talk__note">Speech is turned into text on this computer, and Demido's voice is made here too: nothing you say is sent away. {bargeIn ? 'Speak over Demido and it stops; what you did not hear is taken out of the conversation.' : '"Stop when I speak" is off: the microphone is closed while Demido speaks.'}</p>
      </section>

      {log && (
        <aside className="talk__log" aria-label="Transcript" ref={logBox}>
          <p className="app-insp__title">Transcript <span>{said.length} turns</span></p>
          {s.lines.length === 0 && <p className="d-type-caption d-text-tertiary">Nothing has been said yet.</p>}
          <ol>
            {s.lines.map((l) => l.who === 'event'
              ? <li key={l.id} className="talk__event">{l.text}</li>
              : <li key={l.id} data-who={l.who}><span className="d-type-micro d-text-tertiary">{l.who === 'you' ? 'You' : 'Demido'}</span><p>{l.text}{l.live && '…'}{l.cut && <> <span className="d-badge d-badge--outline">Stopped here</span></>}</p></li>)}
          </ol>
        </aside>
      )}
    </div>
  )
}
