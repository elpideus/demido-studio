#!/usr/bin/env node
/* The desktop app's installers, as the release workflow handles them (DECISIONS F9 to F13).

     node scripts/desktop-installers.mjs collect --bundle <tauri's bundle folder> --version 0.1.0 --platform linux-x64 --out installers
     node scripts/desktop-installers.mjs manifest --in release --version 0.1.0 --tag v0.1.0 --repo owner/name

   `collect` copies what Tauri's bundler made into one folder, each named for the version and the platform
   (demido-studio-desktop-0.1.0-linux-x64.deb, …), with its updater signature beside it when the build was
   signed (`.sig`), and writes `updater-<platform>.json`: for each signed installer, the name the updater looks
   for it by (`linux-x86_64-deb`, `windows-x86_64-nsis`, `darwin-aarch64-app`, and `<os>-<arch>` for the
   usual one), its signature and its file.

   `manifest` puts the platforms' `updater-*.json` together into `latest.json`, the file the desktop app reads
   from the newest release (`releases/latest/download/latest.json`), with each file's address in the release,
   and removes the pieces. With no signed installer it writes nothing: an unsigned release offers no update. */
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'

const [, , action, ...rest] = process.argv
const given = {}
for (let i = 0; i < rest.length; i += 2) given[rest[i].replace(/^--/, '')] = rest[i + 1]
const stop = (why) => { console.error(`desktop-installers: ${why}`); process.exit(1) }

/** What each kind of file is called here, and how the updater names it. `updater` is null for what it cannot install. */
const KINDS = [
  { match: (n) => n.endsWith('.deb'), ext: 'deb', updater: 'deb' },
  { match: (n) => n.endsWith('.rpm'), ext: 'rpm', updater: 'rpm' },
  { match: (n) => n.endsWith('.AppImage'), ext: 'AppImage', updater: 'appimage', usual: true },
  { match: (n) => n.endsWith('-setup.exe'), ext: 'setup.exe', updater: 'nsis', usual: true },
  { match: (n) => n.endsWith('.app.tar.gz'), ext: 'app.tar.gz', updater: 'app', usual: true },
  { match: (n) => n.endsWith('.dmg'), ext: 'dmg', updater: null },
]
/** The updater's `<os>-<arch>` for each of the release's platforms. */
const TARGETS = { 'linux-x64': 'linux-x86_64', 'linux-arm64': 'linux-aarch64', 'windows-x64': 'windows-x86_64', 'windows-arm64': 'windows-aarch64', 'macos-arm64': 'darwin-aarch64', 'macos-x64': 'darwin-x86_64' }

function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    return statSync(path).isDirectory() && !name.endsWith('.app') ? files(path) : [path]
  })
}

if (action === 'collect') {
  const { bundle, version, platform, out } = given
  if (!bundle || !version || !platform || !out) stop('collect needs --bundle, --version, --platform and --out')
  const target = TARGETS[platform] ?? stop(`unknown platform ${platform}`)
  mkdirSync(out, { recursive: true })
  const updater = {}
  let made = 0
  for (const path of files(bundle)) {
    const kind = KINDS.find((k) => k.match(basename(path)))
    if (!kind) continue
    const name = `demido-studio-desktop-${version}-${platform}.${kind.ext}`
    copyFileSync(path, join(out, name))
    made++
    console.log(`${basename(path)} -> ${name}`)
    if (kind.updater && existsSync(`${path}.sig`)) {
      copyFileSync(`${path}.sig`, join(out, `${name}.sig`))
      const entry = { signature: readFileSync(`${path}.sig`, 'utf8').trim(), file: name }
      updater[`${target}-${kind.updater}`] = entry
      if (kind.usual) updater[target] = entry
    }
  }
  if (!made) stop(`no installer in ${bundle}`)
  if (Object.keys(updater).length) writeFileSync(join(out, `updater-${platform}.json`), JSON.stringify(updater, null, 1))
  console.log(Object.keys(updater).length ? `signed for the updater: ${Object.keys(updater).join(', ')}` : 'not signed for the updater (no signing key was given)')
} else if (action === 'manifest') {
  const { in: dir, version, tag, repo } = given
  if (!dir || !version || !tag || !repo) stop('manifest needs --in, --version, --tag and --repo')
  const pieces = readdirSync(dir).filter((n) => /^updater-.+\.json$/.test(n))
  const platforms = {}
  for (const piece of pieces) {
    for (const [target, entry] of Object.entries(JSON.parse(readFileSync(join(dir, piece), 'utf8')))) {
      platforms[target] = { signature: entry.signature, url: `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(entry.file)}` }
    }
    rmSync(join(dir, piece))
  }
  if (!Object.keys(platforms).length) {
    console.log('No installer was signed for the updater: no latest.json, and this release offers no update.')
    process.exit(0)
  }
  writeFileSync(join(dir, 'latest.json'), JSON.stringify({ version, notes: `Demido Studio ${version}`, pub_date: new Date().toISOString(), platforms }, null, 1))
  console.log(`latest.json: ${Object.keys(platforms).sort().join(', ')}`)
} else {
  stop('say collect or manifest')
}
