#!/usr/bin/env node
// Runs a conversation in the running app, end to end, the way a person would: types each
// prompt into the composer, waits for the answer, optionally approves code runs, and saves a
// screenshot after every turn. Prints the tools each turn called.
//
//   node scripts/scenario.mjs --out .dev/scenario --approve "prompt one" "prompt two" ...
//
// Needs the app started for driving (see scripts/drive-transport.mjs). Starts a new chat.

import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { connect } from './drive-transport.mjs';

const args = process.argv.slice(2);
let out = '.dev/scenario';
let approve = false;
const prompts = [];
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === '--out') out = args[++i];
  else if (args[i] === '--approve') approve = true;
  else prompts.push(args[i]);
}
mkdirSync(out, { recursive: true });

const page = await connect();
const { evaluate } = page;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const shot = async (name) => {
  const file = path.join(out, `${name}.png`);
  writeFileSync(file, await page.screenshot());
  return file;
};

// A new chat.
await evaluate(`document.querySelector('[aria-label^="New chat"]')?.click()`);
await sleep(500);

for (const [index, prompt] of prompts.entries()) {
  const started = Date.now();
  // Type like a person: focus the composer, insert text, press Enter.
  await evaluate(`document.querySelector('[aria-label="Message"]').focus()`);
  await page.insertText(prompt);
  await page.key('Enter');
  await sleep(1200);
  // Wait for the turn to end, approving code runs when asked.
  for (;;) {
    const state = await evaluate(`(() => ({
      running: !!document.querySelector('[aria-label="Stop"]'),
      approval: !![...document.querySelectorAll('button')].find(b => b.textContent.trim() === 'Run once'),
    }))()`);
    if (state.approval) {
      if (!approve) {
        await shot(`${String(index + 1).padStart(2, '0')}-approval`);
        console.log('  waiting for approval (run with --approve to click it)');
      }
      await shot(`${String(index + 1).padStart(2, '0')}-approval`);
      await evaluate(
        `[...document.querySelectorAll('button')].find(b => b.textContent.trim() === 'Run once')?.click()`,
      );
      await sleep(800);
      continue;
    }
    if (!state.running) break;
    if (Date.now() - started > 15 * 60_000) throw new Error('turn took longer than 15 minutes');
    await sleep(800);
  }
  await sleep(600);
  const summary = await evaluate(`(() => {
    const cards = [...document.querySelectorAll('[class*="ToolCard"] [class*="label"], [class*="card"] > button [class*="label"]')].map(e => e.textContent);
    const errors = [...document.querySelectorAll('[class*="error"]')].map(e => e.textContent).filter(Boolean);
    return { cards, errors };
  })()`);
  const file = await shot(String(index + 1).padStart(2, '0'));
  console.log(`turn ${index + 1} (${((Date.now() - started) / 1000).toFixed(1)}s) -> ${file}`);
  console.log(`  prompt: ${prompt.slice(0, 100)}`);
  if (summary.errors.length) console.log(`  errors: ${summary.errors.join(' | ').slice(0, 400)}`);
}
page.close();
