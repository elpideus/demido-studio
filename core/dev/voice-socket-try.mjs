// Speaking over Demido, against the core's voice WebSocket, as the window does it: asks for something long,
// waits for Demido's voice, says it is playing, speaks over it, and says how far it had got when told to
// stop. Prints what happens and what was kept of the answer.
// Usage: node core/dev/voice-socket-try.mjs <host:port> <key> <ask.wav> <over.wav>  (16 kHz mono 16-bit WAVs)
// TRIAL_MODEL=<id> picks the model of the new session.
import { readFileSync } from 'node:fs'

const [host, key, askWav, overWav] = process.argv.slice(2)
const pcm = (file) => readFileSync(file).subarray(44)
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const started = Date.now()
const at = () => `${((Date.now() - started) / 1000).toFixed(1).padStart(5)} s`
const model = process.env.TRIAL_MODEL ? `&model=${encodeURIComponent(process.env.TRIAL_MODEL)}` : ''
const ws = new WebSocket(`ws://${host}/api/v1/voice/talk?voice=1410${model}`, { headers: { Authorization: `Bearer ${key}` } })
ws.binaryType = 'arraybuffer'
const seen = []
let sound = 0
let firstSentence = null
ws.onmessage = (m) => {
  if (typeof m.data !== 'string') {
    if (sound === 0) console.log(at(), 'first sound')
    sound += (m.data.byteLength - 4) / 2
    if (firstSentence === null) firstSentence = new DataView(m.data).getUint32(0, true)
    return
  }
  const e = JSON.parse(m.data)
  seen.push(e)
  console.log(at(), JSON.stringify(e).slice(0, 160))
}
await new Promise((r) => { ws.onopen = r })
const feed = async (bytes) => { for (let i = 0; i < bytes.length; i += 640) { ws.send(bytes.subarray(i, i + 640)); await sleep(20) } }
const quiet = (seconds) => Buffer.alloc(Math.round(16000 * seconds) * 2)
const until = async (test, seconds) => { for (let i = 0; i < seconds * 10; i++) { if (seen.some(test)) return true; await sleep(100) } return false }
await until((e) => e.type === 'ready', 20)
await feed(Buffer.concat([quiet(0.5), pcm(askWav), quiet(1)]))
// Synthetic speech can sound unfinished to Smart Turn (research note 28): then, as a person would, Answer now.
if (await until((e) => e.type === 'waiting', 3) && !seen.some((e) => e.type === 'heard')) {
  console.log(at(), 'answer now')
  ws.send(JSON.stringify({ type: 'answer_now' }))
}
await until((e) => e.type === 'say', 60)
// Demido's voice arrives; the window plays it and says so.
for (let i = 0; i < 100 && sound < 24000; i++) await sleep(100)
ws.send(JSON.stringify({ type: 'playing', on: true }))
console.log(at(), `playing: ${(sound / 24000).toFixed(2)} s of voice so far`)
await sleep(1500)
await feed(Buffer.concat([pcm(overWav), quiet(0.6)]))
const stopped = await until((e) => e.type === 'stop_playback', 10)
console.log(at(), stopped ? 'told to stop playing' : 'NOT told to stop playing')
// The window heard 1.5 s of the first sentence.
ws.send(JSON.stringify({ type: 'played', message: seen.find((e) => e.type === 'stop_playback')?.message, sentence: firstSentence ?? 1, ms: 1500 }))
await until((e) => e.type === 'cut', 10)
await until((e) => e.type === 'heard' && seen.filter((x) => x.type === 'heard').length >= 2, 15)
await sleep(1000)
ws.close()
const cut = seen.find((e) => e.type === 'cut')
console.log('--- kept of the answer ---')
console.log(cut ? JSON.stringify(cut.heard) : '(no cut)')
