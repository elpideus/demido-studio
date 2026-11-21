#!/usr/bin/env node
/* Puts one release together: the program, the web UI beside it, the licence, and a short note.

   The release workflow runs this once per platform. It runs the same way on this computer:

     node scripts/package-release.mjs --version 0.1.0 --platform linux-x64 \
       --program core/target/release/demido --ui apps/web/dist --out release

   What comes out: release/demido-studio-0.1.0-linux-x64.tar.gz (a .zip on Windows), holding one folder.
   Nothing here needs a package from npm. */
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')

const { values: given } = parseArgs({
  options: {
    version: { type: 'string' },
    platform: { type: 'string' },
    program: { type: 'string' },
    ui: { type: 'string' },
    out: { type: 'string', default: 'release' },
  },
})

function stop(message) {
  console.error(`package-release: ${message}`)
  process.exit(1)
}

for (const name of ['version', 'platform', 'program', 'ui']) if (!given[name]) stop(`--${name} is missing.`)

const PLATFORMS = ['linux-x64', 'linux-arm64', 'windows-x64', 'windows-arm64', 'macos-arm64', 'macos-x64']
if (!PLATFORMS.includes(given.platform)) stop(`--platform must be one of: ${PLATFORMS.join(', ')}.`)
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/.test(given.version)) stop(`"${given.version}" is not a version like 0.1.0 or 0.1.0-alpha.1.`)

const program = resolve(given.program)
const ui = resolve(given.ui)
if (!existsSync(program) || !statSync(program).isFile()) stop(`the program is not at ${program}. Build it first.`)
if (!existsSync(join(ui, 'index.html'))) stop(`the web UI is not at ${ui} (no index.html). Build it first.`)

const windows = given.platform.startsWith('windows')
const macos = given.platform.startsWith('macos')
const name = `demido-studio-${given.version}-${given.platform}`
const out = resolve(given.out)
const folder = join(out, name)
const programName = windows ? 'demido.exe' : 'demido'
const start = windows ? '.\\demido.exe' : './demido'

rmSync(folder, { recursive: true, force: true })
mkdirSync(folder, { recursive: true })
cpSync(program, join(folder, programName))
if (!windows) chmodSync(join(folder, programName), 0o755)
cpSync(ui, join(folder, 'ui'), { recursive: true })
cpSync(join(root, 'LICENSE'), join(folder, 'LICENSE.txt'))

/* The note that travels with the program. It says what this build is and, just as plainly, what it is not. */
// The status lines are one file, shared with the text of the release page, so the two cannot drift apart.
const status = readFileSync(join(root, 'scripts', 'release-status.txt'), 'utf8').trim()
const unsigned = macos
  ? `
THIS BUILD IS NOT SIGNED
macOS marks programs downloaded with a browser and refuses to open unsigned ones.
To remove the mark from this folder, run this in Terminal, inside the folder:

    xattr -dr com.apple.quarantine .

Only do this for a file you downloaded from the Demido Studio releases page and
whose checksum you compared with SHA256SUMS.txt.
`
  : windows
    ? `
THIS BUILD IS NOT SIGNED
Windows may show "Windows protected your PC" the first time. Compare the checksum
of the .zip with SHA256SUMS.txt from the releases page before choosing "Run anyway".
`
    : ''

const note = `Demido Studio ${given.version} (${given.platform})
A local-first, open-source AI harness.

${status}

START
Open a terminal in this folder.

    ${start} serve

Then, in a second terminal in the same folder:

    ${start} open

Your browser opens the web UI, signed in. The Setup screen shows this computer
as the core reads it.

    ${start} --help      lists every command.
    ${start} system      shows what this computer has.
${unsigned}
WHAT IS IN THIS FOLDER
    ${programName.padEnd(12)} the program: server and command line
    ui/          the web UI. The program looks for it right here, beside itself.
    LICENSE.txt  GPL-3.0-or-later

WHERE IT KEEPS ITS FILES
Run "${start} status". It prints the folder. Set DEMIDO_HOME to choose another.

SOURCE
https://github.com/elpideus/demido-studio
`
writeFileSync(join(folder, 'README.txt'), windows ? note.replace(/\n/g, '\r\n') : note)

/* One archive per platform. tar keeps the "may be run" mark on Linux and macOS; Windows users expect a .zip. */
const archive = join(out, `${name}.${windows ? 'zip' : 'tar.gz'}`)
rmSync(archive, { force: true })
const made = windows
  ? spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command', `Compress-Archive -Path '${folder.replace(/'/g, "''")}' -DestinationPath '${archive.replace(/'/g, "''")}' -CompressionLevel Optimal`], { stdio: 'inherit' })
  : spawnSync('tar', ['-czf', archive, '-C', out, name], { stdio: 'inherit' })
if (made.error) stop(`could not start the archive tool: ${made.error.message}`)
if (made.status !== 0) stop(`the archive tool failed with status ${made.status}.`)
if (!existsSync(archive)) stop(`the archive was not written: ${archive}`)

rmSync(folder, { recursive: true, force: true })
const size = (statSync(archive).size / 1024 / 1024).toFixed(1)
console.log(`${basename(archive)}  ${size} MB`)

// Tell the workflow which file to upload, when there is a workflow.
if (process.env.GITHUB_OUTPUT) writeFileSync(process.env.GITHUB_OUTPUT, `archive=${archive}\nname=${basename(archive)}\n`, { flag: 'a' })
