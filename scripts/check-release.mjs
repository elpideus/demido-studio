/**
 * The rules that only fire at a tag.
 *
 * `docs/rules/releases.md` section 6: `check-rules.mjs` stays the per-commit
 * checker, and the rules that need a version, a predecessor or a built
 * installer live here, because failing them has to fail the tag build rather
 * than every push.
 *
 * Four checks, and each one is a rule that already existed with nowhere to
 * point:
 *
 *   1. The version agrees with itself: the tag, `tauri.conf.json` and the Cargo
 *      workspace (`docs/rules/versioning.md`).
 *   2. The pin and its predecessor, and the line saying why a pin moved
 *      (`docs/rules/runtimes.md` sections 8 and 9, `releases.md` section 8).
 *   3. No source's `verified_on` is more than 90 days old (`docs/rules/nexus.md`
 *      section 5).
 *   4. Nothing third-party is bundled into the installer (hard rule 3,
 *      `docs/rules/setup.md`).
 *
 * Check 4 is the reason this file exists at all: hard rule 3 was enforced by
 * review from #16 until there was an installer to inspect, and `AGENTS.md` says
 * so. There is one now.
 *
 * **It is run twice, and that is deliberate.** Before the build, where checks 1
 * to 3 and the configuration half of check 4 can already fail the tag without
 * spending twenty minutes on an installer nobody will ship; and again after the
 * build, with `--bundle`, where the artifact itself is inspected. A run with no
 * installer says so rather than passing quietly, and `--require-bundle` turns
 * that into a failure for the second run.
 *
 * A check whose data does not exist yet reports itself as pending and does not
 * pass. Nexus has no sources file and the manifest has never moved a pin: those
 * are facts about today, not exemptions, and they are printed on every run so
 * that the first release cannot mistake an absent check for a green one.
 *
 * No dependencies, like `check-rules.mjs`. This runs in a workflow that has a
 * toolchain, but a checker that needs an install is a checker somebody skips.
 *
 * Usage:
 *   node scripts/check-release.mjs
 *   node scripts/check-release.mjs --bundle "src-tauri/target/release/bundle/nsis/Demido Studio_0.1.0_x64-setup.exe"
 *   node scripts/check-release.mjs --tag v0.1.0 --require-bundle
 */

import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = fileURLToPath(new URL('..', import.meta.url))
const CONF = join(ROOT, 'src-tauri', 'tauri.conf.json')
const CARGO = join(ROOT, 'src-tauri', 'Cargo.toml')
const MANIFEST = join(ROOT, 'src-tauri', 'crates', 'demido-catalog', 'src', 'manifest.rs')
const MANIFEST_PATH = 'src-tauri/crates/demido-catalog/src/manifest.rs'

const args = process.argv.slice(2)
const flag = (name, fallback) => {
  const at = args.indexOf(`--${name}`)
  if (at === -1) return fallback
  const value = args[at + 1]
  if (value === undefined || value.startsWith('--')) {
    console.error(`check-release: --${name} needs a value`)
    process.exit(2)
  }
  return value
}
const switched = (name) => args.includes(`--${name}`)

/** The tag being built. CI passes it; a local run works it out or does without. */
const TAG = flag('tag', process.env.RELEASE_TAG ?? currentTag())
/** The installer to inspect, when there is one. */
const BUNDLE = flag('bundle', null)
const REQUIRE_BUNDLE = switched('require-bundle')

/** @type {{rule: string, where: string, message: string}[]} */
const violations = []
/** @type {{name: string, why: string}[]} */
const pending = []
/** @type {string[]} */
const passed = []

const fail = (rule, where, message) =>
  violations.push({ rule, where: relative(ROOT, where).split(sep).join('/') || where, message })

/** Git, or nothing. A clone without history still has to be able to run this. */
function git(...argv) {
  try {
    return execFileSync('git', argv, {
      cwd: ROOT,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim()
  } catch {
    return null
  }
}

/** The tag on HEAD, when HEAD is tagged. A release build is a tag build. */
function currentTag() {
  return git('describe', '--tags', '--exact-match') ?? null
}

/**
 * The release before this one, so a pin can be compared against the pin that
 * last shipped. `null` on the first release, which is not a failure: there is
 * nothing to have moved away from.
 */
function previousTag() {
  const listed = git('tag', '--list', 'v*', '--sort=-v:refname')
  if (!listed) return null
  return (
    listed
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line && line !== TAG)[0] ?? null
  )
}

// --- 1. the version agrees with itself ---------------------------------------
// docs/rules/versioning.md keeps one version in one place and reads it
// everywhere. This is the check that says so at the only moment the tag is a
// third copy of it.

function checkVersion() {
  const conf = JSON.parse(readFileSync(CONF, 'utf8'))
  const declared = conf.version
  const cargo = /^\s*version\s*=\s*"([^"]+)"/m.exec(
    readFileSync(CARGO, 'utf8').split('[workspace.package]')[1] ?? '',
  )

  if (!cargo) {
    fail('versioning', CARGO, 'the workspace has no [workspace.package] version to compare against')
    return
  }

  if (cargo[1] !== declared) {
    fail(
      'versioning',
      CARGO,
      `the workspace is ${cargo[1]} and tauri.conf.json is ${declared}; one version lives in one place`,
    )
  }

  if (!TAG) {
    pending.push({
      name: 'the tag matches the version',
      why: 'HEAD carries no vX.Y.Z tag, so there is no tag to compare. Pass --tag or run this on a tag build.',
    })
    return
  }

  if (TAG !== `v${declared}`) {
    fail(
      'versioning',
      CONF,
      `the tag is ${TAG} and the version is ${declared}; a tag is v<version>`,
    )
    return
  }

  passed.push(`the tag ${TAG} matches tauri.conf.json and the Cargo workspace`)
}

// --- 2. the pin, its predecessor and the line saying why it moved -------------
// docs/rules/runtimes.md section 9 makes the manifest the rollback horizon, and
// section 8 makes a release run against the pin it ships and its predecessor.
// releases.md section 8 requires one line of why whenever the pin moved.

/** `pub const NAME: &str = "value";`, whichever of the names is present. */
function constant(source, ...names) {
  for (const name of names) {
    const found = new RegExp(
      `\\bconst\\s+${name}\\s*:\\s*&(?:'static\\s+)?str\\s*=\\s*"([^"]*)"`,
    ).exec(source)
    if (found) return { name, value: found[1] }
  }
  return null
}

function checkPins() {
  if (!existsSync(MANIFEST)) {
    pending.push({
      name: 'the pin and its predecessor',
      why: `${MANIFEST_PATH} does not exist, so there is no pin to check.`,
    })
    return
  }

  const source = readFileSync(MANIFEST, 'utf8')
  const release = constant(source, 'RELEASE')
  if (!release) {
    fail('runtimes', MANIFEST, 'declares no RELEASE pin')
    return
  }

  const predecessor = constant(source, 'PREDECESSOR', 'PREVIOUS_RELEASE')
  if (predecessor && predecessor.value === release.value) {
    fail(
      'runtimes',
      MANIFEST,
      `${predecessor.name} is ${predecessor.value}, which is the current pin; the predecessor is one pin back, and it is the rollback horizon`,
    )
  }

  const previous = previousTag()
  if (!previous) {
    pending.push({
      name: 'the pin moved, so it carries a note',
      why: 'there is no earlier v* tag to compare the pin against. On the first release there is nothing it can have moved away from.',
    })
    return
  }

  const shipped = git('show', `${previous}:${MANIFEST_PATH}`)
  if (shipped === null) {
    pending.push({
      name: 'the pin moved, so it carries a note',
      why: `${previous} has no ${MANIFEST_PATH}, so the pin cannot be compared against what that release shipped.`,
    })
    return
  }

  const before = constant(shipped, 'RELEASE')
  if (!before || before.value === release.value) {
    passed.push(`the pin is ${release.value}, unmoved since ${previous}`)
    return
  }

  // The pin moved. Both the horizon and the sentence are now owed.
  if (!predecessor) {
    fail(
      'runtimes',
      MANIFEST,
      `the pin moved from ${before.value} to ${release.value} and the manifest names no predecessor; section 9 makes it the rollback horizon, so add a PREDECESSOR const holding ${before.value}`,
    )
  } else if (predecessor.value !== before.value) {
    fail(
      'runtimes',
      MANIFEST,
      `the pin moved from ${before.value} to ${release.value}, but ${predecessor.name} is ${predecessor.value}; the predecessor is the pin the last release shipped`,
    )
  }

  const note = constant(source, 'RELEASE_NOTE', 'NOTE')
  if (!note || !note.value.trim()) {
    fail(
      'releases',
      MANIFEST,
      `the pin moved from ${before.value} to ${release.value} and carries no note; add a RELEASE_NOTE const holding one line saying why it moved, because the person who moved it is the only person who ran it`,
    )
  } else {
    passed.push(`the pin moved to ${release.value} and says why: "${note.value}"`)
  }
}

// --- 3. source rot -----------------------------------------------------------
// docs/rules/nexus.md section 5: a build whose verified_on is more than 90 days
// old fails to release. Deliberately a release gate and not a runtime one, and
// re-argued in releases.md section 9 without changing.

const ROT_DAYS = 90

/** Where a source could declare itself: code and data, never prose. The rule's
 * own files talk about `verified_on` in sentences, and a checker that read
 * those would report the rule as a violation of itself. */
function dataFiles(dir, found = []) {
  if (!existsSync(dir)) return found
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === 'target' || entry.name === 'node_modules') continue
      dataFiles(path, found)
    } else if (/\.(rs|json|toml|ya?ml)$/.test(entry.name)) {
      found.push(path)
    }
  }
  return found
}

function checkRot() {
  const dated = []
  for (const file of dataFiles(join(ROOT, 'src-tauri', 'crates'))) {
    const source = readFileSync(file, 'utf8')
    const declared = /verified_on\b[^0-9]{0,20}(\d{4})-(\d{2})-(\d{2})/g
    let found
    while ((found = declared.exec(source))) {
      dated.push({ file, date: `${found[1]}-${found[2]}-${found[3]}` })
    }
  }

  if (!dated.length) {
    pending.push({
      name: `no source verified more than ${ROT_DAYS} days ago`,
      why: 'nothing declares a verified_on yet. Nexus and its sources are not built, so there is no date to be stale.',
    })
    return
  }

  const now = Date.now()
  let oldest = null
  for (const { file, date } of dated) {
    const age = Math.floor((now - Date.parse(`${date}T00:00:00Z`)) / 86_400_000)
    if (Number.isNaN(age)) {
      fail('nexus', file, `verified_on ${date} is not a date`)
      continue
    }
    if (age > ROT_DAYS) {
      fail(
        'nexus',
        file,
        `verified_on ${date} is ${age} days old; a source is re-verified within ${ROT_DAYS} days or the release waits for it`,
      )
    }
    if (!oldest || age > oldest) oldest = age
  }

  if (oldest !== null) {
    passed.push(`${dated.length} source date(s), the oldest ${oldest} days old`)
  }
}

// --- 4. nothing third-party is bundled ---------------------------------------
// Hard rule 3. Runtimes, inference backends and everything else are fetched
// from upstream onto the user's machine at set-up, so the installer carries
// Demido and nothing else.

/**
 * The names that must not be inside an installer, and what each one would be.
 *
 * Taken from `THIRD_PARTY_NOTICES.md`: everything Demido fetches at set-up, by
 * the name its binary actually carries. A payload big enough to matter cannot
 * hide from this, because it has to be named to be executed.
 */
const FORBIDDEN = [
  ['llama-server', 'the inference backend, fetched at set-up'],
  ['llama-cli', 'the inference backend, fetched at set-up'],
  ['ggml', 'a llama.cpp library, fetched at set-up'],
  ['cudart64', "NVIDIA's CUDA runtime, fetched at set-up"],
  ['cublas', "NVIDIA's CUDA libraries, fetched at set-up"],
  ['.gguf', 'model weights, which belong to the user'],
  ['uv.exe', "astral-sh's uv, fetched at set-up"],
  ['python3', 'a Python runtime, fetched at set-up'],
  ['searxng', 'the search backend, fetched at set-up'],
  ['agent-browser', 'the browser driver, fetched at set-up'],
  ['chrome.exe', 'Chrome for Testing, fetched only where no browser exists'],
]

/**
 * A ceiling the app alone clears by a wide margin and no fetched runtime can.
 *
 * The required set-up group is 818.7 MiB (`docs/rules/setup.md` section 4) and
 * the smallest thing in it is tens of megabytes, so this catches a bundled
 * payload by weight even under a name nobody thought to list. The installer
 * built on #38 was 2.1 MiB.
 */
const MAX_INSTALLER_MIB = 40

/** The installer, if the caller named one or the bundler left one where it does. */
function installer() {
  if (BUNDLE) return existsSync(BUNDLE) ? BUNDLE : null
  const nsis = join(ROOT, 'src-tauri', 'target', 'release', 'bundle', 'nsis')
  if (!existsSync(nsis)) return null
  const built = readdirSync(nsis)
    .filter((name) => name.endsWith('.exe'))
    .map((name) => join(nsis, name))
  return built[0] ?? null
}

/** Every ASCII run of 4 or more, from both encodings a Windows binary uses. */
function strings(bytes) {
  const found = []
  let run = ''
  for (let at = 0; at < bytes.length; at += 1) {
    const byte = bytes[at]
    const printable = byte >= 0x20 && byte < 0x7f
    if (printable) {
      run += String.fromCharCode(byte)
    } else {
      if (run.length >= 4) found.push(run)
      run = ''
    }
  }
  if (run.length >= 4) found.push(run)

  // UTF-16LE, which is what Windows paths and NSIS's own tables are written in.
  // Read as every other byte rather than decoded, because what is wanted is a
  // name, not a correct string.
  for (const offset of [0, 1]) {
    run = ''
    for (let at = offset; at + 1 < bytes.length; at += 2) {
      const byte = bytes[at]
      const printable = byte >= 0x20 && byte < 0x7f && bytes[at + 1] === 0
      if (printable) {
        run += String.fromCharCode(byte)
      } else {
        if (run.length >= 4) found.push(run)
        run = ''
      }
    }
    if (run.length >= 4) found.push(run)
  }
  return found
}

function checkNothingBundled() {
  // The configuration half, which is true before anything is built and is the
  // half that can be fixed cheaply. `externalBin` and `resources` are the two
  // fields that put somebody else's file inside a Tauri bundle.
  const conf = JSON.parse(readFileSync(CONF, 'utf8'))
  const bundle = conf.bundle ?? {}
  for (const field of ['externalBin', 'resources']) {
    const value = bundle[field]
    const empty =
      value === undefined || (Array.isArray(value) ? !value.length : !Object.keys(value).length)
    if (!empty) {
      fail(
        'setup',
        CONF,
        `bundle.${field} carries ${JSON.stringify(value)}; nothing third-party is bundled, it is fetched at set-up (hard rule 3)`,
      )
    }
  }

  // Section 5 of releases.md: an uninstall leaves every profile's data where it
  // is, and the default is not a decision.
  if (bundle.windows?.nsis?.installMode !== 'currentUser') {
    fail(
      'releases',
      CONF,
      'the NSIS install mode is not currentUser; a per-machine install recreates the shared writable directory profiles.md refuses',
    )
  }
  // Section 5 asks for `deleteAppDataOnUninstall: false` explicitly, and Tauri
  // 2's NSIS bundler has no such field: setting it fails the build with
  // "unknown field". So what is checkable is the half that can go wrong, which
  // is somebody adding it back under a name the bundler does accept, or an
  // installer hook that removes a profile's data. The intent of the rule is
  // Tauri 2's own behaviour, and the rule file records that.
  const nsis = bundle.windows?.nsis ?? {}
  if (nsis.deleteAppDataOnUninstall !== undefined && nsis.deleteAppDataOnUninstall !== false) {
    fail(
      'releases',
      CONF,
      'deleteAppDataOnUninstall is set to something other than false; an uninstall leaves the data of every profile where it is',
    )
  }
  if (nsis.installerHooks) {
    fail(
      'releases',
      CONF,
      `bundle.windows.nsis.installerHooks is ${nsis.installerHooks}; a hook is the one place an uninstall could delete a profile, so it is read before it ships`,
    )
  }

  const built = installer()
  if (!built) {
    const where = BUNDLE ?? 'src-tauri/target/release/bundle/nsis'
    if (REQUIRE_BUNDLE) {
      fail('setup', CONF, `no installer at ${where}, and this run was told to require one`)
    } else {
      pending.push({
        name: 'the installer carries nothing third-party',
        why: `no installer at ${where}. Run \`pnpm build\`, then this again with --bundle.`,
      })
    }
    return
  }

  const size = statSync(built).size
  const mib = size / 1024 / 1024
  if (mib > MAX_INSTALLER_MIB) {
    fail(
      'setup',
      built,
      `the installer is ${mib.toFixed(1)} MiB, over the ${MAX_INSTALLER_MIB} MiB ceiling; the app alone is a few MiB, so this is carrying something that should be fetched at set-up`,
    )
  }

  const inside = strings(readFileSync(built))
  for (const [name, what] of FORBIDDEN) {
    const needle = name.toLowerCase()
    const hit = inside.find((text) => text.toLowerCase().includes(needle))
    if (hit) {
      fail(
        'setup',
        built,
        `names "${hit.trim().slice(0, 60)}", which is ${what}; nothing third-party is bundled (hard rule 3)`,
      )
    }
  }

  if (!violations.some((violation) => violation.rule === 'setup')) {
    passed.push(
      `the installer is ${mib.toFixed(1)} MiB and names nothing that is fetched at set-up`,
    )
  }
}

// --- run ---------------------------------------------------------------------

checkVersion()
checkPins()
checkRot()
checkNothingBundled()

for (const line of passed) console.log(`  ok   ${line}`)
for (const { name, why } of pending) console.log(`  wait ${name}: ${why}`)

if (violations.length) {
  console.log('')
  for (const { rule, where, message } of violations) {
    console.error(`  ${where}  ${message}  [${rule}]`)
  }
  console.error(`\n${violations.length} violation(s); this tag does not ship`)
  process.exit(1)
}

console.log(
  `\n✓ release checks clean${pending.length ? `, ${pending.length} pending on data that does not exist yet` : ''}`,
)
