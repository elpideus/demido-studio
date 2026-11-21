// Demido Studio colour lab: resolve the spec, fix what fails, measure everything.
// Usage: node build.mjs            -> writes out/tokens.json, out/tokens.css, out/meta.js, out/tables.md (T1-T10),
//                                   out/recommended.md (the kit for palette.mjs RECOMMEND) and specimen.html
//        node build.mjs --verbose  -> also prints the fix log and collision matrices
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { color, wcag, apca, deltaE, hexToOklch, fmtOklch } from './lib/color.mjs';
import { STEPS, ORDER, TEMPS, SURFACES, ACCENTS, ACCENT_ORDER, SEMANTICS, T, DATA_SLOTS, NEIGHBOURS, SCAN, RECOMMEND } from './palette.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, 'out');
mkdirSync(OUT, { recursive: true });
const VERBOSE = process.argv.includes('--verbose');
const THEMES = ['dark', 'light'];
const WHITE = '#ffffff';
const fixes = []; // every value the build had to move, with the reason
const lc = (text, bg) => Math.abs(apca(text, bg));
const r1 = (n) => Math.round(n * 10) / 10;
const r2 = (n) => Math.round(n * 100) / 100;

// Move L from the intent in direction dir until pred(hex) holds. Chroma and hue are held (gamut-mapped).
function solve([L, C, h], dir, pred, step = 0.0025) {
  let l = L;
  for (let i = 0; i < 400; i++) {
    const c = color(l, C, h);
    if (pred(c.hex)) return c;
    l += dir * step;
    if (l < 0.02 || l > 0.995) break;
  }
  return null;
}
function logFix(where, what, from, to, why) {
  if (fixes.some((x) => x.where === where && x.what === what && x.from === from.hex)) return;
  if (!to) { fixes.push({ where, what, from: from.hex, to: null, why: why + ' (NO SOLUTION)' }); return; }
  if (from.hex !== to.hex) fixes.push({ where, what, from: from.hex, to: to.hex, fromL: r2(from.L) , toL: r2(to.L), why });
}

// ============ 1. neutral ramps ============
const neutrals = {}; // neutrals[temp][theme][key] = colour object
for (const [tk, temp] of Object.entries(TEMPS)) {
  neutrals[tk] = {};
  for (const theme of THEMES) {
    const dark = theme === 'dark';
    const base = dark ? temp.cDark : temp.cLight;
    const ramp = {};
    for (const [key, role, dL, lL, cw] of STEPS) {
      ramp[key] = { ...color(dark ? dL : lL, base * cw, temp.h), key, role, intent: [dark ? dL : lL, base * cw, temp.h] };
    }
    const surf = SURFACES.map((s) => ramp[s].hex);
    const dir = dark ? +1 : -1; // more contrast = lighter on dark, darker on light
    const need = (key, t, extra = []) => {
      const targets = [...surf, ...extra];
      const pred = (hex) => targets.every((s) => wcag(hex, s) >= t.wcag && (!t.lc || lc(hex, s) >= t.lc));
      const before = ramp[key];
      if (pred(before.hex)) return;
      const after = solve(before.intent, dir, pred);
      logFix(`ramp ${TEMPS[tk].label.toLowerCase()}, ${theme}`, key, before, after, `needs ${t.wcag}:1${t.lc ? ' and Lc ' + t.lc : ''} on every surface`);
      if (after) ramp[key] = { ...after, key, role: before.role, intent: before.intent, fixed: true };
    };
    need('text1', T.body, [ramp.fill.hex]);
    need('text2', T.secondary, [ramp.fill.hex]);
    need('text3', T.tertiary);
    need('border', T.boundary);
    neutrals[tk][theme] = ramp;
  }
}

// ============ 2. accents ============
// Accent values are solved against the true-neutral surfaces, then verified against warm and cool.
const INK_DARK = neutrals.neutral.light.solid.hex; // near-black ink for text on a bright fill
function pickInk(fillHex) {
  const d = { hex: INK_DARK, lc: lc(INK_DARK, fillHex), wcag: wcag(INK_DARK, fillHex), kind: 'dark' };
  const w = { hex: WHITE, lc: lc(WHITE, fillHex), wcag: wcag(WHITE, fillHex), kind: 'white' };
  return d.lc >= w.lc ? d : w;
}
const inkOk = (fillHex) => { const i = pickInk(fillHex); return i.wcag >= T.onAccent.wcag && i.lc >= T.onAccent.lc; };

function tintOf(theme, h, C) {
  // a quiet wash of the hue on the raised surface
  return theme === 'dark' ? color(0.270, Math.min(C, 0.048), h) : color(0.950, Math.min(C, 0.032), h);
}
function lineOf(theme, h, C) {
  return theme === 'dark' ? color(0.420, Math.min(C, 0.090), h) : color(0.840, Math.min(C, 0.075), h);
}

function resolveAccent(key, theme) {
  const spec = ACCENTS[key];
  const dark = theme === 'dark';
  const N = neutrals.neutral[theme];
  const surf = SURFACES.map((s) => N[s].hex);
  const dir = dark ? +1 : -1;
  const intent = spec[theme];
  const intentC = color(...intent);
  const where = `accent ${key} ${theme}`;

  // base: holds 3:1 as a mark on every surface, and carries legible text when used as a fill
  let base = intentC;
  const boundary = (hex) => surf.every((s) => wcag(hex, s) >= T.boundary.wcag);
  if (!boundary(base.hex)) {
    const fixed = solve(intent, dir, boundary);
    logFix(where, 'base', base, fixed, 'a mark (dot, arc, focus ring, toggle) needs 3:1 on every surface');
    if (fixed) base = fixed;
  }
  if (!inkOk(base.hex)) {
    // push the fill away from the middle until one of the two inks passes
    const cur = [base.L, intent[1], intent[2]];
    const up = solve(cur, +1, (hex) => inkOk(hex) && boundary(hex));
    const down = solve(cur, -1, (hex) => inkOk(hex) && boundary(hex));
    const pick = [up, down].filter(Boolean).sort((a, b) => Math.abs(a.L - base.L) - Math.abs(b.L - base.L))[0];
    logFix(where, 'base', base, pick, 'text on the fill needs 4.5:1 and Lc 60');
    if (pick) base = pick;
  }
  const ink = pickInk(base.hex);

  // hover and pressed
  const C = intent[1], h = intent[2];
  let hoverL, pressedL;
  if (dark) {
    if (base.L <= 0.84) { hoverL = base.L + 0.04; pressedL = base.L - 0.06; }
    else { hoverL = base.L - 0.045; pressedL = base.L - 0.09; }
  } else if (ink.kind === 'white') { hoverL = base.L - 0.045; pressedL = base.L - 0.09; }
  else { hoverL = base.L - 0.04; pressedL = base.L - 0.08; }
  if (key === 'mono') { hoverL = dark ? 0.925 : 0.285; pressedL = dark ? 0.86 : 0.36; }
  let hover = color(hoverL, C, h), pressed = color(pressedL, C, h);
  for (const [name, get, set] of [['hover', () => hover, (v) => (hover = v)], ['pressed', () => pressed, (v) => (pressed = v)]]) {
    const cur = get();
    const ok = (hex) => wcag(ink.hex, hex) >= T.onAccent.wcag;
    if (!ok(cur.hex)) {
      const fixed = solve([cur.L, C, h], ink.kind === 'dark' ? +1 : -1, ok);
      logFix(where, name, cur, fixed, 'text on the hover/pressed fill needs 4.5:1');
      if (fixed) set(fixed);
    }
  }

  const tint = key === 'mono' ? (dark ? color(0.31, 0, 0) : color(0.915, 0, 0)) : tintOf(theme, h, C);
  const line = key === 'mono' ? (dark ? color(0.46, 0, 0) : color(0.76, 0, 0)) : lineOf(theme, h, C);

  // accent used as text: 4.5:1 and Lc 60 on every surface and on its own tint
  let text = base;
  const textOk = (hex) => [...surf, tint.hex].every((s) => wcag(hex, s) >= T.accentText.wcag && lc(hex, s) >= T.accentText.lc);
  if (!textOk(text.hex)) {
    const fixed = solve([base.L, C, h], dir, textOk);
    logFix(where, 'text', text, fixed, 'accent as text needs 4.5:1 and Lc 60 on every surface and on its tint');
    if (fixed) text = fixed;
  }
  return { key, label: spec.label, note: spec.note, theme, intent: intentC, base, hover, pressed, ink, tint, line, text };
}
const accents = {};
for (const key of ACCENT_ORDER) { accents[key] = {}; for (const theme of THEMES) accents[key][theme] = resolveAccent(key, theme); }

// ============ 3. semantics: search each signal's window for the most separable set ============
const KINDS = ['normal', 'deutan', 'protan'];
function pairStats(a, b) {
  const d = Object.fromEntries(KINDS.map((k) => [k, deltaE(a, b, k)]));
  const cvd = Math.min(d.deutan, d.protan);
  const status = d.normal < T.dE.normal || cvd < T.dE.cvdFloor ? 'FAIL' : cvd < T.dE.cvd ? 'WARN' : 'ok';
  return { ...d, cvd, status, score: Math.min(d.normal / T.dE.normal, cvd / T.dE.cvd) };
}
function resolveSemantic(name, [L, C, h], theme) {
  const N = neutrals.neutral[theme];
  const surf = SURFACES.map((s) => N[s].hex);
  const tint = tintOf(theme, h, C), line = lineOf(theme, h, C);
  const fg = color(L, C, h);
  const ok = [...surf, tint.hex].every((s) => wcag(fg.hex, s) >= T.semText.wcag && lc(fg.hex, s) >= T.semText.lc);
  // solid fill (destructive button, filled badge): the same colour when it can carry ink, else the nearest that can
  // The fill is the deep version with white ink in both themes: on dark, a bright red fill with dark ink sits too
  // close to a bright warm accent fill, and a deep red with white text is the established "destructive" look.
  const whiteOk = (hex) => wcag(WHITE, hex) >= T.onAccent.wcag && lc(WHITE, hex) >= T.onAccent.lc && surf.every((s) => wcag(hex, s) >= T.boundary.wcag);
  const solid = solve([theme === 'dark' ? 0.60 : 0.56, C, h], -1, whiteOk) ?? fg;
  const ink = pickInk(solid.hex);
  const solidHover = color(solid.L - 0.045, C, h);
  return { name, ...SEMANTICS.meta[name], spec: [L, C, h], fg, tint, line, solid, solidHover, ink, ok };
}
const semCands = {}; // semCands[theme][name] = passing candidates
for (const theme of THEMES) {
  semCands[theme] = {};
  for (const name of SEMANTICS.order) {
    const g = SEMANTICS.grid[theme][name], [hL, hh] = SEMANTICS.home[theme][name];
    const list = [];
    for (const L of g.L) for (const h of g.h) {
      const c = resolveSemantic(name, [L, g.C, h], theme);
      if (!c.ok) continue;
      c.cost = Math.abs(L - hL) / 0.03 + Math.abs(h - hh) / 8; // distance from the canonical value, in grid steps
      list.push(c);
    }
    if (!list.length) throw new Error(`no legible candidate for ${name} ${theme}`);
    semCands[theme][name] = list;
  }
}
// Best set, optionally with an accent in it. Order of preference:
// 1. highest worst-pair score, capped at "comfortably passes" (so a set that already clears every floor is not bent further);
// 2. closest to `anchor` (the base set) or to the canonical values; 3. highest uncapped worst-pair score.
function searchSemantics(theme, accentHex, anchor, cap) {
  const [S, Cn, D, I] = SEMANTICS.order.map((n) => semCands[theme][n]);
  const lists = [S, Cn, D, I];
  const pre = (A, B) => A.map((a) => B.map((b) => pairStats(a.fg.hex, b.fg.hex).score));
  const P = {}; for (let i = 0; i < 4; i++) for (let j = i + 1; j < 4; j++) P[i + '' + j] = pre(lists[i], lists[j]);
  const A = accentHex ? lists.map((l) => l.map((c) => pairStats(accentHex, c.fg.hex).score)) : null;
  const costOf = (c, i) => (anchor ? Math.abs(c.spec[0] - anchor[i].spec[0]) / 0.03 + Math.abs(c.spec[2] - anchor[i].spec[2]) / 8 : c.cost);
  let best = null;
  for (let a = 0; a < S.length; a++) for (let b = 0; b < Cn.length; b++) {
    const s01 = P['01'][a][b];
    for (let c = 0; c < D.length; c++) {
      const s3 = Math.min(s01, P['02'][a][c], P['12'][b][c]);
      for (let d = 0; d < I.length; d++) {
        let sc = Math.min(s3, P['03'][a][d], P['13'][b][d], P['23'][c][d]);
        if (A) sc = Math.min(sc, A[0][a], A[1][b], A[2][c], A[3][d]);
        const capped = Math.min(sc, cap);
        const cost = costOf(S[a], 0) + costOf(Cn[b], 1) + costOf(D[c], 2) + costOf(I[d], 3);
        if (!best || capped > best.capped + 1e-9 || (Math.abs(capped - best.capped) < 1e-9 && (cost < best.cost - 1e-9 || (Math.abs(cost - best.cost) < 1e-9 && sc > best.sc)))) best = { capped, cost, sc, pick: [S[a], Cn[b], D[c], I[d]] };
      }
    }
  }
  return best;
}
const semBase = {}, semBaseArr = {};
for (const theme of THEMES) {
  const r = searchSemantics(theme, null, null, 1.0);
  semBaseArr[theme] = r.pick;
  semBase[theme] = Object.fromEntries(r.pick.map((c) => [c.name, { ...c, shifted: false }]));
}
const semFor = {};
for (const key of ACCENT_ORDER) {
  semFor[key] = {};
  for (const theme of THEMES) {
    const r = searchSemantics(theme, accents[key][theme].base.hex, semBaseArr[theme], 1.0);
    semFor[key][theme] = Object.fromEntries(r.pick.map((c, i) => [c.name, { ...c, shifted: c.fg.hex !== semBaseArr[theme][i].fg.hex }]));
  }
}

// ============ 4. colour-difference checks (normal, deuteranopia, protanopia) ============
function collisions(accentKey, theme, sems) {
  const a = accents[accentKey][theme].base.hex;
  const rows = [];
  for (const name of SEMANTICS.order) rows.push({ pair: `accent / ${name}`, ...pairStats(a, sems[name].fg.hex) });
  for (let i = 0; i < 4; i++) for (let j = i + 1; j < 4; j++) {
    const x = SEMANTICS.order[i], y = SEMANTICS.order[j];
    rows.push({ pair: `${x} / ${y}`, ...pairStats(sems[x].fg.hex, sems[y].fg.hex) });
  }
  return rows;
}
const cvd = {}; // cvd[accent][theme] = { base: rows with the base set, used: rows with the set searched for this accent }
for (const key of ACCENT_ORDER) {
  cvd[key] = {};
  for (const theme of THEMES) cvd[key][theme] = { base: collisions(key, theme, semBase[theme]), used: collisions(key, theme, semFor[key][theme]) };
}

// ============ 5. chart series: accent + the three reference slots that separate best ============
function pickSeries(accentKey, theme) {
  const a = accents[accentKey][theme].base.hex;
  const surface = neutrals.neutral[theme].raised.hex;
  // a slot in the accent's own hue family would read as a second "live" line: keep 40 degrees away
  const aLch = accents[accentKey][theme].base.actual;
  const pool = DATA_SLOTS.map((s) => ({ name: s.name, hex: s[theme] })).filter((s) => { const g = hueGap(aLch, hexToOklch(s.hex)); return g == null || g >= 40; });
  let best = null;
  for (let i = 0; i < pool.length; i++) for (let j = i + 1; j < pool.length; j++) for (let k = j + 1; k < pool.length; k++) {
    const set = [{ name: 'accent', hex: a }, pool[i], pool[j], pool[k]];
    let worst = Infinity, wn = Infinity, wc = Infinity;
    for (let x = 0; x < 4; x++) for (let y = x + 1; y < 4; y++) {
      const p = pairStats(set[x].hex, set[y].hex);
      wn = Math.min(wn, p.normal); wc = Math.min(wc, p.cvd);
      worst = Math.min(worst, p.normal / T.dE.normal, p.cvd / T.dE.cvd);
    }
    const minContrast = Math.min(...set.map((s) => wcag(s.hex, surface)));
    // first: every pair clears both floors; then: every line holds 3:1 on the card; then: the widest worst pair
    const rank = (worst >= 1 ? 10 : 0) + (minContrast >= 3 ? 5 : 0) + Math.min(worst, 4);
    if (!best || rank > best.rank) best = { set, worst, rank, normal: wn, cvd: wc, minContrast };
  }
  return best;
}
const series = {};
for (const key of ACCENT_ORDER) series[key] = { dark: pickSeries(key, 'dark'), light: pickSeries(key, 'light') };

// ============ 6. contrast tables ============
function neutralContrast(tk, theme) {
  const N = neutrals[tk][theme];
  const rows = [];
  const add = (label, key, t, surfaces) => {
    for (const s of surfaces) rows.push({ label, fg: N[key].hex, on: s, bg: N[s].hex, wcag: wcag(N[key].hex, N[s].hex), lc: apca(N[key].hex, N[s].hex), need: t });
  };
  add('Primary text', 'text1', T.body, [...SURFACES, 'fill']);
  add('Secondary text', 'text2', T.secondary, [...SURFACES, 'fill']);
  add('Tertiary text', 'text3', T.tertiary, SURFACES);
  add('Disabled (exempt)', 'disabled', null, ['bg', 'raised']);
  add('Strong border', 'border', T.boundary, SURFACES);
  add('Hairline (decorative)', 'hairline', null, ['bg', 'raised']);
  add('Solid fill as a mark', 'solid', T.boundary, ['bg', 'raised']);
  return rows;
}
function accentContrast(key, theme, tk = 'neutral') {
  const A = accents[key][theme], N = neutrals[tk][theme];
  const rows = [];
  const add = (label, fg, onLabel, bg, need) => rows.push({ label, fg, on: onLabel, bg, wcag: wcag(fg, bg), lc: apca(fg, bg), need });
  for (const s of SURFACES) add('Accent as a mark', A.base.hex, s, N[s].hex, T.boundary);
  for (const s of SURFACES) add('Accent as text', A.text.hex, s, N[s].hex, T.accentText);
  add('Accent as text', A.text.hex, 'tint', A.tint.hex, T.accentText);
  add('Text on accent fill', A.ink.hex, 'accent', A.base.hex, T.onAccent);
  add('Text on accent fill', A.ink.hex, 'hover', A.hover.hex, { wcag: 4.5 });
  add('Text on accent fill', A.ink.hex, 'pressed', A.pressed.hex, { wcag: 4.5 });
  return rows;
}
const passes = (r) => !r.need || (r.wcag >= r.need.wcag && (!r.need.lc || Math.abs(r.lc) >= r.need.lc));

// Identity drift between themes: how different the brand colour looks on dark and on light.
const drift = Object.fromEntries(ACCENT_ORDER.map((k) => {
  const d = accents[k].dark.base, l = accents[k].light.base;
  return [k, { dE: deltaE(d.hex, l.hex), dL: d.actual[0] - l.actual[0], dC: d.actual[1] - l.actual[1], intentMove: deltaE(accents[k].light.intent.hex, l.hex) }];
}));
// Nearest neighbour brand colour
const neighbour = Object.fromEntries(ACCENT_ORDER.map((k) => {
  const rows = NEIGHBOURS.map((n) => ({ name: n.name, hex: n.hex, dE: Math.min(deltaE(accents[k].dark.base.hex, n.hex), deltaE(accents[k].light.base.hex, n.hex)), dh: hueGap(accents[k].dark.base.actual, hexToOklch(n.hex)) }));
  rows.sort((a, b) => a.dE - b.dE);
  return [k, rows.slice(0, 2)];
}));
function hueGap(a, b) { if (a[1] < 0.02 || b[1] < 0.02) return null; const d = Math.abs(a[2] - b[2]) % 360; return d > 180 ? 360 - d : d; }

// ============ 7. write tokens.json ============
const strip = (c) => ({ hex: c.hex, oklch: fmtOklch(c), L: r2(c.actual[0]) * 1, C: Math.round(c.actual[1] * 1000) / 1000, h: Math.round(c.actual[2]) });
const tokens = {
  generated: 'node build.mjs',
  neutrals: Object.fromEntries(Object.keys(TEMPS).map((tk) => [tk, Object.fromEntries(THEMES.map((th) => [th, Object.fromEntries(STEPS.map(([k]) => [k, strip(neutrals[tk][th][k])]))]))])),
  accents: Object.fromEntries(ACCENT_ORDER.map((k) => [k, Object.fromEntries(THEMES.map((th) => {
    const A = accents[k][th];
    return [th, { base: strip(A.base), hover: strip(A.hover), pressed: strip(A.pressed), tint: strip(A.tint), line: strip(A.line), text: strip(A.text), onAccent: A.ink.hex, intent: strip(A.intent) }];
  }))])),
  semantics: Object.fromEntries(ACCENT_ORDER.map((k) => [k, Object.fromEntries(THEMES.map((th) => [th, Object.fromEntries(SEMANTICS.order.map((s) => {
    const S = semFor[k][th][s];
    return [s, { fg: strip(S.fg), tint: strip(S.tint), line: strip(S.line), solid: strip(S.solid), onSolid: S.ink.hex, shifted: S.shifted }];
  }))]))])),
  semanticsBase: Object.fromEntries(THEMES.map((th) => [th, Object.fromEntries(SEMANTICS.order.map((s) => { const S = semBase[th][s]; return [s, { fg: strip(S.fg), tint: strip(S.tint), line: strip(S.line), solid: strip(S.solid), onSolid: S.ink.hex }]; }))])),
  series: Object.fromEntries(ACCENT_ORDER.map((k) => [k, Object.fromEntries(THEMES.map((th) => [th, { set: series[k][th].set, worstNormal: r1(series[k][th].normal), worstCvd: r1(series[k][th].cvd), minContrast: r2(series[k][th].minContrast) }]))])),
  cvd, drift, neighbour, fixes,
};
if (!SCAN) writeFileSync(join(OUT, 'tokens.json'), JSON.stringify(tokens, null, 1));

// ============ 8. write tokens.css ============
let css = '/* generated by build.mjs: do not edit */\n';
for (const tk of Object.keys(TEMPS)) for (const th of THEMES) {
  const N = neutrals[tk][th];
  const dark = th === 'dark';
  css += `html[data-theme="${th}"][data-neutral="${tk}"]{color-scheme:${th};`;
  for (const [k] of STEPS) css += `--n-${k}:${N[k].hex};`;
  const on = dark ? INK_DARK : WHITE;
  const sh = color(dark ? 0.925 : 0.285, N.solid.C, N.solid.h).hex, sp = color(dark ? 0.86 : 0.36, N.solid.C, N.solid.h).hex;
  css += `--n-on-solid:${on};--n-solid-hover:${sh};--n-solid-pressed:${sp};`;
  css += `--shadow:${dark ? '0 1px 0 rgba(255,255,255,.04) inset, 0 12px 32px rgba(0,0,0,.5)' : '0 1px 2px rgba(0,0,0,.06), 0 12px 32px rgba(0,0,0,.10)'};`;
  css += '}\n';
}
for (const k of ACCENT_ORDER) for (const th of THEMES) {
  const A = accents[k][th], S = semFor[k][th], R = series[k][th];
  css += `html[data-theme="${th}"][data-accent="${k}"]{`;
  css += `--a:${A.base.hex};--a-hover:${A.hover.hex};--a-pressed:${A.pressed.hex};--a-on:${A.ink.hex};--a-text:${A.text.hex};--a-tint:${A.tint.hex};--a-line:${A.line.hex};`;
  for (const s of SEMANTICS.order) css += `--${s}:${S[s].fg.hex};--${s}-tint:${S[s].tint.hex};--${s}-line:${S[s].line.hex};--${s}-solid:${S[s].solid.hex};--${s}-solid-hover:${S[s].solidHover.hex};--${s}-on:${S[s].ink.hex};`;
  R.set.forEach((s, i) => (css += `--s${i + 1}:${s.hex};`));
  css += '}\n';
}
if (!SCAN) writeFileSync(join(OUT, 'tokens.css'), css);

// labels for the specimen (accent names, series names, hexes for swatch captions)
const meta = {
  temps: Object.fromEntries(Object.entries(TEMPS).map(([k, t]) => [k, t.label])),
  steps: Object.fromEntries(THEMES.map((th) => [th, ORDER[th].map((k) => ({ k, role: STEPS.find((x) => x[0] === k)[1] }))])),
  neutrals: tokens.neutrals,
  accents: Object.fromEntries(ACCENT_ORDER.map((k) => [k, { label: ACCENTS[k].label, note: ACCENTS[k].note, dark: tokens.accents[k].dark, light: tokens.accents[k].light }])),
  semantics: tokens.semantics,
  series: Object.fromEntries(ACCENT_ORDER.map((k) => [k, { dark: series[k].dark.set.map((s) => s.name), light: series[k].light.set.map((s) => s.name) }])),
};
if (!SCAN) writeFileSync(join(OUT, 'meta.js'), 'window.DEMIDO_META=' + JSON.stringify(meta) + ';\n');

// ============ 9. tables.md ============
const f = (n, d = 2) => n.toFixed(d);
const ROLE = Object.fromEntries(STEPS.map(([k, role]) => [k, role]));
const mark = (ok) => (ok ? 'pass' : '**FAIL**');
let md = '';
const H = (s) => (md += `\n${s}\n\n`);
const table = (head, rows) => { md += `| ${head.join(' | ')} |\n| ${head.map(() => '---').join(' | ')} |\n` + rows.map((r) => `| ${r.join(' | ')} |`).join('\n') + '\n'; };

H('### T1. Neutral ramps (12 steps, three temperatures, both themes)');
for (const th of THEMES) {
  md += `\n**${th === 'dark' ? 'Dark' : 'Light'} theme**\n\n`;
  table(['Step', 'Role', 'True neutral', 'Warm (h 80)', 'Cool (h 255)'], ORDER[th].map((k, i) => [
    `${i + 1} \`${k}\``, ROLE[k], ...Object.keys(TEMPS).map((tk) => { const c = neutrals[tk][th][k]; return `\`${c.hex}\` ${fmtOklch(c)}`; }),
  ]));
}

H('### T2. Neutral contrast (WCAG 2 ratio and APCA Lc)');
md += 'Negative Lc = light text on a dark background. Requirement in the last column; "lab" marks a target the lab set itself.\n';
for (const th of THEMES) {
  md += `\n**${th === 'dark' ? 'Dark' : 'Light'} theme, true neutral**\n\n`;
  table(['Use', 'Colour', 'On', 'WCAG 2', 'APCA Lc', 'Needs', 'Result'], neutralContrast('neutral', th).map((r) => [
    r.label, `\`${r.fg}\``, `${r.on} \`${r.bg}\``, f(r.wcag) + ':1', f(r.lc, 1), r.need ? `${r.need.wcag}:1${r.need.lc ? ', Lc ' + r.need.lc : ''}` : 'none', r.need ? mark(passes(r)) : 'n/a',
  ]));
}
md += '\n**Warm and cool, worst case per use (lowest value over the four surfaces)**\n\n';
{
  const rows = [];
  for (const tk of ['warm', 'cool']) for (const th of THEMES) {
    const all = neutralContrast(tk, th);
    for (const label of ['Primary text', 'Secondary text', 'Tertiary text', 'Strong border']) {
      const g = all.filter((r) => r.label === label && SURFACES.includes(r.on));
      const w = g.reduce((a, b) => (b.wcag < a.wcag ? b : a));
      const l = g.reduce((a, b) => (Math.abs(b.lc) < Math.abs(a.lc) ? b : a));
      rows.push([`${tk} ${th}`, label, `\`${w.fg}\``, `${f(w.wcag)}:1 on ${w.on}`, `${f(l.lc, 1)} on ${l.on}`, mark(g.every(passes))]);
    }
  }
  table(['Ramp', 'Use', 'Colour', 'Lowest WCAG 2', 'Lowest APCA Lc', 'Result'], rows);
}

H('### T3. Accent candidates (values after fixing)');
for (const th of THEMES) {
  md += `\n**${th === 'dark' ? 'Dark' : 'Light'} theme**\n\n`;
  table(['Accent', 'Base', 'Hover', 'Pressed', 'Subtle tint', 'As text', 'Text on fill'], ACCENT_ORDER.map((k) => {
    const A = accents[k][th];
    return [A.label, `\`${A.base.hex}\` ${fmtOklch(A.base)}`, `\`${A.hover.hex}\``, `\`${A.pressed.hex}\``, `\`${A.tint.hex}\``, `\`${A.text.hex}\``, `\`${A.ink.hex}\` (${A.ink.kind})`];
  }));
}

H('### T4. Accent contrast (true-neutral surfaces)');
for (const th of THEMES) {
  md += `\n**${th === 'dark' ? 'Dark' : 'Light'} theme**. Mark = dot, arc, focus ring, toggle track: needs 3:1. Text: 4.5:1 and Lc 60 (lab). Lowest value over the four surfaces is shown.\n\n`;
  table(['Accent', 'As a mark: lowest WCAG', 'As text: lowest WCAG', 'As text: lowest Lc', 'As text on tint', 'Text on fill', 'on hover', 'on pressed', 'Result'], ACCENT_ORDER.map((k) => {
    const rows = accentContrast(k, th);
    const m = rows.filter((r) => r.label === 'Accent as a mark').reduce((a, b) => (b.wcag < a.wcag ? b : a));
    const t = rows.filter((r) => r.label === 'Accent as text' && r.on !== 'tint');
    const tw = t.reduce((a, b) => (b.wcag < a.wcag ? b : a)), tl = t.reduce((a, b) => (Math.abs(b.lc) < Math.abs(a.lc) ? b : a));
    const tt = rows.find((r) => r.on === 'tint'), of = rows.find((r) => r.on === 'accent'), oh = rows.find((r) => r.on === 'hover'), op = rows.find((r) => r.on === 'pressed');
    return [accents[k][th].label, `${f(m.wcag)}:1 (${m.on})`, `${f(tw.wcag)}:1 (${tw.on})`, `${f(tl.lc, 1)}`, `${f(tt.wcag)}:1, Lc ${f(tt.lc, 1)}`, `${f(of.wcag)}:1, Lc ${f(of.lc, 1)}`, `${f(oh.wcag)}:1`, `${f(op.wcag)}:1`, mark(rows.every(passes))];
  }));
}
md += '\n**Same accent values on warm and cool surfaces (lowest value over all rows of T4)**\n\n';
table(['Accent', 'warm dark', 'warm light', 'cool dark', 'cool light'], ACCENT_ORDER.map((k) => [ACCENTS[k].label, ...['warm', 'cool'].flatMap((tk) => THEMES.map((th) => {
  const rows = accentContrast(k, th, tk);
  return rows.every(passes) ? 'pass' : '**FAIL** ' + rows.filter((r) => !passes(r)).map((r) => `${r.label} on ${r.on} ${f(r.wcag)}:1`).join('; ');
}))]));

H('### T5. How far each accent had to move for the light theme');
md += 'Intent = the hue at the lightness where it still looks like itself. Fixed = after the 3:1 and 4.5:1 rules. Drift = OKLab distance x100 between the dark-theme and light-theme accent (0 = same colour in both themes).\n\n';
table(['Accent', 'Dark base', 'Light intent', 'Light fixed', 'Moved by (dE)', 'Dark-to-light drift (dE)', 'Lightness drop', 'Chroma change'], ACCENT_ORDER.map((k) => {
  const d = drift[k];
  if (k === 'mono') return [ACCENTS[k].label, `\`${accents[k].dark.base.hex}\``, `\`${accents[k].light.intent.hex}\``, `\`${accents[k].light.base.hex}\``, '0.0', 'n/a (inverts by design)', 'n/a', 'n/a'];
  return [ACCENTS[k].label, `\`${accents[k].dark.base.hex}\``, `\`${accents[k].light.intent.hex}\``, `\`${accents[k].light.base.hex}\``, f(d.intentMove, 1), f(d.dE, 1), f(d.dL), (d.dC >= 0 ? '-' : '+') + f(Math.abs(d.dC), 3)];
}));

H('### T6. Semantic set (base values)');
for (const th of THEMES) {
  md += `\n**${th === 'dark' ? 'Dark' : 'Light'} theme**\n\n`;
  const N = neutrals.neutral[th];
  table(['Signal', 'Glyph (never colour alone)', 'Text and glyph', 'Tint background', 'Border', 'Solid fill', 'Text on solid', 'fg on bg', 'fg on tint'], SEMANTICS.order.map((s) => {
    const S = semBase[th][s];
    return [S.label, S.glyph, `\`${S.fg.hex}\` ${fmtOklch(S.fg)}`, `\`${S.tint.hex}\``, `\`${S.line.hex}\``, `\`${S.solid.hex}\``, `\`${S.ink.hex}\` ${f(S.ink.wcag)}:1`, `${f(wcag(S.fg.hex, N.bg.hex))}:1, Lc ${f(apca(S.fg.hex, N.bg.hex), 1)}`, `${f(wcag(S.fg.hex, S.tint.hex))}:1, Lc ${f(apca(S.fg.hex, S.tint.hex), 1)}`];
  }));
}

H('### T7. Accent against the semantic set: colour difference under normal vision, deuteranopia and protanopia');
md += `OKLab distance x100. Simulation: Machado, Oliveira and Fernandes 2009, severity 1.0. Floors: normal vision at least ${T.dE.normal}; colour-blind at least ${T.dE.cvd} (${T.dE.cvdFloor} to ${T.dE.cvd} = WARN, legal only because every signal also has its own glyph and label; below ${T.dE.cvdFloor} = FAIL).\n`;
for (const th of THEMES) {
  md += `\n**${th === 'dark' ? 'Dark' : 'Light'} theme: accent against each signal** (normal / deutan / protan)\n\n`;
  table(['Accent', 'vs success', 'vs caution', 'vs danger', 'vs info', 'Worst', 'Semantic set used', 'Result'], ACCENT_ORDER.map((k) => {
    const rows = cvd[k][th].used.slice(0, 4);
    const worst = rows.reduce((a, b) => (Math.min(b.normal / T.dE.normal, b.cvd / T.dE.cvd) < Math.min(a.normal / T.dE.normal, a.cvd / T.dE.cvd) ? b : a));
    const st = rows.some((r) => r.status === 'FAIL') ? '**FAIL**' : rows.some((r) => r.status === 'WARN') ? 'WARN' : 'pass';
    const shifted = SEMANTICS.order.filter((s) => semFor[k][th][s].shifted);
    return [ACCENTS[k].label, ...rows.map((r) => `${f(r.normal, 0)} / ${f(r.deutan, 0)} / ${f(r.protan, 0)}${r.status === 'ok' ? '' : ' ' + (r.status === 'FAIL' ? '**FAIL**' : 'WARN')}`), worst.pair.replace('accent / ', ''), shifted.length ? 'shifted: ' + shifted.join(', ') : 'base', st];
  }));
}
md += '\n**Before any shift (base semantic set against every accent), for the record**\n\n';
table(['Accent', 'dark: failing or warned pairs', 'light: failing or warned pairs'], ACCENT_ORDER.map((k) => [ACCENTS[k].label, ...THEMES.map((th) => {
  const bad = cvd[k][th].base.slice(0, 4).filter((r) => r.status !== 'ok');
  return bad.length ? bad.map((r) => `${r.pair.replace('accent / ', '')} ${f(r.normal, 0)}/${f(r.deutan, 0)}/${f(r.protan, 0)} ${r.status}`).join('; ') : 'none';
})]));
md += '\n**The four signals against each other (base set)** (normal / deutan / protan)\n\n';
table(['Pair', 'Dark', 'Light'], cvd.mono.dark.base.slice(4).map((r, i) => {
  const l = cvd.mono.light.base.slice(4)[i];
  const cell = (x) => `${f(x.normal, 0)} / ${f(x.deutan, 0)} / ${f(x.protan, 0)}${x.status === 'ok' ? '' : ' ' + (x.status === 'FAIL' ? '**FAIL**' : 'WARN')}`;
  return [r.pair, cell(r), cell(l)];
}));

H('### T8. Chart series (accent = series 1, plus the three reference slots that separate best)');
table(['Accent', 'Dark: series 2 to 4', 'Dark worst pair (normal / CVD), lowest contrast', 'Light: series 2 to 4', 'Light worst pair (normal / CVD), lowest contrast'], ACCENT_ORDER.map((k) => [
  ACCENTS[k].label, ...THEMES.flatMap((th) => { const R = series[k][th]; return [R.set.slice(1).map((s) => `${s.name} \`${s.hex}\``).join(', '), `${f(R.normal, 1)} / ${f(R.cvd, 1)}, ${f(R.minContrast)}:1`]; }),
]));

H('### T9. Nearest neighbour brand colour (approximate values recalled by the lab, not fetched)');
table(['Accent', 'Nearest', 'dE', 'Hue gap', 'Second nearest', 'dE'], ACCENT_ORDER.map((k) => { const [a, b] = neighbour[k]; return [ACCENTS[k].label, `${a.name} \`${a.hex}\``, f(a.dE, 0), a.dh == null ? 'n/a' : f(a.dh, 0) + ' deg', `${b.name} \`${b.hex}\``, f(b.dE, 0)]; }));

H('### T10. Fix log (every value the build moved, and why)');
table(['Where', 'Value', 'From', 'To', 'L from', 'L to', 'Why'], fixes.map((x) => [x.where, x.what, `\`${x.from}\``, x.to ? `\`${x.to}\`` : 'none', x.fromL ?? '', x.toL ?? '', x.why]));
if (!SCAN) writeFileSync(join(OUT, 'tables.md'), md);

// ============ 9b. recommended.md: the kit for the recommended combination ============
{
  const { neutral: rn, accent: ra } = RECOMMEND;
  md = '';
  const cell = (c) => `\`${c.hex}\` ${fmtOklch(c)}`;
  md += `**Neutral ramp: ${TEMPS[rn].label.toLowerCase()}**\n\n`;
  table(['Role', 'Dark', 'Light'], STEPS.map(([k, role]) => [`\`${k}\` ${role}`, cell(neutrals[rn].dark[k]), cell(neutrals[rn].light[k])]));
  md += `\n**Accent: ${ACCENTS[ra].label}**\n\n`;
  const A = accents[ra];
  table(['Token', 'Dark', 'Light'], [
    ['Accent (mark and fill)', cell(A.dark.base), cell(A.light.base)],
    ['Hover', cell(A.dark.hover), cell(A.light.hover)],
    ['Pressed', cell(A.dark.pressed), cell(A.light.pressed)],
    ['Subtle tint background', cell(A.dark.tint), cell(A.light.tint)],
    ['Tint border', cell(A.dark.line), cell(A.light.line)],
    ['Accent as text', cell(A.dark.text), cell(A.light.text)],
    ['Text on accent fill', `\`${A.dark.ink.hex}\` (${f(A.dark.ink.wcag)}:1, Lc ${f(A.dark.ink.lc, 1)})`, `\`${A.light.ink.hex}\` (${f(A.light.ink.wcag)}:1, Lc ${f(A.light.ink.lc, 1)})`],
  ]);
  md += `\n**Signals used with ${ACCENTS[ra].label}** (a signal marked "moved" differs from the base set in T6)\n\n`;
  const S = semFor[ra];
  const rows = [];
  for (const s of SEMANTICS.order) {
    rows.push([`${SEMANTICS.meta[s].label}: text and glyph`, cell(S.dark[s].fg) + (S.dark[s].shifted ? ' (moved)' : ''), cell(S.light[s].fg) + (S.light[s].shifted ? ' (moved)' : '')]);
    rows.push([`${SEMANTICS.meta[s].label}: tint / border`, `\`${S.dark[s].tint.hex}\` / \`${S.dark[s].line.hex}\``, `\`${S.light[s].tint.hex}\` / \`${S.light[s].line.hex}\``]);
    rows.push([`${SEMANTICS.meta[s].label}: solid fill, white text`, `${cell(S.dark[s].solid)} (${f(S.dark[s].ink.wcag)}:1)`, `${cell(S.light[s].solid)} (${f(S.light[s].ink.wcag)}:1)`]);
  }
  table(['Token', 'Dark', 'Light'], rows);
  md += `\n**All ten pairs for ${ACCENTS[ra].label}** (OKLab distance x100: normal / deuteranopia / protanopia)\n\n`;
  const pc = (x) => `${f(x.normal, 1)} / ${f(x.deutan, 1)} / ${f(x.protan, 1)}${x.status === 'ok' ? '' : ' ' + (x.status === 'FAIL' ? '**FAIL**' : 'WARN')}`;
  table(['Pair', 'Dark', 'Light'], cvd[ra].dark.used.map((r, i) => [r.pair, pc(r), pc(cvd[ra].light.used[i])]));
  md += `\n**Primary action: ${RECOMMEND.primary}**\n\n`;
  const Nd = neutrals[rn].dark, Nl = neutrals[rn].light;
  table(['Token', 'Dark', 'Light'], [
    ['Primary fill', cell(Nd.solid), cell(Nl.solid)],
    ['Primary hover / pressed', `\`${color(0.925, 0, 0).hex}\` / \`${color(0.86, 0, 0).hex}\``, `\`${color(0.285, 0, 0).hex}\` / \`${color(0.36, 0, 0).hex}\``],
    ['Text on primary', `\`${INK_DARK}\` (${f(wcag(INK_DARK, Nd.solid.hex))}:1, Lc ${f(apca(INK_DARK, Nd.solid.hex), 1)})`, `\`${WHITE}\` (${f(wcag(WHITE, Nl.solid.hex))}:1, Lc ${f(apca(WHITE, Nl.solid.hex), 1)})`],
    ['Focus ring', `${cell(A.dark.base)}, 2 px, 2 px gap`, `${cell(A.light.base)}, 2 px, 2 px gap`],
  ]);
  if (!SCAN) writeFileSync(join(OUT, 'recommended.md'), md);
}

// ============ console summary ============
console.log(`neutrals: ${Object.keys(TEMPS).length} temps x 2 themes x ${STEPS.length} steps; accents: ${ACCENT_ORDER.length}; fixes: ${fixes.length}`);
if (VERBOSE) {
  for (const x of fixes) console.log(`  fix ${x.where} ${x.what}: ${x.from} -> ${x.to} (${x.why})`);
  for (const th of THEMES) {
    console.log(`\n== ${th}: neutral ramp`); console.log(STEPS.map(([k]) => `${k}:${neutrals.neutral[th][k].hex}`).join(' '));
    console.log(' warm ' + STEPS.map(([k]) => neutrals.warm[th][k].hex).join(' ')); console.log(' cool ' + STEPS.map(([k]) => neutrals.cool[th][k].hex).join(' '));
    for (const k of ACCENT_ORDER) {
      const A = accents[k][th];
      console.log(`${k.padEnd(8)} base ${A.base.hex} ${fmtOklch(A.base)} hover ${A.hover.hex} press ${A.pressed.hex} ink ${A.ink.kind} ${f(A.ink.wcag)} text ${A.text.hex} tint ${A.tint.hex} | drift ${f(drift[k].dE, 1)}`);
      console.log(`     worst pair score ${Math.min(...cvd[k][th].used.map((r) => r.score)).toFixed(2)} (1.00 = clears both floors)`);
      const sh = SEMANTICS.order.filter((n) => semFor[k][th][n].shifted).map((n) => `${n}->${semFor[k][th][n].fg.hex} ${fmtOklch(semFor[k][th][n].fg)}`);
      if (sh.length) console.log('     shifted ' + sh.join('  '));
      const bad = cvd[k][th].used.filter((r) => r.status !== 'ok');
      for (const r of bad) console.log(`     ${r.status} ${r.pair}: ${f(r.normal, 1)} / ${f(r.deutan, 1)} / ${f(r.protan, 1)}`);
    }
    console.log('semantics ' + SEMANTICS.order.map((s) => `${s}:${semBase[th][s].fg.hex} ${fmtOklch(semBase[th][s].fg)}`).join('  '));
  }
}
let failCount = 0;
for (const tk of Object.keys(TEMPS)) for (const th of THEMES) failCount += neutralContrast(tk, th).filter((r) => !passes(r)).length;
for (const k of ACCENT_ORDER) for (const th of THEMES) for (const tk of Object.keys(TEMPS)) failCount += accentContrast(k, th, tk).filter((r) => !passes(r)).length;
console.log(`contrast rows still failing after fixes: ${failCount}`);

// ============ 10. specimen.html: the source page with the brand's own SVGs inlined as a sprite ============
// (the logo and the state glyphs use currentColor, so they must be inline to take the theme's colours)
import { readFileSync } from 'node:fs';
if (!SCAN) {
  const BRAND = join(HERE, '..', '..', '..');
  const sym = (id, file) => {
    const src = readFileSync(join(BRAND, file), 'utf8');
    const vb = src.match(/viewBox="([^"]+)"/)[1];
    const inner = src.replace(/^[\s\S]*?<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '').replace(/<title[^>]*>[\s\S]*?<\/title>/, '').trim();
    return `<symbol id="${id}" viewBox="${vb}"><g fill="none">${inner.replace(/ id="symbol"/, '')}</g></symbol>`;
  };
  const brandSyms = [
    sym('i-logo', 'logo/demido-symbol.svg'),
    ...['pending', 'running', 'done', 'needs-input', 'failed', 'skipped', 'delegated'].map((n) => sym(`i-task-${n}`, `icons/state/task-${n}.svg`)),
  ];
  // lab-drawn helpers (not brand assets): signal glyphs with distinct silhouettes, and four tiny UI marks
  const labSyms = [
    '<symbol id="i-caution" viewBox="0 0 16 16"><path fill="currentColor" fill-rule="evenodd" d="M8 1.6c.56 0 1.07.3 1.34.79l5.3 9.4c.56 1-.16 2.21-1.3 2.21H2.66c-1.14 0-1.86-1.22-1.3-2.21l5.3-9.4C6.93 1.9 7.44 1.6 8 1.6ZM7.2 5.6h1.6v4.1H7.2V5.6ZM8 10.75a.95.95 0 1 0 0 1.9.95.95 0 0 0 0-1.9Z"/></symbol>',
    '<symbol id="i-danger" viewBox="0 0 16 16"><path fill="currentColor" fill-rule="evenodd" d="M5.5 1.25h5a1 1 0 0 1 .7.3l3.25 3.25a1 1 0 0 1 .3.7v5a1 1 0 0 1-.3.7l-3.25 3.25a1 1 0 0 1-.7.3h-5a1 1 0 0 1-.7-.3L1.55 11.2a1 1 0 0 1-.3-.7v-5a1 1 0 0 1 .3-.7L4.8 1.55a1 1 0 0 1 .7-.3ZM5.62 4.5 4.5 5.62 6.88 8 4.5 10.38l1.12 1.12L8 9.12l2.38 2.38 1.12-1.12L9.12 8l2.38-2.38L10.38 4.5 8 6.88 5.62 4.5Z"/></symbol>',
    '<symbol id="i-info" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5"/><path fill="currentColor" d="M7.25 7.1h1.5v4.15h-1.5z"/><circle cx="8" cy="5.05" r=".95" fill="currentColor"/></symbol>',
    '<symbol id="i-lamp" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.1" fill="none" stroke="currentColor" stroke-opacity=".38" stroke-width="1.5"/><circle cx="8" cy="8" r="3.1" fill="currentColor"/></symbol>',
    '<symbol id="i-ring" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.1" fill="none" stroke="currentColor" stroke-width="1.5"/></symbol>',
    '<symbol id="i-dot" viewBox="0 0 16 16"><circle cx="8" cy="8" r="4.5" fill="currentColor"/></symbol>',
    '<symbol id="i-plus" viewBox="0 0 16 16"><path d="M8 3.25v9.5M3.25 8h9.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></symbol>',
    '<symbol id="i-chev" viewBox="0 0 16 16"><path d="m4.5 6.25 3.5 3.5 3.5-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></symbol>',
    '<symbol id="i-up" viewBox="0 0 16 16"><path d="M8 12.75v-9.5M3.75 7.5 8 3.25l4.25 4.25" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></symbol>',
  ];
  const sprite = `<svg width="0" height="0" style="position:absolute" aria-hidden="true"><defs>\n${[...brandSyms, ...labSyms].join('\n')}\n</defs></svg>`;
  const page = readFileSync(join(HERE, 'specimen.src.html'), 'utf8').replace('<!--SPRITE-->', sprite);
  writeFileSync(join(HERE, 'specimen.html'), page);
  console.log('wrote out/tokens.json, out/tokens.css, out/meta.js, out/tables.md, out/recommended.md, specimen.html');
}
