// Opens a page in headless Chromium over its debugging protocol, collects console lines, errors and policy
// reports from the page and every frame in it, waits, then prints them and the page's #out text.
// Usage: node cdp-probe.mjs <profile-dir> <url> [wait-ms]
import { spawn } from 'node:child_process'
import { readFileSync, existsSync } from 'node:fs'

const [profile, url, waitArg] = process.argv.slice(2)
const wait = Number(waitArg || 6000)
const chrome = spawn('chromium', ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
  `--user-data-dir=${profile}`, '--remote-debugging-port=0', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] })
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
let port = null
for (let i = 0; i < 100 && !port; i++) {
  await sleep(100)
  const f = `${profile}/DevToolsActivePort`
  if (existsSync(f)) port = readFileSync(f, 'utf8').split('\n')[0]
}
const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
const page = list.find((t) => t.type === 'page')
const ws = new WebSocket(page.webSocketDebuggerUrl)
await new Promise((r) => ws.addEventListener('open', r))
let id = 0
const pending = new Map()
const lines = []
const send = (method, params = {}, sessionId) => new Promise((res) => { const m = { id: ++id, method, params }; if (sessionId) m.sessionId = sessionId; pending.set(m.id, res); ws.send(JSON.stringify(m)) })
const enable = async (sessionId) => {
  await send('Runtime.enable', {}, sessionId)
  await send('Log.enable', {}, sessionId)
  await send('Network.enable', {}, sessionId)
  await send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true }, sessionId)
}
ws.addEventListener('message', async (ev) => {
  const m = JSON.parse(ev.data)
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m.result ?? m.error); pending.delete(m.id); return }
  const where = m.sessionId ? 'frame' : 'page'
  if (m.method === 'Target.attachedToTarget') { lines.push(`attached ${m.params.targetInfo.type} ${m.params.targetInfo.url}`); enable(m.params.sessionId) }
  if (m.method === 'Runtime.consoleAPICalled') lines.push(`${where} console.${m.params.type}: ${m.params.args.map((a) => a.value ?? a.description).join(' ')}`)
  if (m.method === 'Runtime.exceptionThrown') lines.push(`${where} exception: ${m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text}`)
  if (m.method === 'Log.entryAdded') lines.push(`${where} log ${m.params.entry.level}: ${m.params.entry.text}`)
  if (m.method === 'Network.requestWillBeSent') lines.push(`${where} request ${m.params.request.method} ${m.params.request.url.slice(0, 120)}`)
  if (m.method === 'Network.loadingFailed') lines.push(`${where} failed: ${m.params.errorText} ${m.params.blockedReason ?? ''} ${m.params.corsErrorStatus ? JSON.stringify(m.params.corsErrorStatus) : ''}`)
})
await enable()
await send('Page.enable')
await send('Page.navigate', { url })
await sleep(wait)
const out = await send('Runtime.evaluate', { expression: "document.getElementById('out')?.textContent", returnByValue: true })
console.log(lines.join('\n'))
console.log('--- #out ---')
console.log(out?.result?.value)
ws.close()
chrome.kill()
