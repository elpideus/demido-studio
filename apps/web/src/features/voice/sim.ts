/* The voice conversation, simulated.

   No microphone and no speech model: "your voice" is a key held down, and what you "say" is the next line
   of a script. Everything else behaves the way the real voice mode must:

   1. Demido answers when your sentence is finished, not at the first silence. If you stop in the middle
      of a sentence to think, it waits, however long that takes.
   2. Speaking while Demido talks stops it at once.
   3. What Demido did not get to say is dropped, so it knows what you actually heard.

   Plain TypeScript with a tick(): the screen only draws what this says. */

export type Phase = 'listening' | 'waiting' | 'thinking' | 'working' | 'speaking'

export interface Line {
  id: number
  who: 'you' | 'demido' | 'event'
  text: string
  /** Still being said. */
  live?: boolean
  /** Demido was stopped before the end. Only what was said is kept. */
  cut?: boolean
}

export interface Snapshot {
  phase: Phase
  /** You are speaking right now. */
  talking: boolean
  /** You spoke, but the microphone is closed while Demido talks ("Stop when I speak" is off). */
  shut: boolean
  /** Something of this turn has been heard. */
  heard: boolean
  /** Everything you said in this turn. */
  said: string
  /** Whole seconds of silence since you stopped speaking. */
  silence: number
  /** What Demido is doing while it works. */
  task: string | null
  lines: Line[]
  /** The example is playing by itself. */
  auto: boolean
  /** The script has nothing left. */
  over: boolean
}

export interface Options {
  /** Speaking over Demido stops it. */
  bargeIn: boolean
  /** Answer when the sentence is finished. Off: answer after a fixed silence. */
  endOfSentence: boolean
  /** Say "one moment" before a task that takes time. */
  waitNotice: boolean
}

type Segment = { say: string; notice?: boolean } | { work: string; ms: number }
interface Exchange {
  /** " | " marks where the example stops to think, in the middle of the sentence. */
  you: string
  reply: Segment[]
  /** In the example this line is spoken over Demido. */
  interrupts?: boolean
}

const SCRIPT: Exchange[] = [
  {
    you: 'How much room is left on my graphics card?',
    reply: [{ say: '4.4 gigabytes are free. This model holds 6.1, the embedding model 0.6, and 0.9 is kept for the system.' }],
  },
  {
    you: 'Check whether the price of | the navy sweater went down.',
    reply: [
      { say: 'One moment. I am opening the shop page; that takes about twenty seconds.', notice: true },
      { work: 'Open the product page', ms: 2800 },
      { say: 'Still 89 euros. No change since Monday. I read the product page and the sale page. The sale page lists three other colours, but not navy. The daily check stays on at seven in the morning, and I will tell you as soon as the price moves.' },
    ],
  },
  {
    you: 'Stop. Only tell me when it drops.',
    interrupts: true,
    reply: [{ say: 'Understood. I stay quiet until the price drops.' }],
  },
]

const YOU_WORD_MS = 300
const DEMIDO_WORD_MS = 250
const PAUSE_AFTER: Record<string, number> = { ',': 140, ';': 220, ':': 200, '.': 380, '?': 380 }
/** Silence after a finished sentence before Demido answers. */
const ANSWER_AFTER_MS = 650
/** "Answer after a fixed silence": the old way, for comparison. */
export const FIXED_SILENCE_MS = 700
/** Silence after which an unfinished sentence is shown as "waiting". */
const UNFINISHED_AFTER_MS = 900
const THINK_MS = 800
/** How long the example stops to think in the middle of a sentence. */
const THINKING_PAUSE_MS = 3200

export class VoiceSim {
  /** How loud each side is right now, 0 to 1. The screen reads these every frame, never through React. */
  modelLevel = 0
  userLevel = 0

  private readonly opts: () => Options
  private readonly listeners = new Set<() => void>()
  private snap: Snapshot
  private phase: Phase = 'listening'
  private lines: Line[] = []
  private nextId = 1

  // where we are in the script
  private ex = 0
  private words: string[] = []
  private breaks: number[] = []
  private w = 0

  // your turn
  private talking = false
  private shut = false
  private heard = false
  private said = ''
  private silenceMs = 0
  private youClock = 0
  private youLine: Line | null = null

  // Demido's turn
  private segs: Segment[] = []
  private seg = -1
  private clock = 0
  private task: string | null = null
  private sayWords: string[] = []
  private sw = 0
  private sayDelay = 0
  private demidoLine: Line | null = null

  // loudness: every word is a push that dies away
  private mPush = 0
  private uPush = 0

  // the example that plays by itself
  private auto = false
  private autoClock = 0
  private autoTarget = 0

  constructor(opts: () => Options) {
    this.opts = opts
    this.loadLine()
    this.snap = this.make()
  }

  subscribe = (fn: () => void) => {
    this.listeners.add(fn)
    return () => void this.listeners.delete(fn)
  }
  getSnapshot = () => this.snap

  // ───────── what the screen may do ─────────

  /** You start speaking (the key goes down). Takes the example out of your hands' way. */
  hold() {
    if (this.auto) this.auto = false
    this.press()
  }
  /** You stop speaking (the key comes up). */
  release() {
    if (this.shut) {
      this.shut = false
      this.emit()
    }
    if (!this.talking) return
    this.talking = false
    this.silenceMs = 0
    this.emit()
  }
  /** "Answer now": do not wait for the end of the sentence. */
  answerNow() {
    if ((this.phase === 'listening' || this.phase === 'waiting') && this.heard) this.endTurn('asked')
  }
  /** Start from the top. With auto, the example speaks your lines for you. */
  restart(auto: boolean) {
    this.phase = 'listening'
    this.lines = []
    this.ex = 0
    this.loadLine()
    this.talking = this.shut = this.heard = false
    this.said = ''
    this.silenceMs = this.youClock = this.clock = 0
    this.youLine = this.demidoLine = null
    this.segs = []
    this.seg = -1
    this.task = null
    this.mPush = this.uPush = 0
    this.auto = auto
    this.autoClock = 0
    this.emit()
  }
  stopExample() {
    if (!this.auto) return
    this.auto = false
    this.release()
    this.emit()
  }

  /** Advance by dt milliseconds. Called once per frame. */
  tick(dt: number) {
    dt = Math.min(dt, 100)
    if (this.auto) this.drive(dt)
    const o = this.opts()

    // you
    if (this.talking) {
      this.youClock += dt
      while (this.youClock >= YOU_WORD_MS) {
        this.youClock -= YOU_WORD_MS
        if (this.w < this.words.length) this.hear(this.words[this.w++])
        else this.uPush = 0.45 + Math.random() * 0.4 // the scripted line is over, the voice line still moves
      }
    } else if (this.heard && (this.phase === 'listening' || this.phase === 'waiting')) {
      const second = Math.floor(this.silenceMs / 1000)
      this.silenceMs += dt
      const finished = this.w >= this.words.length
      if (!o.endOfSentence) {
        if (this.silenceMs >= FIXED_SILENCE_MS) this.endTurn(finished ? 'finished' : 'silence')
      } else if (finished) {
        if (this.silenceMs >= ANSWER_AFTER_MS) this.endTurn('finished')
      } else if (this.phase !== 'waiting' && this.silenceMs >= UNFINISHED_AFTER_MS) {
        // Silence, but the sentence is not finished: stay quiet and keep listening.
        this.phase = 'waiting'
        if (this.youLine) this.youLine.live = false
        this.youLine = null
        this.event('Silence. Your sentence sounds unfinished, so Demido waits.')
        this.emit()
      } else if (Math.floor(this.silenceMs / 1000) !== second) {
        this.emit()
      }
    }

    // Demido
    if (this.phase === 'thinking') {
      this.clock += dt
      if (this.clock >= THINK_MS) this.next()
    } else if (this.phase === 'working') {
      this.clock += dt
      const s = this.segs[this.seg]
      if (s && 'work' in s && this.clock >= s.ms) this.next()
    } else if (this.phase === 'speaking' && this.demidoLine) {
      this.clock += dt
      while (this.sw < this.sayWords.length && this.clock >= this.sayDelay) {
        this.clock -= this.sayDelay
        const word = this.sayWords[this.sw++]
        this.demidoLine.text += (this.demidoLine.text ? ' ' : '') + word
        this.sayDelay = DEMIDO_WORD_MS + Math.min(word.length, 9) * 12 + (PAUSE_AFTER[word[word.length - 1]] ?? 0)
        this.mPush = 0.7 + Math.random() * 0.5
        this.emit()
      }
      if (this.sw >= this.sayWords.length && this.clock >= this.sayDelay) {
        this.demidoLine.live = false
        this.next()
      }
    }

    // loudness chases the pushes: fast up, slower down
    this.mPush *= Math.exp(-dt / 170)
    this.uPush *= Math.exp(-dt / 150)
    const m = this.phase === 'speaking' ? this.mPush : 0
    const u = this.talking ? this.uPush : 0
    this.modelLevel = Math.min(1, this.modelLevel + (m - this.modelLevel) * (1 - Math.exp(-dt / (m > this.modelLevel ? 35 : 130))))
    this.userLevel = Math.min(1, this.userLevel + (u - this.userLevel) * (1 - Math.exp(-dt / (u > this.userLevel ? 30 : 110))))
  }

  // ───────── inside ─────────

  private isOver() {
    return this.ex >= SCRIPT.length && this.phase === 'listening' && !this.heard
  }

  private press() {
    if (this.talking) return
    const busy = this.phase === 'thinking' || this.phase === 'working' || this.phase === 'speaking'
    if (busy) {
      if (!this.opts().bargeIn) {
        this.shut = true
        this.emit()
        return
      }
      this.interrupt()
    }
    if (this.ex >= SCRIPT.length) {
      this.emit()
      return
    }
    this.talking = true
    this.shut = false
    this.phase = 'listening'
    this.silenceMs = 0
    this.youClock = YOU_WORD_MS * 0.6
    this.emit()
  }

  private interrupt() {
    if (this.phase === 'speaking' && this.demidoLine) {
      this.demidoLine.live = false
      this.demidoLine.cut = this.sw < this.sayWords.length
      this.event('You spoke over Demido. It stopped at once. What it had not said yet is dropped.')
    } else if (this.phase === 'working') {
      this.event(`You spoke. Demido stopped; "${this.task}" was cancelled.`)
    } else {
      this.event('You went on speaking. Demido held its answer.')
    }
    this.segs = []
    this.seg = -1
    this.task = null
    this.demidoLine = null
    this.mPush = 0
    this.phase = 'listening'
  }

  private hear(word: string) {
    if (!this.heard) {
      this.heard = true
      this.said = ''
      this.youLine = null
    }
    if (!this.youLine) this.youLine = this.add('you', '')
    this.youLine.text += (this.youLine.text ? ' ' : '') + word
    this.youLine.live = true
    this.said += (this.said ? ' ' : '') + word
    this.uPush = 0.7 + Math.random() * 0.5
    this.emit()
  }

  private endTurn(why: 'finished' | 'silence' | 'asked') {
    const finished = this.w >= this.words.length
    if (this.youLine) this.youLine.live = false
    this.youLine = null
    this.heard = this.talking = false
    this.silenceMs = 0
    if (finished) {
      const notice = this.opts().waitNotice
      this.segs = SCRIPT[this.ex].reply.filter((s) => notice || !('notice' in s && s.notice))
      this.ex++
      this.loadLine()
    } else {
      // The rest of the sentence stays yours to say in the next turn.
      this.event(
        why === 'asked'
          ? 'You asked for an answer before the sentence was finished.'
          : `${(FIXED_SILENCE_MS / 1000).toFixed(1)} s of silence. Demido answers, though the sentence was not finished.`,
      )
      this.segs = [{ say: `I only caught "${this.said}". What comes after that?` }]
    }
    this.seg = -1
    this.clock = 0
    this.phase = 'thinking'
    this.emit()
  }

  private next() {
    this.seg++
    this.task = null
    this.clock = 0
    const s = this.segs[this.seg]
    if (!s) {
      this.phase = 'listening'
      this.demidoLine = null
      this.segs = []
      this.seg = -1
      this.shut = false
    } else if ('say' in s) {
      this.phase = 'speaking'
      this.sayWords = s.say.split(' ')
      this.sw = 0
      this.sayDelay = 160
      this.demidoLine = this.add('demido', '')
      this.demidoLine.live = true
    } else {
      this.phase = 'working'
      this.task = s.work
    }
    this.emit()
  }

  /** The example: speaks your lines, stops to think in the middle of one, and talks over Demido once. */
  private drive(dt: number) {
    if (this.isOver()) {
      this.auto = false
      this.emit()
      return
    }
    if (this.talking) {
      if (this.w >= this.autoTarget && this.youClock >= YOU_WORD_MS * 0.7) this.release()
      return
    }
    const line = SCRIPT[this.ex]
    if (!line) return
    if (this.phase === 'listening' || this.phase === 'waiting') {
      if (this.heard && this.w >= this.words.length) return // finished: Demido's move
      this.autoClock += dt
      if (this.autoClock >= (this.w > 0 ? THINKING_PAUSE_MS : 1200)) this.autoSpeak()
    } else {
      this.autoClock = 0
      const lastPart = this.seg === this.segs.length - 1
      if (line.interrupts && this.opts().bargeIn && this.phase === 'speaking' && lastPart && this.sw >= 14) this.autoSpeak()
    }
  }
  private autoSpeak() {
    this.autoClock = 0
    this.autoTarget = this.breaks.find((b) => b > this.w) ?? this.words.length
    this.press()
  }

  private loadLine() {
    this.words = []
    this.breaks = []
    this.w = 0
    const line = SCRIPT[this.ex]
    if (!line) return
    for (const part of line.you.split(' | ')) {
      this.words.push(...part.split(' '))
      this.breaks.push(this.words.length)
    }
  }

  private add(who: Line['who'], text: string): Line {
    const line: Line = { id: this.nextId++, who, text }
    this.lines.push(line)
    return line
  }
  private event(text: string) {
    this.add('event', text)
  }

  private make(): Snapshot {
    return {
      phase: this.phase,
      talking: this.talking,
      shut: this.shut,
      heard: this.heard,
      said: this.said,
      silence: Math.floor(this.silenceMs / 1000),
      task: this.task,
      lines: this.lines.map((l) => ({ ...l })),
      auto: this.auto,
      over: this.isOver(),
    }
  }
  private emit() {
    this.snap = this.make()
    this.listeners.forEach((fn) => fn())
  }
}
