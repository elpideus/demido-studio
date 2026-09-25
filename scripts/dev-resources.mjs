#!/usr/bin/env node
// Assembles `.dev/resources` the way an installation lays out `resources/`: the bundled market
// service and the default skills. `pnpm dev` runs this first.

import { cpSync, mkdirSync, rmSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const out = path.join(root, '.dev', 'resources');

mkdirSync(path.join(out, 'sidecars'), { recursive: true });
execFileSync(
  process.execPath,
  [path.join(root, 'sidecars/market/build.mjs'), path.join(out, 'sidecars', 'market.mjs')],
  {
    stdio: 'inherit',
  },
);
rmSync(path.join(out, 'skills'), { recursive: true, force: true });
cpSync(path.join(root, 'skills'), path.join(out, 'skills'), { recursive: true });
console.log(`dev resources ready in ${out}`);
