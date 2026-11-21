#!/usr/bin/env node
// Demido Studio design tokens: src/*.json -> dist/tokens.css, tokens.json, tokens.ts
// Zero dependencies. Run: node brand/tokens/build.mjs
//
// Source format (DTCG-like):
//   { "group": { "name": { "$value": "...", "$type": "color", "$description": "..." } } }
// Themed value:   "$value": { "dark": "...", "light": "..." }
// Alias:          "$value": "{color.neutral.3}"   (may point at a themed token)
// Spring:         "$type": "spring", "$value": { "response": 0.35, "damping": 1 }
//                 -> emits a CSS linear() easing plus its settle duration.
// Derived:        "$derived": "<css expression>" (or { "dark": ..., "light": ... }) next to a static "$value".
//                 The static value is the default and the fallback; the expression is emitted inside
//                 @supports (relative colour syntax) and recomputes live from other custom properties.
//                 Used for the accent: every accent shade follows --d-color-accent-source at runtime.

import { readFileSync, writeFileSync, readdirSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const SRC = join(here, 'src')
const DIST = join(here, 'dist')
const PREFIX = 'd'
const THEMES = ['dark', 'light']

// ---------- load + flatten ----------
const tree = {}
for (const f of readdirSync(SRC).filter((f) => f.endsWith('.json')).sort()) {
  const part = JSON.parse(readFileSync(join(SRC, f), 'utf8'))
  merge(tree, part, f)
}

function merge(into, from, file) {
  for (const [k, v] of Object.entries(from)) {
    if (k === '$type') {
      into.$type = v // group-level type is inherited by the tokens below it
      continue
    }
    if (k.startsWith('$')) continue
    if (v && typeof v === 'object' && !('$value' in v)) {
      into[k] ??= {}
      merge(into[k], v, file)
    } else {
      if (into[k]) throw new Error(`Duplicate token "${k}" in ${file}`)
      into[k] = v
    }
  }
}

/** @type {Map<string, {path:string[], raw:any, type?:string, description?:string}>} */
const tokens = new Map()
;(function walk(node, path, inheritedType) {
  const type = node.$type ?? inheritedType
  for (const [k, v] of Object.entries(node)) {
    if (k.startsWith('$')) continue
    if (v && typeof v === 'object' && '$value' in v) {
      tokens.set([...path, k].join('.'), {
        path: [...path, k],
        raw: v.$value,
        derived: v.$derived,
        type: v.$type ?? type,
        description: v.$description,
      })
    } else if (v && typeof v === 'object') {
      walk(v, [...path, k], type)
    }
  }
})(tree, [], undefined)

// ---------- resolve aliases and themes ----------
const ALIAS = /^\{([^}]+)\}$/
const INLINE_ALIAS = /\{([^}]+)\}/g

function isThemed(raw) {
  return raw && typeof raw === 'object' && !Array.isArray(raw) && THEMES.every((t) => t in raw)
}

function resolve(name, theme, seen = []) {
  const t = tokens.get(name)
  if (!t) throw new Error(`Unknown token reference {${name}} (via ${seen.join(' -> ') || 'root'})`)
  if (seen.includes(name)) throw new Error(`Alias cycle: ${[...seen, name].join(' -> ')}`)
  const raw = isThemed(t.raw) ? t.raw[theme] : t.raw
  return resolveValue(raw, theme, [...seen, name])
}

function resolveValue(raw, theme, seen) {
  if (typeof raw !== 'string') return raw
  const whole = raw.match(ALIAS)
  if (whole) return resolve(whole[1], theme, seen)
  return raw.replace(INLINE_ALIAS, (_, ref) => String(resolve(ref, theme, seen)))
}

function themeDependent(name, seen = new Set()) {
  if (seen.has(name)) return false
  seen.add(name)
  const t = tokens.get(name)
  if (!t) return false
  if (isThemed(t.raw)) return true
  const refs = typeof t.raw === 'string' ? [...t.raw.matchAll(INLINE_ALIAS)].map((m) => m[1]) : []
  return refs.some((r) => themeDependent(r, seen))
}

// ---------- colour maths (OKLCH -> sRGB hex, gamut-mapped by chroma reduction) ----------
const OKLCH = /^oklch\(\s*([\d.]+%?)\s+([\d.]+)\s+([\d.]+)(?:deg)?\s*(?:\/\s*([\d.]+%?)\s*)?\)$/i

function parseOklch(v) {
  const m = typeof v === 'string' && v.match(OKLCH)
  if (!m) return null
  const pct = (s) => (s.endsWith('%') ? parseFloat(s) / 100 : parseFloat(s))
  return { L: pct(m[1]), C: parseFloat(m[2]), h: parseFloat(m[3]), a: m[4] === undefined ? 1 : pct(m[4]) }
}

function oklchToLinear({ L, C, h }) {
  const hr = (h * Math.PI) / 180
  const a = C * Math.cos(hr)
  const b = C * Math.sin(hr)
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

const inGamut = (rgb) => rgb.every((c) => c >= -0.0005 && c <= 1.0005)
const encode = (c) => {
  const x = Math.min(1, Math.max(0, c))
  return x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055
}

export function oklchToHex(v) {
  const c = parseOklch(v)
  if (!c) return null
  let lin = oklchToLinear(c)
  if (!inGamut(lin)) {
    let lo = 0
    let hi = c.C
    for (let i = 0; i < 24; i++) {
      const mid = (lo + hi) / 2
      if (inGamut(oklchToLinear({ ...c, C: mid }))) lo = mid
      else hi = mid
    }
    lin = oklchToLinear({ ...c, C: lo })
  }
  const hex = lin.map((x) => Math.round(encode(x) * 255).toString(16).padStart(2, '0')).join('')
  const alpha = c.a < 1 ? Math.round(c.a * 255).toString(16).padStart(2, '0') : ''
  return `#${hex}${alpha}`
}

// ---------- springs -> CSS linear() ----------
// Apple-style parameters: response (seconds) and damping ratio (1 = critically damped).
export function springToLinear({ response, damping }) {
  const w0 = (2 * Math.PI) / response
  const z = damping
  const pos = (t) => {
    if (z >= 1) return 1 - Math.exp(-w0 * t) * (1 + w0 * t)
    const wd = w0 * Math.sqrt(1 - z * z)
    return 1 - Math.exp(-z * w0 * t) * (Math.cos(wd * t) + ((z * w0) / wd) * Math.sin(wd * t))
  }
  // settle time: last moment the value is more than 0.1% away from rest
  let settle = response
  for (let t = 6 * response; t > 0; t -= 0.001) {
    if (Math.abs(pos(t) - 1) > 0.001) {
      settle = t + 0.001
      break
    }
  }
  const n = 32
  const pts = []
  for (let i = 0; i <= n; i++) pts.push(+pos((settle * i) / n).toFixed(4))
  pts[0] = 0
  pts[n] = 1
  return { easing: `linear(${pts.join(', ')})`, duration: Math.round(settle * 1000) }
}

// ---------- emit ----------
const varName = (path) => `--${PREFIX}-${path.join('-')}`

function cssValue(t, value) {
  if (t.type === 'spring') return springToLinear(value).easing
  if (t.type === 'fontFamily' && Array.isArray(value)) {
    return value.map((f) => (/^[a-z-]+$/.test(f) ? f : `"${f}"`)).join(', ')
  }
  if (t.type === 'cubicBezier' && Array.isArray(value)) return `cubic-bezier(${value.join(', ')})`
  if (t.type === 'shadow' && Array.isArray(value)) return value.join(', ')
  return String(value)
}

const base = []
const themed = Object.fromEntries(THEMES.map((t) => [t, []]))
const flat = {}

for (const [name, t] of tokens) {
  const dep = themeDependent(name)
  const entry = { type: t.type, description: t.description, var: varName(t.path) }
  if (dep) {
    entry.value = {}
    for (const theme of THEMES) {
      const v = resolve(name, theme)
      const css = cssValue(t, v)
      themed[theme].push(`  ${varName(t.path)}: ${css};`)
      entry.value[theme] = css
      const hex = oklchToHex(css)
      if (hex) (entry.hex ??= {})[theme] = hex
    }
  } else {
    const v = resolve(name, THEMES[0])
    const css = cssValue(t, v)
    base.push(`  ${varName(t.path)}: ${css};`)
    entry.value = css
    const hex = oklchToHex(css)
    if (hex) entry.hex = hex
    if (t.type === 'spring') {
      const s = springToLinear(v)
      base.push(`  ${varName(t.path)}-duration: ${s.duration}ms;`)
      entry.spring = { ...v, duration: s.duration }
    }
  }
  flat[name] = entry
}

const banner = `/* Demido Studio design tokens. GENERATED by brand/tokens/build.mjs from brand/tokens/src. Do not edit. */`
const css = `${banner}
:root {
${base.join('\n')}
}

/* Dark is the default theme. */
:root,
[data-theme="dark"] {
  color-scheme: dark;
${themed.dark.join('\n')}
}

[data-theme="light"] {
  color-scheme: light;
${themed.light.join('\n')}
}

/* Follow the system when the user has not chosen a theme. */
@media (prefers-color-scheme: light) {
  :root:not([data-theme]) {
    color-scheme: light;
${themed.light.map((l) => '  ' + l).join('\n')}
  }
}
`

// ---------- derived tokens (live, behind @supports) ----------
const derivedLines = Object.fromEntries(THEMES.map((t) => [t, []]))
for (const [name, t] of tokens) {
  if (t.derived === undefined) continue
  for (const theme of THEMES) {
    const expr = isThemed(t.derived) ? t.derived[theme] : t.derived
    derivedLines[theme].push(`${varName(t.path)}: ${expr};`)
  }
  flat[name].derived = t.derived
}
const indent = (lines, n) => lines.map((l) => ' '.repeat(n) + l).join('\n')
const derivedCss = derivedLines.dark.length
  ? `
/* Live values. Where the engine supports relative colours, these replace the static ones above and recompute
   whenever their inputs change (for the accent: --d-color-accent-source, set by the app from the user's
   setting or the system accent). Engines without support keep the static defaults. */
@supports (color: oklch(from red l c h)) {
  :root,
  [data-theme="dark"] {
${indent(derivedLines.dark, 4)}
  }
  [data-theme="light"] {
${indent(derivedLines.light, 4)}
  }
  @media (prefers-color-scheme: light) {
    :root:not([data-theme]) {
${indent(derivedLines.light, 6)}
    }
  }
}
`
  : ''

mkdirSync(DIST, { recursive: true })
writeFileSync(join(DIST, 'tokens.css'), css + derivedCss)
writeFileSync(join(DIST, 'tokens.json'), JSON.stringify(flat, null, 2) + '\n')
writeFileSync(
  join(DIST, 'tokens.ts'),
  `${banner}
export const tokens = ${JSON.stringify(flat, null, 2)} as const

export type TokenName = keyof typeof tokens

/** CSS reference for a token, for example cssVar('color.text.primary') -> 'var(--d-color-text-primary)'. */
export const cssVar = (name: TokenName): string => \`var(\${tokens[name].var})\`
`,
)

// ---------- state glyphs -> CSS masks ----------
// Every SVG in brand/icons/state becomes a custom property holding a data URI, so a glyph can be drawn
// with one element and recoloured with currentColor:  mask-image: var(--d-glyph-task-done)
const GLYPHS = join(here, '..', 'icons', 'state')
const glyphVars = readdirSync(GLYPHS)
  .filter((f) => f.endsWith('.svg'))
  .sort()
  .map((f) => {
    const svg = readFileSync(join(GLYPHS, f), 'utf8')
      .replace(/<title>[^<]*<\/title>/g, '')
      .replace(/\s*(width|height)="16"/g, '')
      .replace(/currentColor/g, '#000')
      .replace(/\s+/g, ' ')
      .replace(/> </g, '><')
      .trim()
    const uri = svg.replace(/"/g, "'").replace(/%/g, '%25').replace(/#/g, '%23').replace(/</g, '%3C').replace(/>/g, '%3E')
    return `  --${PREFIX}-glyph-${f.replace(/\.svg$/, '')}: url("data:image/svg+xml,${uri}");`
  })
writeFileSync(
  join(DIST, 'glyphs.css'),
  `/* Demido Studio state glyphs as CSS masks. GENERATED by brand/tokens/build.mjs from brand/icons/state. Do not edit. */\n:root {\n${glyphVars.join('\n')}\n}\n`,
)

console.log(`tokens: ${tokens.size} tokens, ${glyphVars.length} glyphs -> ${DIST}`)
