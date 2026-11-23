/* Voice mode, for real (DECISIONS V7): its files, and one spoken conversation over the core's WebSocket.
   The conversation is a small store the voice screen reads, as it reads the preview's simulation: a phase, what
   was said, and how loud each side is (read straight from here every frame, never through React). */
import { coreGet, coreSend } from './core'

export interface VoiceStatus {
  ready: boolean; missing_bytes: number; whisper: boolean; whisper_bytes: number
  voices: { id: string; present: boolean }[]
  downloading: { done: number; total: number; file: string } | null
  problem: string | null; folder: string
}
export const voiceStatus = () => coreGet<VoiceStatus>('/voice')
export const downloadVoice = (whisper = false) => coreSend<VoiceStatus>('POST', '/voice/download', { whisper })

export type Phase = 'starting' | 'listening' | 'hearing' | 'waiting' | 'thinking' | 'speaking' | 'closed'
export interface Line { id: string; who: 'you' | 'demido' | 'event'; text: string; live?: boolean; cut?: boolean; message?: string }
export interface Snapshot { phase: Phase; lines: Line[]; problem: string | null; model: string | null; session: string | null; muted: boolean }

const PLAY_RATE = 24000

/** One spoken conversation: the microphone in, Demido's voice out. */
export class Conversation {
  private socket: WebSocket | null = null
  private capture: AudioContext | null = null
  private stream: MediaStream | null = null
  private play: AudioContext | null = null
  private analyser: AnalyserNode | null = null
  private sources: { node: AudioBufferSourceNode; sentence: number; start: number; seconds: number }[] = []
  private nextTime = 0
  private playingNow = false
  private snap: Snapshot = { phase: 'starting', lines: [], problem: null, model: null, session: null, muted: false }
  private listeners = new Set<() => void>()
  private levels = new Float32Array(256)
  private n = 0
  userLevel = 0
  modelLevel = 0

  /** `model`: for a new conversation; left out, the core takes one that is loaded, or else the first. */
  constructor(private opts: { voice: string; bargeIn: boolean; session?: string; model?: string }) {}

  subscribe = (l: () => void) => { this.listeners.add(l); return () => { this.listeners.delete(l) } }
  getSnapshot = () => this.snap
  private set(change: Partial<Snapshot>) { this.snap = { ...this.snap, ...change }; this.listeners.forEach((l) => l()) }
  private line(who: Line['who'], text: string, extra: Partial<Line> = {}) {
    this.set({ lines: [...this.snap.lines, { id: `l${++this.n}`, who, text, ...extra }] })
  }
  private lastDemido(change: Partial<Line>) {
    const lines = [...this.snap.lines]
    for (let i = lines.length - 1; i >= 0; i--) if (lines[i].who === 'demido') { lines[i] = { ...lines[i], ...change }; break }
    this.set({ lines })
  }

  async start() {
    try {
      this.stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, channelCount: 1 } })
    } catch (e) {
      this.set({ phase: 'closed', problem: `The microphone could not be opened: ${(e as Error).message}` })
      return
    }
    this.play = new AudioContext({ sampleRate: PLAY_RATE })
    this.analyser = this.play.createAnalyser()
    this.analyser.fftSize = 256
    this.analyser.connect(this.play.destination)
    this.capture = new AudioContext()
    await this.capture.audioWorklet.addModule('/voice-capture.js')
    const mic = this.capture.createMediaStreamSource(this.stream)
    const worklet = new AudioWorkletNode(this.capture, 'demido-capture')
    mic.connect(worklet)
    const params = new URLSearchParams({ voice: this.opts.voice, barge_in: String(this.opts.bargeIn) })
    if (this.opts.session) params.set('session', this.opts.session)
    if (this.opts.model) params.set('model', this.opts.model)
    const socket = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/v1/voice/talk?${params}`)
    socket.binaryType = 'arraybuffer'
    this.socket = socket
    worklet.port.onmessage = (e: MessageEvent<{ pcm: ArrayBuffer; level: number }>) => {
      this.userLevel = this.userLevel * 0.6 + e.data.level * 0.4
      // With "Stop when I speak" off, the microphone is closed while Demido speaks (V3).
      if (this.snap.muted || (!this.opts.bargeIn && this.playingNow)) return
      if (socket.readyState === WebSocket.OPEN) socket.send(e.data.pcm)
    }
    socket.onmessage = (m) => (typeof m.data === 'string' ? this.event(JSON.parse(m.data)) : this.sound(m.data as ArrayBuffer))
    socket.onclose = () => { if (this.snap.phase !== 'closed') this.set({ phase: 'closed' }) }
  }

  private send(what: object) { if (this.socket?.readyState === WebSocket.OPEN) this.socket.send(JSON.stringify(what)) }

  private event(e: { type: string; [k: string]: unknown }) {
    switch (e.type) {
      case 'ready': this.set({ phase: 'listening', model: String(e.model), session: String(e.session) }); break
      case 'speech': if (!this.playingNow) this.set({ phase: 'hearing' }); break
      case 'waiting': this.set({ phase: 'waiting' }); break
      case 'finished': this.set({ phase: 'thinking' }); break
      case 'heard': this.line('you', String(e.text)); this.set({ phase: 'thinking' }); break
      case 'heard_nothing': this.line('event', 'Demido did not catch that.'); this.set({ phase: 'listening' }); break
      case 'say': {
        const last = [...this.snap.lines].reverse().find((l) => l.who !== 'event')
        if (last?.who === 'demido' && last.live && last.message === e.message) this.lastDemido({ text: `${last.text} ${String(e.text)}` })
        else this.line('demido', String(e.text), { live: true, message: String(e.message) })
        break
      }
      case 'answered': this.lastDemido({ live: false }); break
      case 'stop_playback': this.stopPlayback(e.message as string | undefined); break
      case 'cut': this.cut(String(e.message), String(e.heard)); break
      case 'error': this.set({ problem: String(e.message) }); break
    }
  }

  /** A piece of Demido's voice: played after what is already queued. */
  private sound(data: ArrayBuffer) {
    if (!this.play || !this.analyser || data.byteLength <= 4) return
    const sentence = new DataView(data).getUint32(0, true)
    const pcm = new Int16Array(data, 4)
    const buffer = this.play.createBuffer(1, pcm.length, PLAY_RATE)
    const ch = buffer.getChannelData(0)
    for (let i = 0; i < pcm.length; i++) ch[i] = pcm[i] / 32768
    const node = this.play.createBufferSource()
    node.buffer = buffer
    node.connect(this.analyser)
    const start = Math.max(this.play.currentTime + 0.03, this.nextTime)
    node.start(start)
    this.nextTime = start + buffer.duration
    const entry = { node, sentence, start, seconds: buffer.duration }
    this.sources.push(entry)
    node.onended = () => {
      this.sources = this.sources.filter((s) => s !== entry)
      if (this.sources.length === 0) this.playing(false)
    }
    this.playing(true)
  }

  private playing(on: boolean) {
    if (on === this.playingNow) return
    this.playingNow = on
    this.send({ type: 'playing', on })
    this.set({ phase: on ? 'speaking' : this.snap.phase === 'speaking' ? 'listening' : this.snap.phase })
  }

  /** An answer cut where the person spoke over it: it shows what they heard, and no more. */
  private cut(message: string, heard: string) {
    const at = this.snap.lines.findLastIndex((l) => l.who === 'demido' && l.message === message)
    const change = { text: heard || '…', live: false, cut: true }
    if (at < 0) {
      if (heard) this.line('demido', heard, { ...change, message })
      return
    }
    this.set({ lines: this.snap.lines.map((l, i) => (i === at ? { ...l, ...change } : l)) })
  }

  /** Stops Demido's voice at once and tells the core how far it got into the answer `message`: the sentence
      playing, and how much of it. */
  private stopPlayback(message?: string) {
    const now = this.play?.currentTime ?? 0
    const playingNow = this.sources.find((s) => s.start <= now && now < s.start + s.seconds) ?? this.sources[0]
    let ms = 0
    let sentence = playingNow?.sentence ?? 0
    if (playingNow) {
      // The pieces of the same sentence before this one were heard whole.
      const before = this.sources.filter((s) => s.sentence === sentence && s.start < playingNow.start).reduce((t, s) => t + s.seconds, 0)
      ms = Math.round((before + Math.max(0, now - playingNow.start)) * 1000)
    } else {
      sentence = 0
    }
    for (const s of this.sources) { try { s.node.onended = null; s.node.stop() } catch { /* already ended */ } }
    this.sources = []
    this.nextTime = 0
    this.playing(false)
    this.send({ type: 'played', sentence, ms, message })
    this.set({ phase: 'hearing' })
  }

  answerNow() { this.send({ type: 'answer_now' }) }
  stopSpeaking() { this.send({ type: 'stop' }) }
  setMuted(muted: boolean) { this.set({ muted }) }

  /** How loud Demido is now, 0 to 1. */
  tick() {
    if (this.analyser) {
      this.analyser.getFloatTimeDomainData(this.levels)
      let p = 0
      for (const v of this.levels) p += v * v
      this.modelLevel = this.modelLevel * 0.5 + Math.min(1, Math.sqrt(p / this.levels.length) * 5) * 0.5
    }
    this.userLevel *= 0.92
  }

  close() {
    this.set({ phase: 'closed' })
    this.socket?.close()
    this.stream?.getTracks().forEach((t) => t.stop())
    void this.capture?.close()
    void this.play?.close()
  }
}

const socketTo = (path: string, params: URLSearchParams) => new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/v1/voice/${path}?${params}`)

/** Reads one answer aloud with voice mode's voice (DECISIONS V11). One at a time in the whole window: starting
    another stops this one. `onState` hears `starting`, `speaking`, `done` and, with the reason, `failed`. */
export class ReadAloud {
  private static current: ReadAloud | null = null
  private socket: WebSocket | null = null
  private play: AudioContext | null = null
  private nodes: AudioBufferSourceNode[] = []
  private nextTime = 0
  private finished = false

  constructor(private message: string, private voice: string, private onState: (state: 'starting' | 'speaking' | 'done' | 'failed', why?: string) => void) {}

  start() {
    ReadAloud.current?.stop()
    ReadAloud.current = this
    this.onState('starting')
    this.play = new AudioContext({ sampleRate: PLAY_RATE })
    const socket = socketTo('read', new URLSearchParams({ message: this.message, voice: this.voice }))
    socket.binaryType = 'arraybuffer'
    this.socket = socket
    socket.onmessage = (m) => {
      if (typeof m.data !== 'string') return this.sound(m.data as ArrayBuffer)
      const e = JSON.parse(m.data) as { type: string; message?: string }
      if (e.type === 'error') { this.onState('failed', String(e.message)); this.end() }
      if (e.type === 'done') { this.finished = true; this.endWhenPlayed() }
    }
    socket.onclose = () => { this.finished = true; this.endWhenPlayed() }
  }

  private sound(data: ArrayBuffer) {
    if (!this.play || data.byteLength <= 4) return
    const pcm = new Int16Array(data, 4)
    const buffer = this.play.createBuffer(1, pcm.length, PLAY_RATE)
    const ch = buffer.getChannelData(0)
    for (let i = 0; i < pcm.length; i++) ch[i] = pcm[i] / 32768
    const node = this.play.createBufferSource()
    node.buffer = buffer
    node.connect(this.play.destination)
    const start = Math.max(this.play.currentTime + 0.03, this.nextTime)
    node.start(start)
    this.nextTime = start + buffer.duration
    this.nodes.push(node)
    node.onended = () => { this.nodes = this.nodes.filter((n) => n !== node); this.endWhenPlayed() }
    this.onState('speaking')
  }

  private endWhenPlayed() {
    if (this.finished && this.nodes.length === 0 && this.play) { this.onState('done'); this.end() }
  }

  private end() {
    this.socket?.close()
    this.socket = null
    void this.play?.close()
    this.play = null
    if (ReadAloud.current === this) ReadAloud.current = null
  }

  /** Stops at once. */
  stop() {
    if (this.socket?.readyState === WebSocket.OPEN) this.socket.send(JSON.stringify({ type: 'stop' }))
    for (const n of this.nodes) { try { n.onended = null; n.stop() } catch { /* ended */ } }
    this.nodes = []
    this.onState('done')
    this.end()
  }
}

/** Dictation into the message box (DECISIONS V11): the microphone, heard by voice mode's ears; each finished
    sentence is given to `onHeard`, to be put into the box. Nothing is sent by it. */
export class Dictation {
  private socket: WebSocket | null = null
  private capture: AudioContext | null = null
  private stream: MediaStream | null = null

  constructor(private onHeard: (text: string) => void, private onState: (state: 'starting' | 'listening' | 'hearing' | 'waiting' | 'closed', why?: string) => void) {}

  async start() {
    this.onState('starting')
    try {
      this.stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, channelCount: 1 } })
    } catch (e) {
      this.onState('closed', `The microphone could not be opened: ${(e as Error).message}`)
      return
    }
    this.capture = new AudioContext()
    await this.capture.audioWorklet.addModule('/voice-capture.js')
    const mic = this.capture.createMediaStreamSource(this.stream)
    const worklet = new AudioWorkletNode(this.capture, 'demido-capture')
    mic.connect(worklet)
    const socket = socketTo('dictate', new URLSearchParams())
    socket.binaryType = 'arraybuffer'
    this.socket = socket
    worklet.port.onmessage = (e: MessageEvent<{ pcm: ArrayBuffer }>) => { if (socket.readyState === WebSocket.OPEN) socket.send(e.data.pcm) }
    socket.onmessage = (m) => {
      if (typeof m.data !== 'string') return
      const e = JSON.parse(m.data) as { type: string; text?: string; message?: string }
      if (e.type === 'ready') this.onState('listening')
      if (e.type === 'speech') this.onState('hearing')
      if (e.type === 'waiting') this.onState('waiting')
      if (e.type === 'heard' && e.text) { this.onHeard(String(e.text)); this.onState('listening') }
      if (e.type === 'heard_nothing') this.onState('listening')
      if (e.type === 'error') { this.onState('closed', String(e.message)); this.stop() }
    }
    socket.onclose = () => this.onState('closed')
  }

  /** The sentence being spoken is over: write it down now. */
  done() { if (this.socket?.readyState === WebSocket.OPEN) this.socket.send(JSON.stringify({ type: 'answer_now' })) }

  stop() {
    this.socket?.close()
    this.socket = null
    this.stream?.getTracks().forEach((t) => t.stop())
    void this.capture?.close()
    this.capture = null
  }
}
