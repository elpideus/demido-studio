#!/usr/bin/env node
/* Checks that every dependency's licence can be combined with GPL-3.0-or-later.

   The brief's rule: MIT, BSD, Apache-2.0, MPL-2.0, LGPL and GPLv3 are fine. AGPL only as a separate process.
   GPLv2-only, proprietary and non-commercial licences are not. This script reads the licence every package
   declares (the core's and the desktop shell's through `cargo metadata`, the web UI's through `pnpm licenses`),
   and fails with the
   names of the packages that do not fit.

     node scripts/check-licences.mjs

   It reads what packages declare; it does not read their source for licence texts. A new licence name is
   never guessed at: it fails until someone has read it and added it to the list below, with the reason. */
import { spawnSync } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')

/** Licences that may be combined with GPL-3.0-or-later. Each was read, not assumed. */
const FINE = new Set([
  'MIT', 'MIT-0', 'ISC', '0BSD', 'BSD-2-Clause', 'BSD-3-Clause', 'Zlib', 'BSL-1.0', 'Unlicense', 'CC0-1.0',
  'Apache-2.0', // compatible with GPL version 3 (not with version 2)
  'MPL-2.0', // its section 3.3 allows combining with the GPL
  'LGPL-2.1-or-later', 'LGPL-3.0-only', 'LGPL-3.0-or-later', 'GPL-3.0-only', 'GPL-3.0-or-later',
  'Unicode-3.0', 'Unicode-DFS-2016', // Unicode's data tables: permissive
  'CDLA-Permissive-2.0', // a permissive licence for data (the list of trusted certificate authorities)
  'OFL-1.1', // fonts: may be bundled with any software
  'BlueOak-1.0.0', 'Python-2.0', 'CC-BY-4.0', // permissive; CC-BY for data and documentation only
  // bzip2's own licence (libbz2-rs-sys, read 2026-11-22): BSD-style, like zlib's. Its four conditions (keep the
  // notice, do not misrepresent the origin, mark altered versions, no endorsement by name) are the additional
  // terms GPL-3.0's section 7 allows: (b), (c), (c) and (d).
  'bzip2-1.0.6',
])
/** Exceptions that only add permissions. */
const FINE_WITH = new Set(['LLVM-exception'])

/* An SPDX expression: names joined by AND, OR and WITH, with brackets. "A OR B" is fine when either is
   (we may choose). "A AND B" is fine when both are. Returns the names that stand in the way, or []. */
function inTheWay(expression) {
  const words = expression.replace(/\//g, ' OR ').match(/\(|\)|[^\s()]+/g) ?? []
  let at = 0
  let cut = false // true when the expression ends where a name should be
  const either = () => {
    let bad = both()
    while (words[at]?.toUpperCase() === 'OR') {
      at++
      const other = both()
      bad = bad.length && other.length ? [...bad, ...other] : []
    }
    return bad
  }
  const both = () => {
    let bad = one()
    while (words[at]?.toUpperCase() === 'AND') {
      at++
      bad = [...bad, ...one()]
    }
    return bad
  }
  const one = () => {
    const word = words[at++]
    if (word === '(') {
      const bad = either()
      at++ // the closing bracket
      return bad
    }
    if (word === undefined || word === ')' || /^(AND|OR|WITH)$/i.test(word)) {
      cut = true
      return []
    }
    let bad = FINE.has(word) ? [] : [word]
    if (words[at]?.toUpperCase() === 'WITH') {
      const exception = words[at + 1]
      at += 2
      if (!FINE_WITH.has(exception)) bad = [...bad, `WITH ${exception}`]
    }
    return bad
  }
  const bad = either()
  return cut || at < words.length ? [...bad, `(could not read "${expression}")`] : bad
}

function run(program, args) {
  const out = spawnSync(program, args, { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, shell: process.platform === 'win32' })
  if (out.error || out.status !== 0) {
    console.error(`check-licences: could not run ${program} ${args.join(' ')}\n${out.error?.message ?? out.stderr}`)
    process.exit(2)
  }
  return out.stdout
}

const problems = []
const seen = new Map()
function check(where, name, version, licence) {
  const expression = (licence ?? '').trim()
  seen.set(expression || '(none declared)', (seen.get(expression || '(none declared)') ?? 0) + 1)
  if (!expression) return problems.push(`${where}: ${name} ${version} declares no licence`)
  const bad = inTheWay(expression)
  if (bad.length) problems.push(`${where}: ${name} ${version} is "${expression}" (not on the list: ${[...new Set(bad)].join(', ')})`)
}

// The core: every package that can end up in a build, on any platform.
const cargo = JSON.parse(run('cargo', ['metadata', '--format-version', '1', '--locked', '--manifest-path', 'core/Cargo.toml']))
const ours = new Set(cargo.workspace_members)
for (const p of cargo.packages) if (!ours.has(p.id)) check('core', p.name, p.version, p.license ?? (p.license_file ? `(see ${p.license_file})` : ''))

// The desktop shell: a workspace of its own (it links GTK and WebKit, which the core never does).
const desktop = JSON.parse(run('cargo', ['metadata', '--format-version', '1', '--locked', '--manifest-path', 'apps/desktop/src-tauri/Cargo.toml']))
const shell = new Set(desktop.workspace_members)
for (const p of desktop.packages) if (!shell.has(p.id)) check('desktop', p.name, p.version, p.license ?? (p.license_file ? `(see ${p.license_file})` : ''))

// The web UI and the brand kit: what pnpm installed.
// Every package of the workspace, not only the root's: without `-r` this saw three packages and missed the web UI's.
const pnpm = JSON.parse(run('pnpm', ['-r', 'licenses', 'list', '--json']))
for (const [licence, packages] of Object.entries(pnpm)) for (const p of packages) for (const version of p.versions ?? [p.version]) check('web', p.name, version, licence)

const total = [...seen.values()].reduce((a, b) => a + b, 0)
// A check that reads almost nothing is worse than no check: it says "all fine". The web UI alone has over a hundred packages.
const web = Object.values(pnpm).reduce((n, list) => n + list.length, 0)
if (web < 50) {
  console.error(`check-licences: only ${web} packages of the web UI were found. That cannot be all of them, so nothing was checked. Run pnpm install first.`)
  process.exit(2)
}
if (problems.length) {
  console.error(`check-licences: ${problems.length} of ${total} packages do not fit GPL-3.0-or-later, or have not been read yet:\n`)
  for (const line of problems.sort()) console.error(`  ${line}`)
  console.error('\nRead the licence. If it fits, add it to FINE in scripts/check-licences.mjs with the reason. If not, the package cannot be used.')
  process.exit(1)
}
console.log(`check-licences: ${total} packages, all fit GPL-3.0-or-later.`)
for (const [licence, count] of [...seen].sort((a, b) => b[1] - a[1])) console.log(`  ${String(count).padStart(4)}  ${licence}`)
