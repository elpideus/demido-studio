#!/usr/bin/env node
// Drives the running app's webview over the Chrome DevTools Protocol, for screenshots and
// scripted checks. No dependencies.
//
// Start the app with remote debugging (debug builds only):
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 pnpm dev
//
// Then:
//   node scripts/drive.mjs --eval "document.title"
//   node scripts/drive.mjs --invoke market_status
//   node scripts/drive.mjs --invoke send_message '{"text":"hi","modelId":"local:..."}'
//   node scripts/drive.mjs --screenshot out.png
//   node scripts/drive.mjs --click '[aria-label="Settings"]' --wait 500 --screenshot s.png
//   node scripts/drive.mjs --type "hello" --key Enter
//   node scripts/drive.mjs --hover '[aria-label="Maximize"]' --wait 600 --screenshot flyout.png
//
// Steps run in the order given, over one connection. --target picks the page whose URL
// contains the text (default: the app page on localhost:1420 or tauri.localhost).

import { writeFileSync } from 'node:fs';

const args = process.argv.slice(2);
const port = Number(process.env.CDP_PORT ?? 9222);
let targetMatch = null;
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === '--target') targetMatch = args[i + 1];
}

async function pickTarget() {
  const list = await fetch(`http://127.0.0.1:${port}/json`).then((r) => r.json());
  const pages = list.filter((t) => t.type === 'page');
  const match = targetMatch
    ? pages.find((t) => t.url.includes(targetMatch))
    : (pages.find((t) => /localhost:1420|tauri\.localhost|tauri:\/\//.test(t.url)) ?? pages[0]);
  if (!match) throw new Error(`no page target on port ${port}: ${pages.map((p) => p.url).join(', ')}`);
  return match;
}

const target = await pickTarget();
const ws = new WebSocket(target.webSocketDebuggerUrl);
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
function send(method, params = {}) {
  const id = nextId++;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve) => pending.set(id, resolve));
}

async function evaluate(expression) {
  const res = await send('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (res.result?.exceptionDetails) {
    throw new Error(res.result.exceptionDetails.exception?.description ?? 'evaluation failed');
  }
  return res.result?.result?.value;
}

async function center(selector) {
  const box = await evaluate(`(() => {
    const el = document.querySelector(${JSON.stringify(selector)});
    if (!el) return null;
    el.scrollIntoView({ block: 'nearest' });
    const r = el.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`);
  if (!box) throw new Error(`no element matches ${selector}`);
  return box;
}

async function mouse(type, x, y, extra = {}) {
  await send('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1, ...extra });
}

/** Where the last --drag ended, for --release. */
let held = { x: 0, y: 0 };

for (let i = 0; i < args.length; i += 1) {
  const a = args[i];
  const next = () => args[++i];
  switch (a) {
    case '--target':
      next();
      break;
    case '--eval': {
      const value = await evaluate(next());
      console.log(typeof value === 'string' ? value : JSON.stringify(value, null, 2));
      break;
    }
    case '--invoke': {
      const cmd = next();
      const payload = args[i + 1] && !args[i + 1].startsWith('--') ? next() : '{}';
      const value = await evaluate(
        `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${payload}).then(v => ({ ok: v }), e => ({ error: String(e) }))`,
      );
      console.log(JSON.stringify(value, null, 2));
      break;
    }
    case '--click': {
      const { x, y } = await center(next());
      await mouse('mouseMoved', x, y);
      await mouse('mousePressed', x, y);
      await mouse('mouseReleased', x, y);
      break;
    }
    case '--hover': {
      // --hover selector moves the pointer onto an element; --hover x,y onto a point.
      const target = next();
      const point = /^\d+(\.\d+)?,\d+(\.\d+)?$/.test(target)
        ? Object.fromEntries(target.split(',').map((v, n) => [n ? 'y' : 'x', Number(v)]))
        : await center(target);
      await mouse('mouseMoved', point.x, point.y, { button: 'none' });
      break;
    }
    case '--drag': {
      // --drag "fromSelector|toX,toY" drags from an element's center to a point. A third part,
      // "|hold", keeps the button down (to screenshot mid-drag) until --release.
      const [from, to, hold] = next().split('|');
      const start = await center(from);
      const [tx, ty] = to.split(',').map(Number);
      await mouse('mouseMoved', start.x, start.y);
      await mouse('mousePressed', start.x, start.y);
      const steps = 12;
      for (let s = 1; s <= steps; s += 1) {
        await mouse('mouseMoved', start.x + ((tx - start.x) * s) / steps, start.y + ((ty - start.y) * s) / steps, {
          buttons: 1,
        });
      }
      held = { x: tx, y: ty };
      if (hold !== 'hold') await mouse('mouseReleased', tx, ty);
      break;
    }
    case '--release':
      await mouse('mouseReleased', held.x, held.y);
      break;
    case '--type':
      await send('Input.insertText', { text: next() });
      break;
    case '--key': {
      const key = next();
      const code = { Enter: 13, Escape: 27, Tab: 9, Backspace: 8, ArrowDown: 40, ArrowUp: 38 }[key] ?? 0;
      await send('Input.dispatchKeyEvent', {
        type: 'keyDown',
        key,
        windowsVirtualKeyCode: code,
        text: key === 'Enter' ? '\r' : undefined,
      });
      await send('Input.dispatchKeyEvent', { type: 'keyUp', key, windowsVirtualKeyCode: code });
      break;
    }
    case '--wait': {
      const v = next();
      if (/^\d+$/.test(v)) await new Promise((r) => setTimeout(r, Number(v)));
      else {
        const deadline = Date.now() + 60_000;
        while (!(await evaluate(v))) {
          if (Date.now() > deadline) throw new Error(`timed out waiting for ${v}`);
          await new Promise((r) => setTimeout(r, 250));
        }
      }
      break;
    }
    case '--screenshot': {
      const file = next();
      const res = await send('Page.captureScreenshot', { format: 'png' });
      writeFileSync(file, Buffer.from(res.result.data, 'base64'));
      console.log(`saved ${file}`);
      break;
    }
    case '--resize': {
      const [w, h] = next().split('x').map(Number);
      await send('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false });
      break;
    }
    default:
      throw new Error(`unknown step ${a}`);
  }
}
ws.close();
