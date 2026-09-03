/**
 * Enforces the machine-checkable rules in docs/rules/.
 *
 * A rule an agent can violate without CI noticing is a rule that will be
 * violated, so every rule in docs/rules/ that *can* be checked is checked here.
 * Each check is independent and reports every violation it finds rather than
 * stopping at the first, so one run tells you everything.
 *
 * The shape is carried over from v2, which ran nine checks. This file starts
 * with the two that wayfinder ticket #9 owns. Ticket #16 adds the rest
 * (provenance headers, attribution, em dashes, text hygiene) to the same
 * CHECKS array.
 *
 * No dependencies, on purpose: the stack is still open on ticket #10, and this
 * has to run before there is one.
 *
 * Usage: node scripts/check-rules.mjs
 */

import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = fileURLToPath(new URL('..', import.meta.url))
const TOKENS = join(ROOT, 'design', 'tokens.css')

/** @type {{rule: string, file: string, line?: number, message: string}[]} */
const violations = []

function fail(rule, file, message, line) {
  violations.push({ rule, file: relative(ROOT, file).split(sep).join('/'), line, message })
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

    source.split('\n').forEach((text, i) => {
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

const CHECKS = [checkNoRawValues, checkThemeContrast]

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
