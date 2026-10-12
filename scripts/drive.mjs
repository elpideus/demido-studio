#!/usr/bin/env node
// Drives the running app's page, for screenshots and scripted checks: over WebView2's DevTools
// protocol on Windows, over the app's own bridge on Linux (see scripts/drive-transport.mjs for
// how to start the app either way). No dependencies.
//
//   node scripts/drive.mjs --eval "document.title"
//   node scripts/drive.mjs --invoke market_status
//   node scripts/drive.mjs --invoke send_message '{"text":"hi","modelId":"local:..."}'
//   node scripts/drive.mjs --screenshot out.png
//   node scripts/drive.mjs --click '[aria-label="Settings"]' --wait 500 --screenshot s.png
//   node scripts/drive.mjs --type "hello" --key Enter
//   node scripts/drive.mjs --hover '[aria-label="Maximize"]' --wait 600 --screenshot flyout.png
//
// Steps run in the order given, over one connection. --target picks the page whose URL
// contains the text (default: the app's own page).

import { writeFileSync } from 'node:fs';
import { connect } from './drive-transport.mjs';

const args = process.argv.slice(2);
let targetMatch = null;
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === '--target') targetMatch = args[i + 1];
}

const page = await connect({ target: targetMatch });
const { evaluate } = page;
const mouse = (type, x, y, extra) => page.mouse(type, x, y, extra);

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
      await page.insertText(next());
      break;
    case '--key':
      await page.key(next());
      break;
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
      writeFileSync(file, await page.screenshot());
      console.log(`saved ${file}`);
      break;
    }
    case '--resize': {
      const [w, h] = next().split('x').map(Number);
      await page.resize(w, h);
      break;
    }
    default:
      throw new Error(`unknown step ${a}`);
  }
}
page.close();
