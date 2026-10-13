#!/usr/bin/env node
// Builds the release.
//
//   pnpm build             Windows: release/Demido-Studio-Setup-<version>.exe
//                          Linux:   release/demido-studio-<version>-linux-<arch>.tar.gz
//                                   release/Demido-Studio-<version>-<arch>.AppImage
//   pnpm build --tree      Linux: only the usr/ tree, in release/linux/ (what a PKGBUILD packages)
//
// The AppImage carries GStreamer, which bundling needs patchelf for; DEMIDO_APPIMAGE_GSTREAMER=0
// leaves it out of a build to try (voice input then uses the system's, if any).
//
// Windows, the single-file installer that carries the app:
//   1. bundle the market service and copy the default skills   -> release/staging/resources
//   2. build the app (no Tauri bundle: the installer is ours)   -> release/staging/demido-studio.exe
//   3. zip the staging folder                                    -> target/payload/demido-studio.zip
//   4. build the installer with the zip compiled in              -> target/release/demido-setup.exe
//
// Linux, where a package or the AppImage installs the app and setup travels beside it without it
// (setup's runtimes-only mode lays down the runtimes and models in the person's own folder):
//   1. the resources and 2. the app, as on Windows
//   3. setup, without a payload                                  -> target/release/demido-setup
//   4. the usr/ tree: usr/lib/demido-studio/{demido-studio, demido-setup, resources}, a link in
//      usr/bin, the desktop entry, icons and AppStream metadata  -> release/linux/, then .tar.gz
//   5. the AppImage, by Tauri's bundler (which brings WebKitGTK and GStreamer along), with setup
//      beside the app
//
// Then, when TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH is set, it signs what the
// updater downloads (the installer on Windows, the AppImage on Linux) the way the release
// workflow does (-> <file>.sig), so a local build can be served to the updater.
// TAURI_SIGNING_PRIVATE_KEY_PASSWORD holds the key's password; without it the signer asks for it.

import { createHash } from 'node:crypto';
import {
  chmodSync,
  cpSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { execFileSync, execSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const version = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')).version;
const linux = process.platform === 'linux';
const exe = process.platform === 'win32' ? '.exe' : '';
const release = path.join(root, 'release');
const staging = path.join(release, 'staging');
const payload = path.join(root, 'target', 'payload', 'demido-studio.zip');
const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
const treeOnly = process.argv.includes('--tree');

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

const steps = linux ? (treeOnly ? 4 : 5) : 4;

rmSync(staging, { recursive: true, force: true });
mkdirSync(path.join(staging, 'resources', 'sidecars'), { recursive: true });

console.log(`\n1/${steps} Resources`);
run(process.execPath, [
  path.join(root, 'sidecars/market/build.mjs'),
  path.join(staging, 'resources', 'sidecars', 'market.mjs'),
]);
cpSync(path.join(root, 'skills'), path.join(staging, 'resources', 'skills'), { recursive: true });
for (const file of ['LICENSE', 'THIRD_PARTY_NOTICES.md']) {
  copyFileSync(path.join(root, file), path.join(staging, file));
}

console.log(`\n2/${steps} App`);
run(pnpm, ['--filter', '@demido/studio', 'tauri', 'build', '--no-bundle']);
copyFileSync(path.join(root, 'target', 'release', `demido-studio${exe}`), path.join(staging, `demido-studio${exe}`));

const outputs = linux ? linuxRelease() : windowsInstaller();
const updaterFile = outputs.find((o) => o.forUpdater)?.file;

const signed = Boolean(process.env.TAURI_SIGNING_PRIVATE_KEY || process.env.TAURI_SIGNING_PRIVATE_KEY_PATH);
// The signer runs in apps/studio: a key path relative to where the build started must not change
// meaning on the way there.
const keyPath = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH && path.resolve(process.env.TAURI_SIGNING_PRIVATE_KEY_PATH);
if (signed && updaterFile) {
  console.log('\nSignature');
  // `pnpm exec` runs in the package's folder, so the file is named relative to it (a path without
  // spaces, as run() needs for pnpm on Windows).
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
      path.relative(studio, updaterFile),
    ],
    keyPath ? { TAURI_SIGNING_PRIVATE_KEY_PATH: keyPath } : {},
  );
}

console.log('\nDone:');
for (const { file } of outputs) {
  if (statSync(file).isDirectory()) {
    console.log(`  ${file}`);
    continue;
  }
  const sha = createHash('sha256').update(readFileSync(file)).digest('hex');
  console.log(`  ${file}\n    ${(statSync(file).size / 1e6).toFixed(1)} MB  sha256 ${sha}`);
}
if (updaterFile) {
  console.log(
    signed
      ? `  signed: ${updaterFile}.sig`
      : '  unsigned: set TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH to sign it for the updater',
  );
}

/** Windows: the installer, with the app zipped inside it. */
function windowsInstaller() {
  console.log(`\n3/${steps} Payload`);
  run('cargo', ['run', '--release', '-p', 'demido-setup-cli', '--', '--pack', staging, payload]);

  console.log(`\n4/${steps} Installer`);
  run(pnpm, ['--filter', '@demido/installer', 'tauri', 'build', '--no-bundle'], { DEMIDO_PAYLOAD: payload });
  const setup = path.join(release, `Demido-Studio-Setup-${version}${exe}`);
  copyFileSync(path.join(root, 'target', 'release', `demido-setup${exe}`), setup);
  // A signature left by an earlier build would not match this installer.
  rmSync(`${setup}.sig`, { force: true });
  return [{ file: setup, forUpdater: true }];
}

/** Linux: the usr/ tree, packed, and the AppImage. */
function linuxRelease() {
  const arch = { x64: 'x86_64', arm64: 'aarch64' }[process.arch];
  if (!arch) throw new Error(`No Linux release for ${process.arch}`);

  console.log(`\n3/${steps} Setup, without the app inside`);
  // An empty payload: setup is then runtimes-only (apps/installer/src-tauri/build.rs).
  run(pnpm, ['--filter', '@demido/installer', 'tauri', 'build', '--no-bundle'], { DEMIDO_PAYLOAD: '' });

  console.log(`\n4/${steps} The usr/ tree`);
  const name = `demido-studio-${version}-${arch}`;
  const tree = path.join(release, 'linux', name);
  rmSync(path.join(release, 'linux'), { recursive: true, force: true });
  const usr = path.join(tree, 'usr');
  const lib = path.join(usr, 'lib', 'demido-studio');
  const share = path.join(usr, 'share');
  mkdirSync(lib, { recursive: true });
  for (const bin of ['demido-studio', 'demido-setup']) {
    copyFileSync(path.join(root, 'target', 'release', bin), path.join(lib, bin));
    chmodSync(path.join(lib, bin), 0o755);
  }
  cpSync(path.join(staging, 'resources'), path.join(lib, 'resources'), { recursive: true });
  mkdirSync(path.join(usr, 'bin'), { recursive: true });
  symlinkSync('../lib/demido-studio/demido-studio', path.join(usr, 'bin', 'demido-studio'));
  const install = (from, to) => {
    mkdirSync(path.dirname(to), { recursive: true });
    copyFileSync(from, to);
  };
  install(
    path.join(root, 'packaging/linux/demido-studio.desktop'),
    path.join(share, 'applications/demido-studio.desktop'),
  );
  install(
    path.join(root, 'packaging/linux/app.demido.studio.metainfo.xml'),
    path.join(share, 'metainfo/app.demido.studio.metainfo.xml'),
  );
  const icons = path.join(root, 'apps/studio/src-tauri/icons');
  for (const [size, file] of [
    [32, '32x32.png'],
    [64, '64x64.png'],
    [128, '128x128.png'],
    [256, '128x128@2x.png'],
    [512, 'icon.png'],
  ]) {
    install(path.join(icons, file), path.join(share, `icons/hicolor/${size}x${size}/apps/demido-studio.png`));
  }
  install(path.join(root, 'design/app-icon.svg'), path.join(share, 'icons/hicolor/scalable/apps/demido-studio.svg'));
  install(path.join(root, 'LICENSE'), path.join(share, 'licenses/demido-studio/LICENSE'));
  install(path.join(root, 'THIRD_PARTY_NOTICES.md'), path.join(share, 'doc/demido-studio/THIRD_PARTY_NOTICES.md'));
  if (treeOnly) return [{ file: tree }];

  const tarball = path.join(release, `demido-studio-${version}-linux-${arch}.tar.gz`);
  run('tar', ['--owner=0', '--group=0', '--numeric-owner', '-czf', tarball, '-C', path.join(release, 'linux'), name]);

  console.log(`\n5/${steps} AppImage`);
  // Setup goes beside the app (an external binary, named for the target as Tauri wants it), the
  // resources where the app looks for them in an AppImage (usr/lib/<product name>/resources).
  const triple = { x86_64: 'x86_64-unknown-linux-gnu', aarch64: 'aarch64-unknown-linux-gnu' }[arch];
  const bins = path.join(root, 'target', 'appimage');
  rmSync(bins, { recursive: true, force: true });
  mkdirSync(bins, { recursive: true });
  copyFileSync(path.join(lib, 'demido-setup'), path.join(bins, `demido-setup-${triple}`));
  const config = path.join(bins, 'tauri.appimage.json');
  writeFileSync(
    config,
    JSON.stringify({
      bundle: {
        active: true,
        targets: ['appimage'],
        category: 'Utility',
        shortDescription: 'An AI workspace that runs on your own computer',
        externalBin: [path.join(bins, 'demido-setup')],
        resources: { [`${path.join(lib, 'resources')}/`]: 'resources/' },
        // WebKitGTK records and plays sound through GStreamer: voice input needs it inside.
        // Bundling it needs patchelf; DEMIDO_APPIMAGE_GSTREAMER=0 leaves it out of a build to try.
        linux: { appimage: { bundleMediaFramework: process.env.DEMIDO_APPIMAGE_GSTREAMER !== '0' } },
      },
    }),
  );
  const bundleDir = path.join(root, 'target', 'release', 'bundle', 'appimage');
  rmSync(bundleDir, { recursive: true, force: true });
  // linuxdeploy is an AppImage itself: extracted, it needs no FUSE. The binaries are stripped by
  // Cargo already, and its own strip is older than what built them.
  run(pnpm, ['--filter', '@demido/studio', 'tauri', 'build', '--bundles', 'appimage', '--config', config], {
    APPIMAGE_EXTRACT_AND_RUN: '1',
    NO_STRIP: 'true',
  });
  const built = readdirSync(bundleDir).find((f) => f.endsWith('.AppImage'));
  if (!built) throw new Error(`Tauri made no AppImage in ${bundleDir}`);
  const appimage = path.join(release, `Demido-Studio-${version}-${arch}.AppImage`);
  copyFileSync(path.join(bundleDir, built), appimage);
  chmodSync(appimage, 0o755);
  rmSync(`${appimage}.sig`, { force: true });
  return [{ file: tarball }, { file: appimage, forUpdater: true }];
}
