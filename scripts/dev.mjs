#!/usr/bin/env node
// Starts the app in development.
//
//   pnpm dev                 the app for this system, with hot reload (`tauri dev`; other
//                            arguments go to it)
//   pnpm dev windows         Linux: the Windows build, under Proton (see dev-windows.mjs)
//   pnpm dev:windows         the same
//
// Both first assemble `.dev/resources` (dev-resources.mjs).

import { execFileSync, execSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);

execFileSync(process.execPath, [path.join(root, 'scripts', 'dev-resources.mjs')], { cwd: root, stdio: 'inherit' });

if (args[0] === 'windows') {
  await import('./dev-windows.mjs');
} else {
  // pnpm is a .cmd script on Windows and only runs through a shell, so the arguments are quoted.
  const quoted = args.map((a) => JSON.stringify(a)).join(' ');
  try {
    execSync(`pnpm --filter @demido/studio tauri dev ${quoted}`, { cwd: root, stdio: 'inherit' });
  } catch (e) {
    process.exit(e.status ?? 1);
  }
}
