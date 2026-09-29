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
//
// Then, when TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH is set, it signs the
// installer the way the release workflow does (-> release/Demido-Studio-Setup-<version>.exe.sig),
// so a local build can be served to the updater. TAURI_SIGNING_PRIVATE_KEY_PASSWORD holds the
// key's password; without it the signer asks for it.

import { createHash } from 'node:crypto';
import { cpSync, copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, statSync } from 'node:fs';
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

// A key that is not there fails now, not after the whole build.
if (
  process.env.TAURI_SIGNING_PRIVATE_KEY_PATH &&
  !existsSync(path.resolve(process.env.TAURI_SIGNING_PRIVATE_KEY_PATH))
) {
  console.error(
    `TAURI_SIGNING_PRIVATE_KEY_PATH names ${path.resolve(process.env.TAURI_SIGNING_PRIVATE_KEY_PATH)}, which does not exist.`,
  );
  process.exit(1);
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
// A signature left by an earlier build would not match this installer.
rmSync(`${setup}.sig`, { force: true });

const signed = Boolean(process.env.TAURI_SIGNING_PRIVATE_KEY || process.env.TAURI_SIGNING_PRIVATE_KEY_PATH);
// The signer runs in apps/studio: a key path relative to where the build started must not change
// meaning on the way there.
const keyPath = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH && path.resolve(process.env.TAURI_SIGNING_PRIVATE_KEY_PATH);
if (signed) {
  console.log('\nSignature');
  // `pnpm exec` runs in the package's folder, so the installer is named relative to it (a path
  // without spaces, as run() needs for pnpm).
  const studio = path.join(root, 'apps', 'studio');
  run(
    pnpm,
    [
      '--filter',
      '@demido/studio',
      'exec',
      'tauri',
      'signer',
      'sign',
      '--app-version',
      version,
      path.relative(studio, setup),
    ],
    keyPath ? { TAURI_SIGNING_PRIVATE_KEY_PATH: keyPath } : {},
  );
}

const bytes = readFileSync(setup);
const sha = createHash('sha256').update(bytes).digest('hex');
console.log(`\nDone: ${setup}`);
console.log(`  ${(statSync(setup).size / 1e6).toFixed(1)} MB  sha256 ${sha}`);
console.log(
  signed
    ? `  signed: ${setup}.sig`
    : '  unsigned: set TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH to sign it for the updater',
);
