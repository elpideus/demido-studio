#!/usr/bin/env node
// Builds the release: the app, its resources, and the single-file installer that carries them.
//
//   pnpm build            ->  release/Demido-Studio-Setup-<version>.exe
//
// Steps:
//   1. bundle the market service and copy the default skills   -> release/staging/resources
//   2. build the app (no Tauri bundle: the installer is ours)   -> release/staging/demido-studio.exe
//   3. zip the staging folder                                    -> target/payload/demido-studio.zip
//   4. build the installer with the zip compiled in              -> target/release/demido-setup.exe

import { createHash } from 'node:crypto';
import { cpSync, copyFileSync, mkdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { execFileSync, execSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const version = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')).version;
const exe = process.platform === 'win32' ? '.exe' : '';
const release = path.join(root, 'release');
const staging = path.join(release, 'staging');
const payload = path.join(root, 'target', 'payload', 'demido-studio.zip');
const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';

function run(cmd, args, env = {}) {
  console.log(`\n$ ${cmd} ${args.join(' ')}`);
  const options = { cwd: root, stdio: 'inherit', env: { ...process.env, ...env } };
  if (process.platform === 'win32' && cmd === pnpm) {
    // pnpm is a .cmd script on Windows and only runs through a shell. Nothing passed to it
    // contains spaces, so the command line needs no quoting.
    execSync([cmd, ...args].join(' '), options);
  } else {
    // Everything else runs directly, so paths with spaces stay intact.
    execFileSync(cmd, args, options);
  }
}

rmSync(staging, { recursive: true, force: true });
mkdirSync(path.join(staging, 'resources', 'sidecars'), { recursive: true });

console.log('\n1/4 Resources');
run(process.execPath, [
  path.join(root, 'sidecars/market/build.mjs'),
  path.join(staging, 'resources', 'sidecars', 'market.mjs'),
]);
cpSync(path.join(root, 'skills'), path.join(staging, 'resources', 'skills'), { recursive: true });
for (const file of ['LICENSE', 'THIRD_PARTY_NOTICES.md']) {
  copyFileSync(path.join(root, file), path.join(staging, file));
}

console.log('\n2/4 App');
run(pnpm, ['--filter', '@demido/studio', 'tauri', 'build', '--no-bundle']);
copyFileSync(path.join(root, 'target', 'release', `demido-studio${exe}`), path.join(staging, `demido-studio${exe}`));

console.log('\n3/4 Payload');
run('cargo', ['run', '--release', '-p', 'demido-setup-cli', '--', '--pack', staging, payload]);

console.log('\n4/4 Installer');
run(pnpm, ['--filter', '@demido/installer', 'tauri', 'build', '--no-bundle'], { DEMIDO_PAYLOAD: payload });
const setupName = `Demido-Studio-Setup-${version}${exe}`;
const setup = path.join(release, setupName);
copyFileSync(path.join(root, 'target', 'release', `demido-setup${exe}`), setup);

const bytes = readFileSync(setup);
const sha = createHash('sha256').update(bytes).digest('hex');
console.log(`\nDone: ${setup}`);
console.log(`  ${(statSync(setup).size / 1e6).toFixed(1)} MB  sha256 ${sha}`);
