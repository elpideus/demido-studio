// How scripts/drive.mjs and scripts/scenario.mjs reach the running app's page. No dependencies.
//
// On Windows, over WebView2's DevTools protocol: start the app with
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 pnpm dev
// On Linux, where WebKitGTK has no DevTools protocol, over the app's own bridge (debug builds,
// apps/studio/src-tauri/src/drive.rs): start the app with
//   DEMIDO_DRIVE_PORT=9223 pnpm dev
//
// DRIVE=cdp or DRIVE=bridge picks one by hand; CDP_PORT and DEMIDO_DRIVE_PORT move the ports.
//
// connect() returns the same steps either way: evaluate (a page expression, awaited, its value),
// mouse (CDP's mouseMoved, mousePressed and mouseReleased at a point), insertText, key,
// screenshot (a PNG Buffer), resize and close.

/**
 * @param {{ target?: string | null }} [options] `target` picks the page whose URL (or, over the
 *   bridge, window label) contains the text; by default the app's own page.
 */
export async function connect({ target = null } = {}) {
  const kind = process.env.DRIVE ?? (process.platform === 'linux' ? 'bridge' : 'cdp');
  if (kind === 'bridge') return bridge(target);
  if (kind === 'cdp') return cdp(target);
  throw new Error(`DRIVE is ${kind}: cdp or bridge`);
}

const KEY_CODES = { Enter: 13, Escape: 27, Tab: 9, Backspace: 8, ArrowDown: 40, ArrowUp: 38 };

async function cdp(targetMatch) {
  const port = Number(process.env.CDP_PORT ?? 9222);
  const list = await fetch(`http://127.0.0.1:${port}/json`)
    .then((r) => r.json())
    .catch(() => {
      throw new Error(
        `nothing answers on port ${port}: start the app with remote debugging (scripts/drive-transport.mjs)`,
      );
    });
  const pages = list.filter((t) => t.type === 'page');
  const match = targetMatch
    ? pages.find((t) => t.url.includes(targetMatch))
    : (pages.find((t) => /localhost:1420|tauri\.localhost|tauri:\/\//.test(t.url)) ?? pages[0]);
  if (!match) throw new Error(`no page target on port ${port}: ${pages.map((p) => p.url).join(', ')}`);

  const ws = new WebSocket(match.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = reject;
  });
  let nextId = 1;
  const pending = new Map();
  ws.onmessage = (event) => {
    const msg = JSON.parse(event.data);
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    }
  };
  const send = (method, params = {}) => {
    const id = nextId++;
    ws.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve) => pending.set(id, resolve));
  };

  return {
    async evaluate(expression) {
      const res = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (res.result?.exceptionDetails) {
        throw new Error(res.result.exceptionDetails.exception?.description ?? 'evaluation failed');
      }
      return res.result?.result?.value;
    },
    async mouse(type, x, y, extra = {}) {
      await send('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1, ...extra });
    },
    async insertText(text) {
      await send('Input.insertText', { text });
    },
    async key(key) {
      const code = KEY_CODES[key] ?? 0;
      await send('Input.dispatchKeyEvent', {
        type: 'keyDown',
        key,
        windowsVirtualKeyCode: code,
        text: key === 'Enter' ? '\r' : undefined,
      });
      await send('Input.dispatchKeyEvent', { type: 'keyUp', key, windowsVirtualKeyCode: code });
    },
    async screenshot() {
      const res = await send('Page.captureScreenshot', { format: 'png' });
      return Buffer.from(res.result.data, 'base64');
    },
    async resize(width, height) {
      await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
    },
    close() {
      ws.close();
    },
  };
}

async function bridge(targetMatch) {
  const port = Number(process.env.DEMIDO_DRIVE_PORT ?? 9223);
  const post = async (path, body = {}) => {
    const res = await fetch(`http://127.0.0.1:${port}${path}`, {
      method: 'POST',
      headers: { 'x-demido-drive': '1', 'content-type': 'application/json' },
      body: JSON.stringify(body),
    }).catch(() => {
      throw new Error(`nothing answers on port ${port}: start the app with DEMIDO_DRIVE_PORT=${port} pnpm dev`);
    });
    const answer = await res.json();
    if (answer?.error) throw new Error(answer.error);
    return answer;
  };

  const windows = await post('/targets');
  const label = targetMatch
    ? windows.find((w) => w.url.includes(targetMatch) || w.label.includes(targetMatch))?.label
    : 'main';
  if (!label)
    throw new Error(`no window matches ${targetMatch}: ${windows.map((w) => `${w.label} ${w.url}`).join(', ')}`);

  const evaluate = async (script) => (await post('/eval', { target: label, script })).value;
  // Input becomes DOM events, sent by a helper put in the page once.
  let helper = false;
  const page = async (call) => {
    if (!helper) {
      await evaluate(PAGE_HELPER);
      helper = true;
    }
    return evaluate(`window.__demidoDrive.${call}`);
  };

  return {
    evaluate,
    async mouse(type, x, y, extra = {}) {
      await page(`mouse(${JSON.stringify(type)}, ${x}, ${y}, ${JSON.stringify(extra.buttons ?? null)})`);
    },
    async insertText(text) {
      await page(`insertText(${JSON.stringify(text)})`);
    },
    async key(key) {
      await page(`key(${JSON.stringify(key)})`);
    },
    async screenshot() {
      return Buffer.from((await post('/screenshot', { target: label })).png, 'base64');
    },
    async resize(width, height) {
      await post('/resize', { target: label, width, height });
    },
    close() {},
  };
}

// Runs in the page: CDP's input, as DOM events. The pointer is emulated, so its capture is too,
// entirely: WebKit's own capture of the mouse, whose button is not really down, ends at once
// with a lostpointercapture that would end every drag before it starts.
const PAGE_HELPER = `(() => {
  if (window.__demidoDrive) return;
  const capture = { el: null };
  const proto = Element.prototype;
  proto.setPointerCapture = function () {
    capture.el = this;
  };
  proto.releasePointerCapture = function () {
    if (capture.el === this) capture.el = null;
  };
  proto.hasPointerCapture = function () {
    return capture.el === this;
  };

  let over = null;
  let down = null;
  let buttons = 0;
  let mouseEvents = true;
  const fire = (el, type, x, y, related = null) => {
    const init = {
      bubbles: !/enter|leave/.test(type), cancelable: true, composed: true, view: window, detail: 1,
      clientX: x, clientY: y, screenX: x, screenY: y, button: 0, buttons, relatedTarget: related,
    };
    const event = type.startsWith('pointer') || type === 'lostpointercapture'
      ? new PointerEvent(type, { ...init, pointerId: 1, pointerType: 'mouse', isPrimary: true })
      : new MouseEvent(type, init);
    return el.dispatchEvent(event);
  };
  const common = (a, b) => {
    for (let n = a; n; n = n.parentElement) if (n.contains(b)) return n;
    return document.documentElement;
  };
  const focusable = 'input, textarea, select, button, a[href], [tabindex], [contenteditable="true"], [contenteditable=""]';

  const mouse = (type, x, y, held) => {
    const hit = document.elementFromPoint(x, y) ?? document.documentElement;
    const target = capture.el ?? hit;
    if (hit !== over) {
      if (over) {
        fire(over, 'pointerout', x, y, hit); fire(over, 'pointerleave', x, y, hit);
        fire(over, 'mouseout', x, y, hit); fire(over, 'mouseleave', x, y, hit);
      }
      fire(hit, 'pointerover', x, y, over); fire(hit, 'pointerenter', x, y, over);
      fire(hit, 'mouseover', x, y, over); fire(hit, 'mouseenter', x, y, over);
      over = hit;
    }
    if (type === 'mouseMoved') {
      if (held !== null) buttons = held;
      fire(target, 'pointermove', x, y);
      if (mouseEvents) fire(target, 'mousemove', x, y);
    } else if (type === 'mousePressed') {
      buttons = 1;
      down = hit;
      // A cancelled pointerdown keeps the mouse events away until the button is up, as in a browser.
      mouseEvents = fire(target, 'pointerdown', x, y);
      if (mouseEvents && fire(target, 'mousedown', x, y)) {
        const next = hit.closest(focusable);
        if (next) next.focus();
        else document.activeElement?.blur?.();
      }
    } else if (type === 'mouseReleased') {
      buttons = 0;
      fire(target, 'pointerup', x, y);
      if (mouseEvents) fire(target, 'mouseup', x, y);
      if (capture.el) {
        const el = capture.el;
        capture.el = null;
        fire(el, 'lostpointercapture', x, y);
      }
      if (down) fire(common(down, hit), 'click', x, y);
      down = null;
      mouseEvents = true;
    }
  };

  const insertText = (text) => {
    const el = document.activeElement;
    if (!el || el === document.body) throw new Error('nothing has the focus to type into');
    if (document.execCommand('insertText', false, text)) return;
    const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value')?.set;
    if (!setter) throw new Error('the focused element takes no text');
    setter.call(el, el.value + text);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };

  const codes = { Enter: 13, Escape: 27, Tab: 9, Backspace: 8, ArrowDown: 40, ArrowUp: 38 };
  const key = (name) => {
    const el = document.activeElement ?? document.body;
    const code = name.length === 1 ? 'Key' + name.toUpperCase() : name;
    const init = { key: name, code, keyCode: codes[name] ?? 0, which: codes[name] ?? 0, bubbles: true, cancelable: true, composed: true };
    if (el.dispatchEvent(new KeyboardEvent('keydown', init)) && name === 'Backspace') document.execCommand('delete');
    el.dispatchEvent(new KeyboardEvent('keyup', init));
  };

  window.__demidoDrive = { mouse, insertText, key };
})()`;
