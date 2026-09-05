/**
 * Enforces the machine-checkable rules in docs/rules/.
 *
 * A rule an agent can violate without CI noticing is a rule that will be
 * violated, so every rule in docs/rules/ that *can* be checked is checked here.
 * Each check is independent and reports every violation it finds rather than
 * stopping at the first, so one run tells you everything.
 *
 * The shape is carried over from v2, which ran nine checks. This file holds the
 * two that wayfinder ticket #9 owns, the brief checks from ticket #12, and the
 * repo hygiene checks from ticket #16: attribution, provenance, em dashes,
 * decision references and crate docs.
 *
 * Two of these read git rather than files. See docs/rules/attribution.md for
 * which commits a run is responsible for and how CI passes the range.
 *
 * No dependencies, on purpose: the stack is decided (ticket #10) but nothing is
 * scaffolded, and this has to run before there is a toolchain to run it with.
 *
 * Usage: node scripts/check-rules.mjs
 */

import { readFileSync, readdirSync, existsSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = fileURLToPath(new URL('..', import.meta.url))
const TOKENS = join(ROOT, 'design', 'tokens.css')

/** @type {{rule: string, file: string, line?: number, message: string}[]} */
const violations = []

function fail(rule, file, message, line) {
  violations.push({ rule, file: relative(ROOT, file).split(sep).join('/'), line, message })
}

/**
 * Split text into lines on either ending.
 *
 * `.gitattributes` checks text out native, so every file this reads is CRLF on
 * a Windows clone. Splitting on a bare newline leaves a carriage return at the
 * end of each line, and any regex anchored with `$` then matches nothing: the
 * ledger's `## Amendments` heading stopped being seen, so all seven amendment
 * rows were read as duplicates of the ledger rows they amend. Windows is the
 * first platform, so the checker reads files as they are actually checked out.
 */
function lines(text) {
  return text.split(/\r?\n/)
}

function walk(dir, predicate) {
  const out = []
  if (!existsSync(dir)) return out
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) {
      if (['node_modules', 'target', 'dist', '.git', 'gen', 'graphify-out'].includes(entry.name)) continue
      out.push(...walk(full, predicate))
    } else if (predicate(entry.name)) {
      out.push(full)
    }
  }
  return out
}

/**
 * Source the value rules apply to. Written as a list of candidate roots rather
 * than one, because ticket #10 has not chosen where the frontend lives yet and
 * a missing root is not an error.
 */
function sourceFiles() {
  const roots = ['web/src', 'src', 'app/src', 'src-tauri']
  const files = []
  for (const root of roots) {
    files.push(...walk(join(ROOT, root), (n) => /\.(css|scss|ts|tsx|js|jsx|html|svelte|vue)$/.test(n)))
  }
  return files
}

// --- docs/rules/no-raw-values.md --------------------------------------------
// Arbitrary values belong in one file so a repaint is one diff, not a hunt.
// v2 enforced this for colour and it held perfectly; it did not enforce type
// and the same codebase grew 21 font sizes. That is the whole argument.

const FAMILIES = [
  {
    name: 'colour',
    // A property is not needed: a colour literal is a colour literal wherever
    // it appears, including inside a var() fallback chain.
    test: (code) => /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?|oklch|lab)\s*\(/.exec(code),
    use: '--color-, --cap-, --src- or --brand-',
  },
  {
    name: 'type',
    test: (code) =>
      /\bfont-size\s*:\s*(?!0\b)[^;}]*?[\d.]+(?:px|rem|em)/.exec(code) ||
      /\bletter-spacing\s*:\s*(?!0\b|normal\b)[^;}]*?[-\d.]+(?:em|px|rem)?/.exec(code) ||
      /\bline-height\s*:\s*(?!0\b|normal\b|inherit\b)[^;}]*?[\d.]+/.exec(code),
    use: '--text-, --leading- or --tracking-',
  },
  {
    name: 'space',
    test: (code) =>
      /\b(?:padding|margin|gap|row-gap|column-gap|inset)(?:-(?:top|right|bottom|left|inline|block|start|end|x|y))?\s*:\s*[^;}]*?(?<![\w.-])(?!0(?![\d.]))[\d.]+(?:px|rem)/.exec(
        code,
      ),
    use: '--space-',
  },
  {
    name: 'radius',
    test: (code) =>
      /\bborder(?:-(?:top|bottom)-(?:left|right))?-radius\s*:\s*[^;}]*?(?<![\w.-])(?!0(?![\d.]))[\d.]+(?:px|rem|%)/.exec(
        code,
      ),
    use: '--radius-',
  },
  {
    name: 'motion',
    test: (code) =>
      /\b(?:transition|animation)(?:-(?:duration|delay))?\s*:\s*[^;}]*?(?<![\w.-])(?!0(?![\d.]))[\d.]+m?s\b/.exec(code),
    use: '--duration-, --delay- or --ease-',
  },
]

function checkNoRawValues() {
  for (const file of sourceFiles()) {
    const source = readFileSync(file, 'utf8')

    // A documented exception says so at the top of the file, in the first 400
    // characters, and names its reason. See docs/rules/no-raw-values.md.
    const exempt = /@raw-values-exempt:\s*(\S.*)/.exec(source.slice(0, 400))
    if (exempt) continue

    lines(source).forEach((text, i) => {
      // Only a comment is exempt line by line, because explaining a token needs
      // to be able to write its value down.
      //
      // A plain var(--token) reference is then blanked, because the leading `--`
      // is itself a run of dashes and digits-adjacent characters and trips the
      // type family. A var() with a FALLBACK is deliberately left intact: a
      // colour in a fallback chain is still a raw colour, which is v2's own
      // rule and the reason this is a narrow pattern rather than /var\(.*\)/.
      const code = text.replace(/\/\*.*?\*\/|\/\/.*$/g, '').replace(/var\(\s*--[\w-]+\s*\)/g, 'T')
      for (const family of FAMILIES) {
        const hit = family.test(code)
        if (hit) {
          fail(
            'no-raw-values',
            file,
            `raw ${family.name} value "${hit[0].trim()}", use a ${family.use} token`,
            i + 1,
          )
        }
      }
    })
  }
}

// --- docs/rules/no-raw-values.md, the contrast half --------------------------
// A theme is 24 colours. Whether those 24 are legible is arithmetic, not
// opinion, so it is computed here rather than claimed in a comment.

/** #rgb / #rrggbb / rgb() / rgb(r g b / a) into [r, g, b] 0-255, alpha composited on `over`. */
function parse(value, over) {
  const v = value.trim()
  let rgb = null
  let alpha = 1

  const hex = /^#([0-9a-fA-F]{3,8})$/.exec(v)
  if (hex) {
    let h = hex[1]
    if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join('')
    rgb = [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)]
    if (h.length === 8) alpha = parseInt(h.slice(6, 8), 16) / 255
  } else {
    const fn = /^rgba?\(([^)]+)\)$/.exec(v)
    if (!fn) return null
    const parts = fn[1].split(/[\s,/]+/).filter(Boolean).map(Number)
    if (parts.length < 3 || parts.some(Number.isNaN)) return null
    rgb = parts.slice(0, 3)
    if (parts.length > 3) alpha = parts[3]
  }

  if (alpha < 1 && over) rgb = rgb.map((c, i) => c * alpha + over[i] * (1 - alpha))
  return rgb
}

const toLinear = (c) => {
  const s = c / 255
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
}

const luminance = ([r, g, b]) => 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b)

function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

/**
 * Vienot, Brettel and Mollon 1999: simulate a dichromat by reconstructing the
 * missing cone response from the two that remain, in LMS.
 *
 * Both red and green dichromacies are simulated, because that is the failure
 * class, not one condition: a pair that separates for a deuteranope and
 * collapses for a protanope has not passed.
 */
function dichromat(rgb, kind) {
  const [r, g, b] = rgb.map(toLinear)
  let l = 17.8824 * r + 43.5161 * g + 4.11935 * b
  let m = 3.45565 * r + 27.1554 * g + 3.86714 * b
  const s = 0.0299566 * r + 0.184309 * g + 1.46709 * b

  if (kind === 'deutan') m = 0.494207 * l + 1.24827 * s
  else l = 2.02344 * m - 2.52581 * s

  const rr = 0.080944 * l - 0.130504 * m + 0.116721 * s
  const gg = -0.0102485 * l + 0.0540194 * m - 0.113615 * s
  const bb = -0.000365294 * l - 0.00412163 * m + 0.693513 * s

  const toSrgb = (c) => {
    const v = Math.max(0, Math.min(1, c))
    return 255 * (v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055)
  }
  return [toSrgb(rr), toSrgb(gg), toSrgb(bb)]
}

/** CIE Lab, D65. */
function lab(rgb) {
  const [r, g, b] = rgb.map(toLinear)
  const x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047
  const y = 0.2126 * r + 0.7152 * g + 0.0722 * b
  const z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883
  // Only the linear branch carries the 116 divisor. Dividing both shrinks every
  // delta-E by about two orders of magnitude, which reads as "every pair of
  // colours is identical" and passes nothing.
  const f = (t) => (t > 216 / 24389 ? Math.cbrt(t) : ((24389 / 27) * t + 16) / 116)
  const [fx, fy, fz] = [f(x), f(y), f(z)]
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)]
}

/**
 * CIEDE2000. Chosen over the plain Euclidean CIE76 because CIE76 badly
 * overstates separation for saturated pairs: it scores this palette's
 * green/violet at 69 and its rejected green/rose at 19, which puts a floor of
 * 8 nowhere useful. CIEDE2000 scores them 46.5 and 6.1, so the floor of 8
 * recorded on #8 lands cleanly between the pair that passed and the pair that
 * failed.
 *
 * Note for anyone comparing numbers: the dataviz skill's own
 * `validate_palette.js`, which produced the values written into #8 and into
 * `tokens.css`, reports 17.0 and 4.6 for those same two pairs against the same
 * floor of 8. The absolute scale differs because the formula does. The verdict
 * does not: rose against green fails, violet against green passes, and it
 * passes here by a factor of five.
 */
const rad = (d) => (d * Math.PI) / 180
const deg = (r) => (r * 180) / Math.PI

function deltaE(rgb1, rgb2) {
  const [L1, a1, b1] = lab(rgb1)
  const [L2, a2, b2] = lab(rgb2)
  const C1 = Math.hypot(a1, b1)
  const C2 = Math.hypot(a2, b2)
  const Cb = (C1 + C2) / 2
  const G = 0.5 * (1 - Math.sqrt(Cb ** 7 / (Cb ** 7 + 25 ** 7)))
  const A1 = (1 + G) * a1
  const A2 = (1 + G) * a2
  const Cp1 = Math.hypot(A1, b1)
  const Cp2 = Math.hypot(A2, b2)

  const hue = (a, b) => {
    if (a === 0 && b === 0) return 0
    const t = deg(Math.atan2(b, a))
    return t < 0 ? t + 360 : t
  }
  const h1 = hue(A1, b1)
  const h2 = hue(A2, b2)

  const dL = L2 - L1
  const dC = Cp2 - Cp1
  let dh = 0
  if (Cp1 * Cp2 !== 0) {
    dh = h2 - h1
    if (dh > 180) dh -= 360
    else if (dh < -180) dh += 360
  }
  const dH = 2 * Math.sqrt(Cp1 * Cp2) * Math.sin(rad(dh) / 2)

  const Lb = (L1 + L2) / 2
  const Cpb = (Cp1 + Cp2) / 2
  let hb
  if (Cp1 * Cp2 === 0) {
    hb = h1 + h2
  } else {
    hb = Math.abs(h1 - h2) > 180 ? (h1 + h2 + 360) / 2 : (h1 + h2) / 2
    if (hb >= 360) hb -= 360
  }

  const T =
    1 -
    0.17 * Math.cos(rad(hb - 30)) +
    0.24 * Math.cos(rad(2 * hb)) +
    0.32 * Math.cos(rad(3 * hb + 6)) -
    0.2 * Math.cos(rad(4 * hb - 63))
  const dTheta = 30 * Math.exp(-(((hb - 275) / 25) ** 2))
  const Rc = 2 * Math.sqrt(Cpb ** 7 / (Cpb ** 7 + 25 ** 7))
  const Sl = 1 + (0.015 * (Lb - 50) ** 2) / Math.sqrt(20 + (Lb - 50) ** 2)
  const Sc = 1 + 0.045 * Cpb
  const Sh = 1 + 0.015 * Cpb * T
  const Rt = -Math.sin(rad(2 * dTheta)) * Rc

  return Math.sqrt((dL / Sl) ** 2 + (dC / Sc) ** 2 + (dH / Sh) ** 2 + Rt * (dC / Sc) * (dH / Sh))
}

/** Parse tokens.css into { themeName: { '--color-x': 'value' } }. A theme block
 * is any rule that sets `color-scheme`, which is the one declaration every
 * theme must carry and nothing else does. */
function readThemes() {
  const css = readFileSync(TOKENS, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
  const themes = {}
  for (const block of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const [, selector, body] = block
    if (!/color-scheme\s*:/.test(body)) continue
    const named = /\[data-theme=['"]?([\w-]+)['"]?\]/.exec(selector)
    const name = named ? named[1] : 'default'
    const values = {}
    for (const decl of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) values[decl[1]] = decl[2].trim()
    themes[name] = values
  }
  return themes
}

const SURFACES = ['void', 'well', 'rack', 'panel', 'raised', 'chrome', 'hover', 'active']
// ink-4 is decorative and disabled only. Its floor is "visible at all", not
// "readable": WCAG exempts disabled text entirely, and Karl sets it deliberately
// below body-text legibility so that using it for a sentence is a visible bug.
// See docs/rules/no-raw-values.md.
const READABLE = ['ink', 'ink-2', 'ink-3', 'signal', 'amber', 'sky', 'violet', 'rose']
const FLOOR_READABLE = 4.5
const FLOOR_DECORATIVE = 2.5
const FLOOR_CVD = 8.0

function checkThemeContrast() {
  const themes = readThemes()
  if (Object.keys(themes).length === 0) {
    fail('theme-contrast', TOKENS, 'no theme block found (a theme block sets color-scheme)')
    return
  }

  for (const [name, values] of Object.entries(themes)) {
    const colour = (role) => parse(values[`--color-${role}`] ?? '', null)

    const surfaces = SURFACES.map((role) => [role, colour(role)]).filter(([, rgb]) => rgb)
    if (surfaces.length !== SURFACES.length) {
      fail('theme-contrast', TOKENS, `theme "${name}" is missing one of the eight surfaces`)
      continue
    }

    // The worst case is the lightest surface text ever sits on. That is not
    // hardcoded: it is whichever of the eight measures lightest, so a theme
    // that reorders the ramp is still measured against its own worst case.
    const [worstRole, worstRgb] = surfaces.reduce((a, b) => (luminance(a[1]) > luminance(b[1]) ? a : b))

    for (const role of READABLE) {
      const rgb = colour(role)
      if (!rgb) {
        fail('theme-contrast', TOKENS, `theme "${name}" has no --color-${role}`)
        continue
      }
      const ratio = contrast(rgb, worstRgb)
      if (ratio < FLOOR_READABLE) {
        fail(
          'theme-contrast',
          TOKENS,
          `theme "${name}": --color-${role} is ${ratio.toFixed(2)}:1 on --color-${worstRole}, floor is ${FLOOR_READABLE}`,
        )
      }
    }

    const ink4 = colour('ink-4')
    if (ink4) {
      const ratio = contrast(ink4, worstRgb)
      if (ratio < FLOOR_DECORATIVE) {
        fail(
          'theme-contrast',
          TOKENS,
          `theme "${name}": --color-ink-4 is ${ratio.toFixed(2)}:1 on --color-${worstRole}, floor is ${FLOOR_DECORATIVE}`,
        )
      }
      const ink3 = colour('ink-3')
      if (ink3 && contrast(ink4, worstRgb) >= contrast(ink3, worstRgb)) {
        fail('theme-contrast', TOKENS, `theme "${name}": --color-ink-4 is not dimmer than --color-ink-3`)
      }
    }

    // rise against fall, as a red or green dichromat sees them. A property of
    // the PAIR: a theme can redefine either one and silently walk back into the
    // failure that #8 measured its way out of, and nobody would notice, because
    // both candles still look fine to a trichromat.
    const rise = colour('rise')
    const fall = colour('fall')
    if (rise && fall) {
      for (const kind of ['deutan', 'protan']) {
        const separation = deltaE(dichromat(rise, kind), dichromat(fall, kind))
        if (separation < FLOOR_CVD) {
          fail(
            'theme-contrast',
            TOKENS,
            `theme "${name}": --color-rise and --color-fall separate by only ${separation.toFixed(1)} for a ${kind}ope, floor is ${FLOOR_CVD}`,
          )
        }
      }
    }
  }
}

// --- docs/rules/brief.md -----------------------------------------------------
// The brief is 196 lines and it is the thing being built. v2 drifted from it by
// summarising it and then building from the summary, which was found only by a
// dedicated audit milestone re-reading the original item by item. These checks
// make that audit continuous: a requirement cannot go unlisted, and a citation
// cannot be a paraphrase, because both are matched against brief.md itself.
//
// Everything here is offline and structural, on purpose. See the rule doc for
// what this deliberately does NOT check.

const BRIEF = join(ROOT, 'docs', 'brief.md')
const LEDGER = join(ROOT, 'docs', 'brief-map.md')

/** Whitespace is not meaningful in a quote that has been wrapped by an editor,
 * so both sides are squashed before matching. Nothing else is normalised: a
 * changed word is a changed quote. */
const squash = (s) => s.replace(/\s+/g, ' ').trim()

/** Rows of docs/brief-map.md, split by the section they sit in. A row is
 * `| B01 | "anchor" | ... |`; the section is the last `## ` heading above it. */
function readLedger() {
  const rows = new Map()
  const amendments = []
  if (!existsSync(LEDGER)) {
    fail('brief', LEDGER, 'the ledger is missing; every brief requirement is meant to have a row here')
    return { rows, amendments }
  }
  let section = ''
  lines(readFileSync(LEDGER, 'utf8'))
    .forEach((text, i) => {
      const heading = /^##\s+(.*)$/.exec(text)
      if (heading) section = heading[1].toLowerCase()
      const row = /^\|\s*~?~?(B\d+)~?~?\s*\|\s*(.*?)\s*\|/.exec(text)
      if (!row) return
      const [, id, cell] = row
      const quoted = /^"(.*)"$/.exec(cell)
      if (!quoted) {
        fail('brief', LEDGER, `${id}: the second cell must be a quoted fragment of the brief`, i + 1)
        return
      }
      const entry = { id, quote: quoted[1], line: i + 1 }
      if (section.startsWith('amendment')) {
        amendments.push(entry)
      } else if (rows.has(id)) {
        fail('brief', LEDGER, `${id} is used twice; an id is assigned once and never reused`, i + 1)
      } else {
        rows.set(id, entry)
      }
    })
  return { rows, amendments }
}

function checkBrief() {
  if (!existsSync(BRIEF)) {
    fail('brief', BRIEF, 'the brief is missing; it is the one file this project is built from')
    return
  }
  const brief = readFileSync(BRIEF, 'utf8')
  const briefFlat = squash(brief)
  const { rows, amendments } = readLedger()

  // 1. Every anchor is verbatim. A row that has drifted from the line it claims
  //    to track is worse than no row: it says the requirement is accounted for.
  for (const row of rows.values()) {
    if (!briefFlat.includes(squash(row.quote))) {
      fail('brief', LEDGER, `${row.id}: "${row.quote}" is not in docs/brief.md verbatim`, row.line)
    }
  }

  // 2. Every amendment names a row that exists, and still quotes the brief.
  for (const a of amendments) {
    if (!rows.has(a.id)) {
      fail('brief', LEDGER, `amendment ${a.id} has no row in the ledger`, a.line)
    }
    if (!briefFlat.includes(squash(a.quote))) {
      fail('brief', LEDGER, `amendment ${a.id}: "${a.quote}" is not in docs/brief.md verbatim`, a.line)
    }
  }

  // 3. Every bullet of the brief is covered by at least one anchor. This is the
  //    check that catches a requirement nobody has looked at: the brief grows a
  //    line, and CI says so on the next commit rather than in an audit a
  //    milestone later.
  const anchors = [...rows.values()].map((r) => squash(r.quote))
  lines(brief).forEach((text, i) => {
    const bullet = /^\s*-\s+(\S.*)$/.exec(text)
    if (!bullet) return
    const line = squash(bullet[1])
    if (!anchors.some((a) => line.includes(a))) {
      fail('brief', BRIEF, `no row in docs/brief-map.md covers this requirement`, i + 1)
    }
  })

  // 4. Citations resolve, and quote rather than paraphrase. `Brief <id>:` is
  //    followed by the quote on the same line, or by a blockquote under it.
  //    `Brief: silent` is the only other legal form, and it is deliberately
  //    cheap to write and expensive to write dishonestly.
  const prose = [
    ...walk(join(ROOT, 'docs'), (n) => n.endsWith('.md')),
    ...walk(join(ROOT, 'design'), (n) => n.endsWith('.md')),
    ...['AGENTS.md', 'README.md'].map((n) => join(ROOT, n)).filter((p) => existsSync(p)),
  ]
  for (const file of prose) {
    if (file === LEDGER || file === BRIEF) continue
    const cited = lines(readFileSync(file, 'utf8'))
    cited.forEach((text, i) => {
      for (const hit of text.matchAll(/\bBrief (B\d+)(:?)/g)) {
        const [, id, colon] = hit
        if (!rows.has(id)) {
          fail('brief', file, `cites ${id}, which has no row in docs/brief-map.md`, i + 1)
          continue
        }
        if (!colon) continue
        const rest = text.slice(hit.index + hit[0].length)
        const inline = /"([^"]+)"/.exec(rest)
        let quote = inline ? inline[1] : null
        if (!quote) {
          // Long form: the tag line stands alone and the quote is the
          // blockquote under it, blank lines allowed between.
          const block = []
          for (let j = i + 1; j < cited.length; j++) {
            const next = cited[j].trim()
            if (next === '' && block.length === 0) continue
            if (!next.startsWith('>')) break
            block.push(next.replace(/^>\s?/, ''))
          }
          if (block.length) quote = block.join(' ')
        }
        if (!quote) {
          fail('brief', file, `${id}: a citation quotes the brief, on this line or as a blockquote under it`, i + 1)
        } else if (!briefFlat.includes(squash(quote))) {
          fail('brief', file, `${id}: "${quote}" is not in docs/brief.md verbatim`, i + 1)
        }
      }
    })
  }
}

// --- docs/rules/attribution.md -----------------------------------------------
// One author, every commit signed, and no assistant named in repo metadata.
// All three are true by hand today, which is precisely how long an unenforced
// rule stays true. The .githooks/commit-msg hook catches a violation before the
// commit exists; this catches it in CI, where no hook is installed.

const GIT = join(ROOT, '.git')
const EM_DASH = '\u2014'

const AUTHOR = 'Stefan Cucoranu <elpideus@gmail.com>'

/**
 * Phrases that name an assistant as an author or a co-author. Deliberately
 * narrow, because this repo has to be able to *talk* about `openclaude`,
 * `open-webui`, `gemini-cli` and its own `.claude` directory: those names are
 * blanked before matching, so naming a project or a path is free and being
 * credited to an assistant is not.
 */
const ATTRIBUTION_LEAKS = [
  [/^\s*co-authored-by:/im, 'a Co-Authored-By trailer'],
  [/\bgenerated with\b/i, 'a "generated with" line'],
  [/\u{1F916}/u, 'a robot emoji'],
  [/\b(claude|anthropic|chatgpt|copilot|codex|devin|cursor)\b/i, 'the name of an assistant'],
  [/\bai[- ](generated|assisted|written)\b/i, 'an AI authorship claim'],
]

const PROJECT_NAMES = /openclaude|open-webui|gemini-cli|claude-code|[.]claude/gi

function gitOut(args) {
  try {
    return execFileSync('git', args, { cwd: ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
  } catch {
    return null
  }
}

const UNIT = '\u001f'
const RECORD = '\u001e'

/**
 * The commits this run is responsible for. CI passes the push or pull-request
 * range in RULES_RANGE; a local run checks whatever is not yet on origin/main,
 * which is the same set the hook already saw. A repo with no origin falls back
 * to the tip commit, so a fresh clone still checks something.
 */
function commitsUnderReview() {
  const explicit = (process.env.RULES_RANGE || '').trim()
  const format = `--format=%H${UNIT}%an <%ae>${UNIT}%cn <%ce>${UNIT}%G?${UNIT}%B${RECORD}`
  const attempts = []
  if (explicit) attempts.push([format, explicit])
  else if (gitOut(['rev-parse', '--verify', '--quiet', 'origin/main'])) attempts.push([format, 'origin/main..HEAD'])
  attempts.push([format, '-n', '1', 'HEAD'])

  for (const args of attempts) {
    const out = gitOut(['log', ...args])
    if (out === null) continue
    return out
      .split(RECORD)
      .map((record) => record.trim())
      .filter(Boolean)
      .map((record) => {
        const [hash, author, committer, signature, message] = record.split(UNIT)
        return { hash, author, committer, signature, message: message ?? '' }
      })
  }
  return []
}

function checkAttribution() {
  if (!gitOut(['rev-parse', '--git-dir'])) return

  // Signature verification needs the public key. CI points git at
  // .github/allowed_signers, and where it is configured a *good* signature is
  // demanded rather than merely a present one.
  const signersFile = (gitOut(['config', 'gpg.ssh.allowedSignersFile']) || '').trim()
  const verifiable = signersFile.length > 0

  for (const commit of commitsUnderReview()) {
    const at = `commit ${commit.hash.slice(0, 8)}`
    if (commit.author !== AUTHOR) {
      fail('attribution', GIT, `${at} is authored by ${commit.author}, not ${AUTHOR}`)
    }
    if (commit.committer !== AUTHOR) {
      fail('attribution', GIT, `${at} is committed by ${commit.committer}, not ${AUTHOR}`)
    }
    if (commit.signature === 'N') {
      fail('attribution', GIT, `${at} is not signed`)
    } else if ('BRXY'.includes(commit.signature)) {
      fail('attribution', GIT, `${at} has a bad, revoked or expired signature (git reports "${commit.signature}")`)
    } else if (verifiable && commit.signature !== 'G') {
      fail('attribution', GIT, `${at} is signed by a key that is not in ${signersFile} (git reports "${commit.signature}")`)
    }

    const message = commit.message.replace(PROJECT_NAMES, 'a-project')
    for (const [pattern, what] of ATTRIBUTION_LEAKS) {
      if (pattern.test(message)) {
        fail('attribution', GIT, `${at} carries ${what} in its message`)
      }
    }
    if (commit.message.includes(EM_DASH)) {
      fail('text', GIT, `${at} has an em dash in its message; use a colon, a comma, parentheses, a semicolon or a full stop`)
    }
  }
}

// --- docs/rules/provenance.md ------------------------------------------------
// Ported code says where it came from, and where it came from has its license
// on disk. The brief asks for both halves in one sentence, and the second half
// is the one that rots: the header gets written, the license folder does not,
// and a GPL obligation becomes a claim nobody can check.

const LICENSES = join(ROOT, 'licenses')
const NOTICES = join(ROOT, 'THIRD_PARTY_NOTICES.md')

/** `Ported from <owner>/<project> @ <commit>, <license>.` See the rule doc. */
const PROVENANCE = /Ported from ([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+) @ ([A-Za-z0-9._-]{7,40}), ([A-Za-z0-9.+-]+)\./g

/** The floor. THIRD_PARTY_NOTICES.md may add to this list; it cannot shrink it
 * below these two, because both bans are legal rather than editorial. */
const FORBIDDEN_FLOOR = new Map([
  ['open-webui', 'its license carries a branding-retention clause'],
  ['openclaude', 'it is an unauthorized derivative of proprietary code'],
])

function codeFiles() {
  return walk(ROOT, (n) => /\.(rs|ts|tsx|js|jsx|mjs|cjs|css|scss|py|html|svelte|vue|toml)$/.test(n)).filter((file) => {
    const rel = relative(ROOT, file).split(sep).join('/')
    return !rel.startsWith('licenses/') && !rel.startsWith('.research/')
  })
}

/** The two tables of THIRD_PARTY_NOTICES.md: what ships, and what is banned. */
function readNotices() {
  const listed = new Set()
  const chosen = new Set()
  const forbidden = new Map(FORBIDDEN_FLOOR)
  if (!existsSync(NOTICES)) {
    fail('provenance', NOTICES, 'the human-readable index is missing; the in-app credits surface renders from it')
    return { listed, chosen, forbidden }
  }
  let section = ''
  for (const text of lines(readFileSync(NOTICES, 'utf8'))) {
    const heading = /^##\s+(.*)$/.exec(text)
    if (heading) section = heading[1].toLowerCase()
    const row = /^\|\s*([^|]+?)\s*\|\s*([^|]*?)\s*\|/.exec(text)
    if (!row) continue
    const [, first, second] = row
    if (/^:?-+:?$/.test(first) || first === 'Project' || first.startsWith('_(')) continue
    // The index is the table before the first heading. A later section is
    // prose about things that do not ship yet, and claims nothing.
    if (section === '') listed.add(`${second}/${first}`.toLowerCase())
    else if (section.startsWith('ruled out')) forbidden.set(first.toLowerCase(), second || 'it is on the ruled-out list')
    else if (section.startsWith('chosen')) chosen.add(`${second}/${first}`.toLowerCase())
  }
  return { listed, chosen, forbidden }
}

function checkProvenance() {
  const { listed, chosen, forbidden } = readNotices()

  // 1. Every provenance header resolves to a license on disk, is not a banned
  //    source, and is credited in the index the application renders.
  for (const file of codeFiles()) {
    const source = readFileSync(file, 'utf8')
    for (const hit of source.matchAll(PROVENANCE)) {
      const [, owner, project, , license] = hit
      const line = source.slice(0, hit.index).split('\n').length
      const banned = forbidden.get(project.toLowerCase())
      if (banned) {
        fail('provenance', file, `ported from ${project}, which cannot be copied from: ${banned}`, line)
        continue
      }
      if (!existsSync(join(LICENSES, owner, project, 'LICENSE'))) {
        fail('provenance', file, `names ${owner}/${project} (${license}) with no licenses/${owner}/${project}/LICENSE on disk`, line)
      }
      if (!listed.has(`${owner}/${project}`.toLowerCase())) {
        fail('provenance', file, `names ${owner}/${project} with no row in THIRD_PARTY_NOTICES.md`, line)
      }
    }
  }

  // 2. The licenses tree and the index agree in both directions. A license
  //    nobody credits is as broken as a credit with no license.
  if (existsSync(LICENSES)) {
    for (const owner of readdirSync(LICENSES, { withFileTypes: true })) {
      if (!owner.isDirectory()) continue
      for (const project of readdirSync(join(LICENSES, owner.name), { withFileTypes: true })) {
        if (!project.isDirectory()) continue
        const dir = join(LICENSES, owner.name, project.name)
        const key = `${owner.name}/${project.name}`.toLowerCase()
        if (!existsSync(join(dir, 'LICENSE'))) fail('provenance', dir, 'has no LICENSE file')
        if (forbidden.has(project.name.toLowerCase())) {
          fail('provenance', dir, `${project.name} is on the ruled-out list and must not ship`)
        }
        // A license may land ahead of the code that fetches it when there is
        // no upstream text to unpack later, so a row under "Chosen, not yet
        // installed" is enough to justify one on disk. Credited nowhere is
        // still a failure. #28.
        if (!listed.has(key) && !chosen.has(key)) {
          fail('provenance', NOTICES, `licenses/${key} has no row in the index`)
        }
      }
    }
  }
  for (const key of listed) {
    if (!existsSync(join(LICENSES, ...key.split('/'), 'LICENSE'))) {
      fail('provenance', NOTICES, `${key} is credited with no licenses/${key}/LICENSE on disk`)
    }
  }

  // 3. The two legal bans survive an edit of the index.
  for (const [project, why] of FORBIDDEN_FLOOR) {
    if (!forbidden.has(project)) {
      fail('provenance', NOTICES, `the ruled-out table no longer lists ${project}, which is banned because ${why}`)
    }
  }
}

// --- the em dash, AGENTS.md rule 6 -------------------------------------------
// The one pure style rule here, and the cheapest possible demonstration of the
// asymmetry this file exists for: a rule with a checker holds, a rule without
// one does not.

function checkText() {
  const files = [
    ...walk(join(ROOT, 'docs'), (n) => /\.(md|css|mjs|js|json|yml|yaml)$/.test(n)),
    ...walk(join(ROOT, 'design'), (n) => /\.(md|css|svg)$/.test(n)),
    ...walk(join(ROOT, 'scripts'), (n) => /\.(mjs|js|sh)$/.test(n)),
    ...walk(join(ROOT, '.github'), (n) => /\.(yml|yaml|md)$/.test(n)),
    ...walk(join(ROOT, '.githooks'), () => true),
    ...codeFiles(),
    ...['AGENTS.md', 'README.md', 'THIRD_PARTY_NOTICES.md'].map((n) => join(ROOT, n)).filter((p) => existsSync(p)),
  ]
  for (const file of new Set(files)) {
    // The brief is reproduced verbatim and is never edited, and a third-party
    // license is not ours to rewrite. Both are excluded rather than exempted.
    const rel = relative(ROOT, file).split(sep).join('/')
    if (rel === 'docs/brief.md' || rel.startsWith('licenses/')) continue
    readFileSync(file, 'utf8')
      .split('\n')
      .forEach((text, i) => {
        if (text.includes(EM_DASH)) {
          fail('text', file, 'em dash; use a colon, a comma, parentheses, a semicolon or a full stop', i + 1)
        }
      })
  }
}

// --- docs/decisions/README.md ------------------------------------------------
// A dangling decision reference tells the reader an explanation exists when it
// does not. A reference to a superseded note is worse: it hands them reasoning
// that has since been overturned.

function checkDecisions() {
  const notes = join(ROOT, 'docs', 'decisions')
  const status = new Map()
  for (const file of walk(notes, (n) => /^\d{4}-[a-z0-9-]+\.md$/.test(n))) {
    const name = relative(notes, file).split(sep).join('/')
    const found = /^Status:\s*(.+)$/m.exec(readFileSync(file, 'utf8'))
    if (!found) fail('decisions', file, 'has no Status line (accepted, or superseded-by NNNN)')
    status.set(name, (found ? found[1] : '').trim())
  }

  const prose = [
    ...walk(join(ROOT, 'docs'), (n) => n.endsWith('.md')),
    ...walk(join(ROOT, 'design'), (n) => n.endsWith('.md')),
    ...codeFiles(),
    ...['AGENTS.md', 'README.md'].map((n) => join(ROOT, n)).filter((p) => existsSync(p)),
  ]
  for (const file of new Set(prose)) {
    readFileSync(file, 'utf8')
      .split('\n')
      .forEach((text, i) => {
        for (const hit of text.matchAll(/docs\/decisions\/(\d{4}-[a-z0-9-]+\.md)/g)) {
          const name = hit[1]
          if (!status.has(name)) {
            fail('decisions', file, `references docs/decisions/${name}, which does not exist`, i + 1)
          } else if (/^superseded-by/i.test(status.get(name))) {
            fail('decisions', file, `references docs/decisions/${name}, which is ${status.get(name)}`, i + 1)
          }
        }
      })
  }
}

// --- AGENTS.md, rule 9 -------------------------------------------------------
// Every crate documents itself, so a session that opens one crate learns what
// it is without reading the whole tree. A no-op until the scaffold on #10.

function checkCrateDocs() {
  const crates = join(ROOT, 'src-tauri', 'crates')
  if (!existsSync(crates)) return
  for (const entry of readdirSync(crates, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue
    if (!existsSync(join(crates, entry.name, 'AGENTS.md'))) {
      fail('crate-docs', join(crates, entry.name), 'has no AGENTS.md; every crate documents itself')
    }
  }
}

// --- report ------------------------------------------------------------------

/** `--report` prints the measurements instead of only the failures. Useful when
 * adding a theme, and it is how the numbers in tokens.css were checked. */
function report() {
  for (const [name, values] of Object.entries(readThemes())) {
    const colour = (role) => parse(values[`--color-${role}`] ?? '', null)
    const surfaces = SURFACES.map((r) => [r, colour(r)]).filter(([, c]) => c)
    const [worstRole, worstRgb] = surfaces.reduce((a, b) => (luminance(a[1]) > luminance(b[1]) ? a : b))
    console.log(`\ntheme "${name}", measured against --color-${worstRole} (the lightest surface):`)
    for (const role of [...READABLE, 'ink-4']) {
      const rgb = colour(role)
      if (rgb) console.log(`  --color-${role.padEnd(10)} ${contrast(rgb, worstRgb).toFixed(2)}:1`)
    }
    const rise = colour('rise')
    const fall = colour('fall')
    if (rise && fall) {
      for (const kind of ['deutan', 'protan']) {
        console.log(`  rise/fall ${kind} separation   ${deltaE(dichromat(rise, kind), dichromat(fall, kind)).toFixed(1)}`)
      }
    }
  }
}

// --- run ---------------------------------------------------------------------

const CHECKS = [
  checkNoRawValues,
  checkThemeContrast,
  checkBrief,
  checkAttribution,
  checkProvenance,
  checkText,
  checkDecisions,
  checkCrateDocs,
]

if (process.argv.includes('--report')) {
  report()
  process.exit(0)
}

for (const check of CHECKS) check()

if (violations.length === 0) {
  console.log(`✓ ${CHECKS.length} rules clean`)
  process.exit(0)
}

const byRule = new Map()
for (const v of violations) {
  if (!byRule.has(v.rule)) byRule.set(v.rule, [])
  byRule.get(v.rule).push(v)
}

for (const [rule, found] of byRule) {
  console.error(`\n✗ ${rule}  (docs/rules/${rule}.md)`)
  for (const v of found) {
    console.error(`    ${v.file}${v.line ? `:${v.line}` : ''}  ${v.message}`)
  }
}
console.error(`\n${violations.length} violation(s) across ${byRule.size} rule(s)`)
process.exit(1)
