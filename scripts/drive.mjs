/**
 * Drives the running Demido window over CDP, for the window gate.
 *
 * `docs/rules/done.md` names the trap this exists to refuse:
 *
 *     typeof window.__TAURI__           = undefined
 *     typeof window.__TAURI_INTERNALS__ = object
 *
 * The internals object is non-enumerable, so listing `window` reports no Tauri
 * at all and a healthy app inspects as a broken one. v2 saw this as "Request
 * timeout after 2000ms" from every webview tool while its Rust side worked. So
 * this asserts both handles at connect and says which one is missing, in one
 * line, rather than timing out and leaving the caller to guess.
 *
 * Two switches, deliberately not one. The debugging port opens on any debug
 * build, which is what lets this connect to a window started with `pnpm dev`
 * and report the missing handle rather than guess at silence. `withGlobalTauri`
 * comes from `src-tauri/tauri.drive.conf.json`, merged by `pnpm dev:drive` and
 * never through `tauri.conf.json` or `capabilities/`. A release build has
 * neither, and is not drivable by this on purpose.
 *
 * No dependencies, like `check-rules.mjs`: Node's own `fetch` and `WebSocket`
 * are enough, and a driver that needs an install is a driver that is skipped.
 *
 * Usage:
 *   pnpm dev:drive                        # in one terminal, then:
 *   node scripts/drive.mjs                # assert the handles and the channel
 *   node scripts/drive.mjs --screenshot evidence/38.png
 *   node scripts/drive.mjs --eval "document.title"
 *   node scripts/drive.mjs --window splash --screenshot evidence/47.png
 *
 * ## One session, several steps
 *
 * `--click`, `--type`, `--key`, `--wait`, `--eval`, `--sleep` and `--screenshot`
 * are **steps**, run in the order they appear on the command line, over one
 * connection:
 *
 *   node scripts/drive.mjs \
 *     --click '[aria-label="Message"]' --type 'Save shipped into plan.txt.' --key Enter \
 *     --wait 'document.querySelector("[aria-label^=Approve]") !== null' \
 *     --screenshot evidence/59-a-call-waiting.png \
 *     --key Enter
 *
 * That shape is the point rather than a convenience.
 * [#59](https://github.com/elpideus/demido-studio/issues/59) asks for an
 * approval and a denial driven "in one CDP session", because they are two
 * answers to the same prompt and a fresh connection between them would be two
 * conversations that never met. It is also the only way to photograph a state
 * that exists between two keystrokes: a prompt waiting on somebody is gone the
 * moment the process that made it wait reconnects.
 *
 * The input is real. `--click` and `--key` go through `Input.dispatchMouseEvent`
 * and `Input.dispatchKeyEvent`, and `--type` through `Input.insertText`, so the
 * window sees what a person's hands would produce: React's own change handler
 * runs, and `web/src/shell/keys.ts` decides whether the keystroke belongs to the
 * field or to a binding exactly as it does for anybody. Driving the same thing
 * by calling a store's setter would prove the store works and nothing about the
 * desk.
 *
 * Which is why the click on the approval row is there before the Enter: with
 * focus in the composer, Enter is the field's and sends the message. That is the
 * scoping rule working, not a driver quirk.
 *
 * There are two windows now, the desk and the splash it opens ahead of
 * (`design/splash.md`), so a run says which one it means. `--window splash`
 * takes the splash; anything else is matched as a substring of the page URL;
 * the default, `desk`, is every window that is not the splash. It defaults that
 * way because the splash is gone within a second of an ordinary launch, and a
 * driver that silently caught it would report a different window on every run.
 * `DEMIDO_BOOT_HOLD_MS` is what holds the splash still long enough to
 * photograph.
 */

import { writeFileSync, mkdirSync } from 'node:fs'
import { dirname } from 'node:path'

const args = process.argv.slice(2)
const flag = (name, fallback) => {
  const at = args.indexOf(`--${name}`)
  if (at === -1) return fallback
  const value = args[at + 1]
  if (value === undefined || value.startsWith('--')) {
    console.error(`drive: --${name} needs a value`)
    process.exit(2)
  }
  return value
}

const PORT = Number(flag('port', 9222))
const TIMEOUT = Number(flag('timeout', 30_000))
/** Which window: `desk` is anything that is not the splash, and any other
 * value is matched as a substring of the page URL. The desk is served from the
 * dev server's root rather than from a path that names it, so it is identified
 * by what it is not. */
const WINDOW = flag('window', 'desk')
const wanted = (target) =>
  WINDOW === 'desk' ? !target.url.includes('splash') : target.url.includes(WINDOW)

/** Which flags are steps, and therefore ordered. Everything else is an option
 * about the connection and is read once, above. */
const STEPS = new Set(['eval', 'wait', 'click', 'type', 'key', 'sleep', 'screenshot'])

/**
 * The steps, in the order they were written.
 *
 * Read off `argv` rather than out of the `flag` table, because the table is a
 * lookup and the whole value of a sequence is that `--key Enter` before a
 * `--screenshot` is a different run from the same two the other way round.
 */
const script = []
for (let at = 0; at < args.length; at += 1) {
  const name = args[at].startsWith('--') ? args[at].slice(2) : null
  if (!name || !STEPS.has(name)) continue
  const value = args[at + 1]
  if (value === undefined || value.startsWith('--')) {
    console.error(`drive: --${name} needs a value`)
    process.exit(2)
  }
  script.push({ name, value })
  at += 1
}

/** Poll until `check` returns something truthy, or give up with `message`.
 * Both waits in this script are this shape; writing it twice is how they come
 * to disagree about their timeout. */
async function until(check, message) {
  const deadline = Date.now() + TIMEOUT
  let detail = ''
  while (Date.now() < deadline) {
    const [got, why] = await check()
    if (got) return got
    detail = why ?? detail
    await new Promise((resolve) => setTimeout(resolve, 150))
  }
  die(`${message} after ${TIMEOUT}ms${detail ? ` (${detail})` : ''}`)
}

/** The socket, so a failure closes it before the process ends. Exiting with a
 * WebSocket still open trips a libuv assertion, which buries the sentence this
 * script exists to print under a crash from somebody else's C file. */
let session = null

class Refused extends Error {}
const die = (message) => {
  throw new Refused(message)
}

/**
 * The window's page target.
 *
 * WebView2 opens the debugging endpoint when the browser arguments ask for it,
 * which a driven debug build does and nothing else does. A refused connection
 * therefore means the app is not running, or is running without the relaxation,
 * and those are two different sentences.
 */
async function findTarget() {
  return until(async () => {
    try {
      const response = await fetch(`http://127.0.0.1:${PORT}/json/list`)
      const targets = await response.json()
      // `about:blank` is the webview before it has navigated. Connecting to it
      // reports both handles missing, which is the one wrong answer this
      // script exists to never give, so it is waited out rather than read.
      const pages = targets.filter(
        (t) => t.type === 'page' && t.webSocketDebuggerUrl && t.url !== 'about:blank',
      )
      const page = pages.find(wanted)
      if (page) return [page]
      return [
        null,
        pages.length
          ? `${pages.length} window(s), none of them matching "${WINDOW}": ${pages
              .map((t) => t.url)
              .join(', ')}`
          : targets.some((t) => t.type === 'page')
            ? 'the webview is still on about:blank'
            : `${targets.length} target(s), none of them a page`,
      ]
    } catch (error) {
      return [null, error.cause?.code ?? error.message]
    }
  }, `no debuggable window on 127.0.0.1:${PORT}. The port opens on any debug build, so this is either nothing running or a release build, which is not drivable and is not meant to be`)
}

/** A CDP session over one WebSocket, with ids matched to replies. */
async function connect(url) {
  const socket = new WebSocket(url)
  const pending = new Map()
  let nextId = 1

  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true })
    socket.addEventListener('error', () => reject(new Error('the CDP socket refused')), {
      once: true,
    })
  })

  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data)
    const waiting = pending.get(message.id)
    if (!waiting) return
    pending.delete(message.id)
    if (message.error) waiting.reject(new Error(message.error.message))
    else waiting.resolve(message.result)
  })

  return {
    send(method, params = {}) {
      const id = nextId++
      // Registered before the frame goes out: a reply that arrives between the
      // send and the registration is a reply nobody is waiting for.
      const reply = new Promise((resolve, reject) => pending.set(id, { resolve, reject }))
      socket.send(JSON.stringify({ id, method, params }))
      return reply
    },
    close: () => socket.close(),
  }
}

/** Wait until the document has finished loading and has a stylesheet. */
async function settled() {
  await until(
    async () => [
      await evaluate(`document.readyState === 'complete' && document.styleSheets.length > 0`),
      'no stylesheet yet',
    ],
    'the window never finished loading',
  )
}

/** Evaluate an expression in the page and return its value, awaiting promises. */
async function evaluate(expression) {
  const result = await session.send('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  })
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.exception?.description ?? 'evaluation threw')
  }
  return result.result.value
}

/**
 * The keys this driver knows how to press, with the codes WebView2 wants.
 *
 * A short table rather than a general one: `keydown` handlers read `event.key`,
 * and the only chords bound anywhere in this application are the two on the
 * approval row plus Escape on a popover. A driver that guessed virtual key
 * codes for arbitrary text would be a driver whose failures look like the app's.
 */
const KEYS = {
  Enter: { code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' },
  Escape: { code: 'Escape', windowsVirtualKeyCode: 27 },
  Tab: { code: 'Tab', windowsVirtualKeyCode: 9 },
}

/** Press and release one key, where the window is looking. */
async function press(name) {
  const key = KEYS[name]
  if (!key)
    die(`--key ${name} is not one this driver has a code for: ${Object.keys(KEYS).join(', ')}`)
  for (const type of ['keyDown', 'keyUp']) {
    await session.send('Input.dispatchKeyEvent', { type, key: name, ...key })
  }
}

/** Click the middle of the first element matching `selector`. */
async function click(selector) {
  const box = await evaluate(
    `(() => {
       const found = document.querySelector(${JSON.stringify(selector)})
       if (!found) return null
       const at = found.getBoundingClientRect()
       return { x: at.left + at.width / 2, y: at.top + at.height / 2 }
     })()`,
  )
  if (!box) die(`--click found nothing matching ${selector}`)
  for (const type of ['mousePressed', 'mouseReleased']) {
    await session.send('Input.dispatchMouseEvent', {
      type,
      x: box.x,
      y: box.y,
      button: 'left',
      clickCount: 1,
    })
  }
}

/** Write the page to `path`, making the directory if it is not there. */
async function capture(path) {
  await session.send('Page.enable')
  // The splash is a transparent frameless window with a rounded face
  // (design/splash.md). Without this the capture composites it onto white and
  // the evidence shows four white corners the running window does not have.
  await session.send('Emulation.setDefaultBackgroundColorOverride', {
    color: { r: 0, g: 0, b: 0, a: 0 },
  })
  const shot = await session.send('Page.captureScreenshot', {
    format: 'png',
    captureBeyondViewport: false,
  })
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, Buffer.from(shot.data, 'base64'))
  console.log(`drive: screenshot written to ${path}`)
}

/** Run one step, and say what it did. */
async function step({ name, value }) {
  switch (name) {
    case 'eval':
      console.log(`drive: eval ${JSON.stringify(await evaluate(value))}`)
      return
    case 'wait':
      await until(
        async () => [await evaluate(value), 'still false'],
        `--wait never became true: ${value}`,
      )
      console.log(`drive: wait ${value}`)
      return
    case 'click':
      await click(value)
      console.log(`drive: click ${value}`)
      return
    // `Input.insertText` rather than a key per character: React reads the
    // `input` event, and this produces exactly one for the whole string
    // without this script having to hold a keyboard layout.
    case 'type':
      await session.send('Input.insertText', { text: value })
      console.log(`drive: type ${JSON.stringify(value)}`)
      return
    case 'key':
      await press(value)
      console.log(`drive: key ${value}`)
      return
    case 'sleep':
      await new Promise((resolve) => setTimeout(resolve, Number(value)))
      console.log(`drive: sleep ${value}ms`)
      return
    case 'screenshot':
      await capture(value)
      return
    default:
      die(`drive: ${name} is not a step`)
  }
}

async function drive() {
  const target = await findTarget()
  session = await connect(target.webSocketDebuggerUrl)
  await session.send('Runtime.enable')

  // A window caught mid-load answers every question honestly and uselessly:
  // no stylesheet yet, so a screenshot of it is a screenshot of unstyled
  // markup. Waited out here rather than in every caller.
  await settled()

  // Both handles, read directly rather than by listing `window`, because the
  // internals are non-enumerable and a list reports neither.
  const handles = await evaluate(
    `({
       tauri: typeof window.__TAURI__,
       internals: typeof window.__TAURI_INTERNALS__,
       title: document.title,
       url: location.href,
     })`,
  )

  if (handles.internals !== 'object') {
    die(
      `window.__TAURI_INTERNALS__ is ${handles.internals}, so this is not a Tauri webview at all: ` +
        `the page at ${handles.url} is something else, or the frontend dev server is serving it alone.`,
    )
  }

  if (handles.tauri !== 'object') {
    die(
      `window.__TAURI__ is ${handles.tauri} while __TAURI_INTERNALS__ is present, so the IPC ` +
        `channel is fine and only the driver's handle is missing: the build was started without ` +
        `withGlobalTauri. Use \`pnpm dev:drive\`, not \`pnpm dev\`.`,
    )
  }

  // The channel, not just the handle. A global that exists and cannot reach
  // Rust is the same wasted day one layer down.
  const report = await evaluate(`window.__TAURI__.core.invoke('boot_report')`)
  console.log(`drive: "${handles.title}" at ${handles.url}`)
  console.log(`drive: both handles present, boot_report answered version ${report.version}`)

  for (const one of script) {
    await step(one)
  }
}

try {
  await drive()
} catch (error) {
  console.error(`drive: ${error instanceof Refused ? error.message : (error.stack ?? error)}`)
  process.exitCode = 1
} finally {
  session?.close()
}
