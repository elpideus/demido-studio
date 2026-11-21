/* Voice mode: its own screen, without the navigation.

   Two things move, and they never share a meaning:
     the Demido symbol in the middle is Demido. It swells with every word Demido speaks; its two eyes most.
     the line at the bottom is you. It rises with your voice.
   Everything else on the screen holds still, so one glance tells you who has the floor. */
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { useNavigate } from 'react-router'
import { ArrowLeft, CornerDownLeft, Mic, MicOff, Play, RotateCcw, ScrollText, SlidersHorizontal, Square } from 'lucide-react'
import { setSetting, setting, useSetting } from '@/data/store'
import { Button, Glyph, Icon, Menu, NotBuilt, Tip } from '@/ui'
import { FIXED_SILENCE_MS, VoiceSim, type Snapshot } from './sim'

const VOICE_NAME: Record<string, string> = { ava: 'Ava', theo: 'Theo', mila: 'Mila' }

/* The symbol, from brand/logo/demido-symbol.svg (256 grid). Drawn here as parts, because the parts move. */
const STEMS = [
  'M40.05 49.01C75.24 36.95 111.89 49.45 123.17 78.01C131.93 100.18 122.92 125.74 102.56 143.2',
  'M92.64 148.87C101.5 143.33 111.89 140.14 123.01 140.14C155.43 140.14 181.72 167.29 181.72 200.77C181.72 215.36 176.73 228.75 168.42 239.21',
]
const EYES = [
  'M94.77 86.99C88.93 77.75 74.1 76.64 61.64 84.52C49.18 92.4 43.82 106.27 49.66 115.51C55.5 124.75 70.34 125.85 82.79 117.97C95.25 110.1 100.61 96.22 94.77 86.99',
  'M150.55 178.79C144.71 169.56 129.88 168.45 117.42 176.33C104.97 184.2 99.6 198.08 105.45 207.32C111.29 216.55 126.12 217.66 138.58 209.78C151.03 201.91 156.4 188.03 150.55 178.79',
]
/* The rings that leave the symbol while Demido speaks. Each follows the loudness a little later than the
   one before, so a word travels outward. */
const HALOS = [
  { base: 1.09, spread: 0.1, alpha: 0.5, lag: 90 },
  { base: 1.22, spread: 0.2, alpha: 0.3, lag: 180 },
  { base: 1.36, spread: 0.28, alpha: 0.15, lag: 300 },
]
/* Your voice line: tallest in the middle, every bar with its own small wobble. */
const BARS = Array.from({ length: 44 }, (_, i) => ({
  shape: 0.32 + 0.68 * Math.exp(-(((i - 21.5) / 10.5) ** 2)),
  period: 90 + ((i * 37) % 70),
  phase: i * 1.7,
}))

function stateOf(s: Snapshot): string {
  if (s.phase === 'speaking') return s.shut ? 'Demido is speaking · the microphone is closed' : 'Demido is speaking'
  if (s.phase === 'thinking') return 'Demido is thinking'
  if (s.phase === 'working') return `Demido is working · ${s.task ?? ''}`
  if (s.phase === 'waiting') return `Demido waits for you to finish · ${s.silence} s of silence`
  return 'Demido is listening'
}
/** The last two sentences: a caption, not a page. */
const tail = (text: string) => text.split(/(?<=[.?!]) /).slice(-2).join(' ')

export function VoiceMode() {
  const navigate = useNavigate()
  const sim = useMemo(
    () =>
      new VoiceSim(() => ({
        bargeIn: setting('voice.bargeIn', true),
        endOfSentence: setting('voice.endOfSentence', true),
        waitNotice: setting('voice.waitNotice', true),
      })),
    [],
  )
  const s = useSyncExternalStore(sim.subscribe, sim.getSnapshot)
  const bargeIn = useSetting('voice.bargeIn', true)
  const endOfSentence = useSetting('voice.endOfSentence', true)
  const waitNotice = useSetting('voice.waitNotice', true)
  const voice = useSetting<string>('voice.voice', 'none')
  const [log, setLog] = useState(false)
  const mark = useRef<SVGGElement>(null)
  const eyes = useRef<(SVGGElement | null)[]>([])
  const halos = useRef<(SVGGElement | null)[]>([])
  const line = useRef<HTMLDivElement>(null)
  const logBox = useRef<HTMLElement>(null)
  // Back to where you came from; to the Voice page when voice mode was opened directly.
  const leave = () => ((window.history.state as { idx?: number } | null)?.idx ? navigate(-1) : navigate('/voice'))
  const leaveRef = useRef(leave)
  leaveRef.current = leave

  // One loop: advance the conversation, then draw the loudness straight onto the elements.
  // Loudness never goes through React; React only hears about words and states.
  useEffect(() => {
    const calm = window.matchMedia('(prefers-reduced-motion: reduce)')
    const late = HALOS.map(() => 0)
    let slowUser = 0
    let last = performance.now()
    let raf = 0
    const frame = (now: number) => {
      const dt = Math.min(now - last, 100)
      last = now
      sim.tick(dt)
      const still = calm.matches
      const m = sim.modelLevel
      const u = sim.userLevel
      HALOS.forEach((h, i) => {
        late[i] += (m - late[i]) * (1 - Math.exp(-dt / h.lag))
        const el = halos.current[i]
        if (!el) return
        // Reduced motion: the rings keep their place and only fade.
        el.style.transform = `scale(${still ? h.base : h.base + late[i] * h.spread})`
        el.style.opacity = String(late[i] * h.alpha)
      })
      // The whole symbol swells a little; the eyes swell a lot. Each element gets its own transform.
      if (mark.current) mark.current.style.transform = still ? '' : `scale(${1 + m * 0.05})`
      for (const el of eyes.current) {
        if (!el) continue
        el.style.transform = still ? '' : `scale(${1 + m * 0.28})`
        el.style.opacity = still ? String(0.6 + late[2] * 0.4) : ''
      }
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
  }, [sim])

  // Space stands in for your voice. Enter: answer now. Escape: leave.
  useEffect(() => {
    const elsewhere = (t: EventTarget | null) =>
      (t instanceof HTMLElement && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName))) || document.querySelector('[role="menu"]') !== null
    const down = (e: KeyboardEvent) => {
      if (elsewhere(e.target) || e.metaKey || e.ctrlKey || e.altKey) return
      if (e.code === 'Space') {
        e.preventDefault()
        if (!e.repeat) sim.hold()
      } else if (e.key === 'Enter' && !(e.target instanceof HTMLElement && e.target.closest('button, a'))) {
        e.preventDefault()
        sim.answerNow()
      } else if (e.key === 'Escape') {
        leaveRef.current()
      }
    }
    const up = (e: KeyboardEvent) => {
      if (e.code !== 'Space' || elsewhere(e.target)) return
      e.preventDefault()
      sim.release()
    }
    const blur = () => sim.release()
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    window.addEventListener('blur', blur)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
      window.removeEventListener('blur', blur)
    }
  }, [sim])

  // Keep the newest line of the transcript in view.
  useEffect(() => {
    if (logBox.current) logBox.current.scrollTop = logBox.current.scrollHeight
  }, [s.lines, log])

  // Demido's caption shows what it is saying, or what it said last. Once you have answered, it clears;
  // an answer you cut off stays up while you speak, so you can see where it stopped.
  const said = s.lines.filter((l) => l.who !== 'event')
  const last = said[said.length - 1]
  const lastDemido = [...said].reverse().find((l) => l.who === 'demido')
  const spoken = lastDemido && (last === lastDemido || (lastDemido.cut && (s.heard || s.talking))) ? lastDemido : undefined
  const canAnswer = s.heard && (s.phase === 'listening' || s.phase === 'waiting')

  return (
    <div className="talk" data-phase={s.phase} data-talking={s.talking ? '' : undefined} data-log={log ? '' : undefined}>
      <header className="talk__bar">
        <Tip label="Leave voice mode · Esc" side="bottom">
          <Button variant="ghost" iconOnly aria-label="Leave voice mode" onClick={leave}><Icon icon={ArrowLeft} /></Button>
        </Tip>
        <span className="d-type-headline">Voice mode</span>
        <span className="talk__who"><Glyph name="model-loaded" tone="live" label="Loaded" />Qwen3 4B Instruct · voice {VOICE_NAME[voice] ?? 'Ava'}</span>
        <span className="app-spacer" />
        <NotBuilt>Scripted: no microphone, no sound</NotBuilt>
        <Menu
          align="end"
          trigger={<Button variant="ghost" size="sm"><Icon icon={SlidersHorizontal} size="sm" />Behaviour</Button>}
          items={[
            { kind: 'label', label: 'In voice mode' },
            { label: 'Wait until my sentence is finished', checked: endOfSentence, onSelect: () => setSetting('voice.endOfSentence', !endOfSentence) },
            { label: 'Stop when I speak', checked: bargeIn, onSelect: () => setSetting('voice.bargeIn', !bargeIn) },
            { label: 'Say "one moment" before long tasks', checked: waitNotice, onSelect: () => setSetting('voice.waitNotice', !waitNotice) },
          ]}
        />
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
        <p className="talk__state" role="status">
          {s.phase === 'working' && <Glyph name="task-running" />}
          {stateOf(s)}
        </p>
        <p className="talk__words" data-live={spoken?.live ? '' : undefined}>
          {spoken && (
            <>
              {tail(spoken.text)}
              {spoken.cut && <> <span className="d-badge d-badge--outline">Stopped here</span></>}
            </>
          )}
        </p>
      </section>

      <section className="talk__you" aria-label="You">
        <p className="talk__heard" data-live={s.heard ? '' : undefined}>
          {s.shut ? (
            <><Icon icon={MicOff} size="sm" /> The microphone is closed while Demido speaks. Turn on "Stop when I speak" to talk over it.</>
          ) : s.said ? (
            <>{s.said}{s.heard && '…'}</>
          ) : (
            'Hold Space and speak. Demido answers when your sentence is finished.'
          )}
        </p>
        <div className="talk__line" ref={line} aria-hidden="true">
          {BARS.map((_, i) => <span key={i} />)}
        </div>
        <div className="talk__controls">
          <Button
            size="lg"
            className="talk__hold"
            pressed={s.talking}
            onPointerDown={(e) => { e.currentTarget.setPointerCapture(e.pointerId); sim.hold() }}
            onPointerUp={() => sim.release()}
            onPointerCancel={() => sim.release()}
          >
            <Icon icon={Mic} />Hold to speak <kbd className="d-kbd">Space</kbd>
          </Button>
          <Tip label="Do not wait for the end of the sentence">
            <Button size="lg" variant="ghost" disabled={!canAnswer} onClick={() => sim.answerNow()}><Icon icon={CornerDownLeft} />Answer now <kbd className="d-kbd">Enter</kbd></Button>
          </Tip>
          {s.auto ? (
            <Button size="lg" variant="ghost" onClick={() => sim.stopExample()}><Icon icon={Square} size="sm" />Stop the example</Button>
          ) : (
            <Button size="lg" variant="ghost" onClick={() => sim.restart(true)}><Icon icon={Play} size="sm" />Play the example</Button>
          )}
          {s.lines.length > 0 && !s.auto && (
            <Button size="lg" variant="ghost" onClick={() => sim.restart(false)}><Icon icon={RotateCcw} size="sm" />Start again</Button>
          )}
        </div>
        <p className="talk__note">
          {s.over
            ? 'That was the whole example. Start again, or change a behaviour at the top and compare.'
            : endOfSentence
              ? 'In the preview, holding Space is your voice: each hold says the next words of a script. Let go in the middle of a sentence and Demido waits.'
              : `"Wait until my sentence is finished" is off: Demido answers after ${(FIXED_SILENCE_MS / 1000).toFixed(1)} s of silence, finished or not.`}
        </p>
      </section>

      {log && (
        <aside className="talk__log" aria-label="Transcript" ref={logBox}>
          <p className="app-insp__title">Transcript <span>{s.lines.filter((l) => l.who !== 'event').length} turns</span></p>
          {s.lines.length === 0 && <p className="d-type-caption d-text-tertiary">Nothing has been said yet. Notes about pauses and interruptions appear here too.</p>}
          <ol>
            {s.lines.map((l) =>
              l.who === 'event' ? (
                <li key={l.id} className="talk__event">{l.text}</li>
              ) : (
                <li key={l.id} data-who={l.who}>
                  <span className="d-type-micro d-text-tertiary">{l.who === 'you' ? 'You' : 'Demido'}</span>
                  <p>{l.text}{l.live && '…'}{l.cut && <> <span className="d-badge d-badge--outline">Stopped here</span></>}</p>
                </li>
              ),
            )}
          </ol>
        </aside>
      )}
    </div>
  )
}
