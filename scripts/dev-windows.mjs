#!/usr/bin/env node
// Runs the Windows build of the app on Linux, under Proton, to test it without a Windows machine.
// `pnpm dev windows` (or `pnpm dev:windows`) runs it after assembling `.dev/resources`.
//
//   1. build the app for x86_64-pc-windows-msvc with cargo-xwin (a debug build, as `tauri dev`
//      makes, which loads the page from Vite)
//   2. on the first run, make a Proton prefix in `.dev/proton` and install WebView2 in it
//   3. start Vite on 127.0.0.1:1420, the page hot reloads as with `pnpm dev`
//   4. start the app with umu-run; closing it (or Ctrl+C) stops Vite and the prefix
//
// Needs umu-run, a Proton (GE-Proton, Proton-CachyOS, UMU-Proton or Steam's Proton Experimental,
// or the one DEMIDO_PROTON names), `cargo install --locked cargo-xwin` and
// `rustup target add x86_64-pc-windows-msvc`. Rust changes need a restart.
//
// Plain Wine is not enough: WebView2 needs DirectComposition, which Wine does not implement, and
// its browser process stops at once. Everything stays inside the prefix: its own data folder
// (C:\users\steamuser\AppData\Local\Demido Studio), no menu entries or file associations on the
// desktop (winemenubuilder is off), umu's runtime is not updated. Deleting `.dev/proton` undoes
// it. The install folder is `.dev/install-windows` (DEMIDO_INSTALL_DIR sets another), so the
// Linux runtimes in `.dev/install` are never picked up.
//
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS reaches WebView2 as on Windows, e.g.
// `--remote-debugging-port=9222` for scripts/drive.mjs: Proton's WebView2 ignores the variable,
// so the build carries it.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const target = 'x86_64-pc-windows-msvc';
const exe = path.join(root, 'target', target, 'debug', 'demido-studio.exe');
const prefix = path.join(root, '.dev', 'proton');
const webview2Installer = path.join(root, '.dev', 'cache', 'webview2', 'MicrosoftEdgeWebView2RuntimeInstallerX64.exe');
// Microsoft's standalone x64 WebView2 runtime installer.
const webview2Url = 'https://go.microsoft.com/fwlink/?linkid=2124701';
const devHost = '127.0.0.1';
const devUrl = `http://${devHost}:1420`;
// What wry passes when it is given no arguments; given some, it passes only those.
const wryDefaultArgs = '--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection';

function fail(message) {
  console.error(`\n${message}`);
  process.exit(1);
}

function run(cmd, args, env = {}) {
  console.log(`\n$ ${cmd} ${args.join(' ')}`);
  const result = spawnSync(cmd, args, { cwd: root, stdio: 'inherit', env: { ...process.env, ...env } });
  if (result.error) fail(`${cmd}: ${result.error.message}`);
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function found(cmd, args) {
  const result = spawnSync(cmd, args, { stdio: 'ignore' });
  return !result.error && result.status === 0;
}

/** The Proton to run with: DEMIDO_PROTON, else the newest one installed for Steam. */
function findProton() {
  if (process.env.DEMIDO_PROTON) {
    const dir = path.resolve(process.env.DEMIDO_PROTON);
    if (!existsSync(path.join(dir, 'proton'))) fail(`DEMIDO_PROTON names ${dir}, which holds no Proton.`);
    return dir;
  }
  const home = os.homedir();
  const tools = [
    path.join(home, '.local', 'share', 'Steam', 'compatibilitytools.d'),
    path.join(home, '.steam', 'root', 'compatibilitytools.d'),
  ];
  const byVersion = (a, b) => b.localeCompare(a, undefined, { numeric: true });
  for (const family of [/^GE-Proton/, /^Proton-CachyOS/, /^UMU-Proton/]) {
    for (const dir of tools) {
      if (!existsSync(dir)) continue;
      const match = readdirSync(dir)
        .filter((name) => family.test(name) && existsSync(path.join(dir, name, 'proton')))
        .sort(byVersion)[0];
      if (match) return path.join(dir, match);
    }
  }
  const experimental = path.join(home, '.local', 'share', 'Steam', 'steamapps', 'common', 'Proton - Experimental');
  if (existsSync(path.join(experimental, 'proton'))) return experimental;
  fail(
    'No Proton found. Install GE-Proton (e.g. with ProtonUp-Qt) or Proton Experimental from Steam, ' +
      'or set DEMIDO_PROTON to a Proton folder.',
  );
}

/** A Linux path as the Windows side sees it (Proton maps `/` to Z:); a Windows path as it is. */
function windowsPath(p) {
  return p.startsWith('/') ? `Z:${p.replace(/\//g, '\\')}` : p;
}

if (process.platform !== 'linux') fail('pnpm dev windows runs the Windows build on Linux. On Windows, run pnpm dev.');
if (!found('umu-run', ['--help'])) fail('umu-run is missing: install umu-launcher (sudo pacman -S umu-launcher).');
if (!found('cargo', ['xwin', '--version'])) fail('cargo-xwin is missing: cargo install --locked cargo-xwin');
const targets = spawnSync('rustup', ['target', 'list', '--installed'], { encoding: 'utf8' }).stdout ?? '';
if (!targets.split('\n').includes(target)) fail(`The Rust target is missing: rustup target add ${target}`);

const proton = findProton();
console.log(`Proton: ${proton}`);
const wineserver = path.join(proton, 'files', 'bin', 'wineserver');
const wineEnv = {
  WINEPREFIX: prefix,
  PROTONPATH: proton,
  GAMEID: 'umu-default',
  UMU_RUNTIME_UPDATE: '0',
  // No menu entries or file associations for the prefix on the person's desktop.
  WINEDLLOVERRIDES: ['winemenubuilder.exe=d', process.env.WINEDLLOVERRIDES].filter(Boolean).join(';'),
  // GE-Proton's screen reader bridge: only noise here.
  PROTON_USE_XALIA: '0',
};

// 1. The app. The configuration differs from tauri.conf.json in the page's address (Vite listens
// on 127.0.0.1, where the Windows side looks for localhost) and in WebView2's arguments.
const conf = JSON.parse(readFileSync(path.join(root, 'apps', 'studio', 'src-tauri', 'tauri.conf.json'), 'utf8'));
const extraArgs = process.env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS;
const tauriConfig = {
  build: { devUrl },
  ...(extraArgs && {
    app: {
      windows: conf.app.windows.map((w) => ({ ...w, additionalBrowserArgs: `${wryDefaultArgs} ${extraArgs}` })),
    },
  }),
};
run('cargo', ['xwin', 'build', '-p', 'demido-studio', '--target', target], {
  TAURI_CONFIG: JSON.stringify(tauriConfig),
});

// 2. The prefix, with WebView2.
const webview2Dir = path.join(prefix, 'drive_c', 'Program Files (x86)', 'Microsoft', 'EdgeWebView', 'Application');
if (!existsSync(webview2Dir) || !readdirSync(webview2Dir).some((name) => /^\d+\./.test(name))) {
  mkdirSync(prefix, { recursive: true });
  if (!existsSync(webview2Installer)) {
    mkdirSync(path.dirname(webview2Installer), { recursive: true });
    run('curl', ['-fL', '--progress-bar', '-o', webview2Installer, webview2Url]);
  }
  console.log('\nInstalling WebView2 in .dev/proton (once, a minute or two)');
  run('umu-run', [webview2Installer, '/silent', '/install'], wineEnv);
}

// 3. Vite, in its own process group so stopping it takes its children along.
const vite = spawn('pnpm', ['--filter', '@demido/studio', 'dev'], {
  cwd: root,
  stdio: 'inherit',
  detached: true,
  env: { ...process.env, TAURI_DEV_HOST: devHost, TAURI_ENV_DEBUG: 'true', TAURI_ENV_PLATFORM: 'windows' },
});

let stopping = false;
function stop(code) {
  if (stopping) return;
  stopping = true;
  try {
    process.kill(-vite.pid, 'SIGTERM');
  } catch {
    // Already gone.
  }
  spawnSync(wineserver, ['-k'], { env: { ...process.env, ...wineEnv }, stdio: 'ignore' });
  process.exit(code);
}
process.on('SIGINT', () => stop(130));
process.on('SIGTERM', () => stop(143));
vite.on('exit', (code) => {
  if (!stopping) {
    console.error('\nVite stopped.');
    stop(code ?? 1);
  }
});

const deadline = Date.now() + 60_000;
for (;;) {
  try {
    await fetch(devUrl);
    break;
  } catch {
    if (Date.now() > deadline) {
      console.error(`\nVite did not answer on ${devUrl}.`);
      stop(1);
    }
    await new Promise((r) => setTimeout(r, 300));
  }
}

// 4. The app.
console.log(`\n$ umu-run ${path.relative(root, exe)}`);
const noise = /^(MESA|pci id for fd|ProtonFixes\[|Proton: |ntsync: )/;
const app = spawn('umu-run', [exe], {
  cwd: root,
  stdio: ['ignore', 'pipe', 'pipe'],
  env: {
    ...process.env,
    ...wineEnv,
    DEMIDO_INSTALL_DIR: windowsPath(process.env.DEMIDO_INSTALL_DIR ?? path.join(root, '.dev', 'install-windows')),
  },
});
for (const [stream, out] of [
  [app.stdout, process.stdout],
  [app.stderr, process.stderr],
]) {
  let rest = '';
  stream.setEncoding('utf8');
  stream.on('data', (chunk) => {
    const lines = (rest + chunk).split('\n');
    rest = lines.pop();
    for (const line of lines) if (!noise.test(line)) out.write(`${line}\n`);
  });
}
app.on('exit', (code) => stop(code ?? 0));
