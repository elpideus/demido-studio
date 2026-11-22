// Voice mode in headless Chromium, with a WAV file as the microphone: signs in with a one-use link, opens
// voice mode, waits, opens the transcript, and prints what the page shows and what it complained of.
// Usage: node core/dev/voice-window-try.mjs <profile dir> <sign-in url> <microphone.wav> [seconds]
import { spawn } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'

const [profile, signIn, mic, secondsArg] = process.argv.slice(2)
const origin = new URL(signIn).origin
const chrome = spawn('chromium', ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', `--user-data-dir=${profile}`, '--remote-debugging-port=0',
  '--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', `--use-file-for-fake-audio-capture=${mic}%noloop`, '--autoplay-policy=no-user-gesture-required', 'about:blank'], { stdio: 'ignore' })
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
let port = null
for (let i = 0; i < 100 && !port; i++) { await sleep(100); const f = `${profile}/DevToolsActivePort`; if (existsSync(f)) port = readFileSync(f, 'utf8').split('\n')[0] }
const page = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find((t) => t.type === 'page')
const ws = new WebSocket(page.webSocketDebuggerUrl)
await new Promise((r) => ws.addEventListener('open', r))
let id = 0
const pending = new Map()
const logs = []
const send = (method, params = {}) => new Promise((res) => { const m = { id: ++id, method, params }; pending.set(m.id, res); ws.send(JSON.stringify(m)) })
ws.addEventListener('message', (ev) => {
  const m = JSON.parse(ev.data)
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m.result ?? m.error); pending.delete(m.id); return }
  if (m.method === 'Runtime.exceptionThrown') logs.push(`exception: ${m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text}`)
  if (m.method === 'Log.entryAdded' && m.params.entry.level !== 'verbose') logs.push(`${m.params.entry.level}: ${m.params.entry.text.slice(0, 200)}`)
  if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') logs.push(`console: ${m.params.args.map((a) => a.value ?? a.description).join(' ')}`)
})
const js = async (expression) => (await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }))?.result?.value
await send('Runtime.enable')
await send('Log.enable')
await send('Page.enable')
await send('Browser.grantPermissions', { origin, permissions: ['audioCapture'] })
await send('Page.navigate', { url: signIn })
await sleep(2500)
await js(`location.hash = '#/voice/talk'; 1`)
const seconds = Number(secondsArg || 40)
const seen = []
for (let t = 0; t < seconds; t += 2) {
  await sleep(2000)
  const state = await js(`document.querySelector('.talk__state')?.textContent`)
  if (seen.at(-1) !== state) { seen.push(state); console.log(`${String(t + 2).padStart(3)} s  ${state}`) }
}
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.includes('Transcript'))?.click(); 1`)
await sleep(500)
console.log('--- transcript ---')
console.log(await js(`document.querySelector('.talk__log')?.innerText`))
console.log('--- words, problem ---')
console.log(await js(`[document.querySelector('.talk__words')?.innerText, document.querySelector('.talk__you .d-signal')?.innerText]`))
console.log('--- page said ---')
console.log(logs.slice(0, 20).join('\n') || '(nothing)')
ws.close()
chrome.kill()
