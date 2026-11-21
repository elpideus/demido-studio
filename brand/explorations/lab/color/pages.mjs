// Demido Studio colour lab: two evidence pages generated from out/tokens.json.
//   out/sheet-cvd.html      accent + the four signals as seen with normal vision, deuteranopia and protanopia
//   out/sheet-palette.html  the three neutral ramps and every accent's states, both themes
// Usage: node pages.mjs   (prints the pixel size each page needs; sheets.sh screenshots them)
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { simulate } from './lib/color.mjs';
import { ACCENTS, ACCENT_ORDER, STEPS, ORDER, TEMPS, SEMANTICS, T } from './palette.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const tok = JSON.parse(readFileSync(join(HERE, 'out', 'tokens.json'), 'utf8'));
const THEMES = ['dark', 'light'];
const base = `*{box-sizing:border-box}html{background:#808080}body{margin:0;font:13px/1.4 system-ui,sans-serif}h1{font-size:20px;margin:0 0 4px;font-weight:650}p{margin:0 0 14px;opacity:.72}
.th{padding:22px 26px 24px}.mono{font-family:ui-monospace,"Noto Sans Mono",monospace;font-size:10.5px}`;

// ---------- CVD page ----------
const GLYPH = {
  accent: '<svg width="14" height="14" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.1" fill="none" stroke="currentColor" stroke-opacity=".38" stroke-width="1.5"/><circle cx="8" cy="8" r="3.1" fill="currentColor"/></svg>',
  success: '<svg width="14" height="14" viewBox="0 0 16 16"><path fill="currentColor" fill-rule="evenodd" d="M8 1a7 7 0 1 1 0 14A7 7 0 0 1 8 1ZM4.25 8.85l2.2 2.2a.85.85 0 0 0 1.24-.04l4.1-4.7a.85.85 0 0 1-1.28-1.12L7.01 9.21 5.45 7.65a.85.85 0 0 1-1.2 1.2Z"/></svg>',
  caution: '<svg width="14" height="14" viewBox="0 0 16 16"><path fill="currentColor" fill-rule="evenodd" d="M8 1.6c.56 0 1.07.3 1.34.79l5.3 9.4c.56 1-.16 2.21-1.3 2.21H2.66c-1.14 0-1.86-1.22-1.3-2.21l5.3-9.4C6.93 1.9 7.44 1.6 8 1.6ZM7.2 5.6h1.6v4.1H7.2V5.6ZM8 10.75a.95.95 0 1 0 0 1.9.95.95 0 0 0 0-1.9Z"/></svg>',
  danger: '<svg width="14" height="14" viewBox="0 0 16 16"><path fill="currentColor" fill-rule="evenodd" d="M5.5 1.25h5a1 1 0 0 1 .7.3l3.25 3.25a1 1 0 0 1 .3.7v5a1 1 0 0 1-.3.7l-3.25 3.25a1 1 0 0 1-.7.3h-5a1 1 0 0 1-.7-.3L1.55 11.2a1 1 0 0 1-.3-.7v-5a1 1 0 0 1 .3-.7L4.8 1.55a1 1 0 0 1 .7-.3ZM5.62 4.5 4.5 5.62 6.88 8 4.5 10.38l1.12 1.12L8 9.12l2.38 2.38 1.12-1.12L9.12 8l2.38-2.38L10.38 4.5 8 6.88 5.62 4.5Z"/></svg>',
  info: '<svg width="14" height="14" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5"/><path fill="currentColor" d="M7.25 7.1h1.5v4.15h-1.5z"/><circle cx="8" cy="5.05" r=".95" fill="currentColor"/></svg>',
};
const WORD = { accent: 'Live', success: 'OK', caution: 'Caution', danger: 'Danger', info: 'Info' };
function cvdPage() {
  let html = `<!doctype html><meta charset="utf-8"><style>${base}
  .row{display:grid;grid-template-columns:150px repeat(3,420px) 1fr;align-items:center;height:38px;gap:14px}
  .head{height:24px;font-size:11.5px;opacity:.7}.grp{display:flex;gap:6px}
  .chip{display:inline-flex;align-items:center;gap:5px;height:26px;width:78px;padding:0 7px;border-radius:7px;font-size:11.5px;font-weight:600;white-space:nowrap}
  .sw{width:26px;height:26px;border-radius:7px;flex:none}.st{font-size:11.5px;white-space:nowrap}.name{font-weight:600}
  .FAIL{font-weight:700;text-decoration:underline}.WARN{font-weight:700}
  </style>`;
  for (const th of THEMES) {
    const N = tok.neutrals.neutral[th];
    html += `<div class="th" style="background:${N.bg.hex};color:${N.text1.hex}"><h1>Accent against the four signals, ${th} theme</h1>
    <p>Left to right in each group: live accent, success, caution, danger, info, as text and glyph on the raised surface. Simulation: Machado, Oliveira and Fernandes 2009, severity 1.0. Last column: the closest pair that involves the accent (OKLab distance x100: normal / deutan / protan). Floors: normal ${T.dE.normal}, colour-blind ${T.dE.cvd} (WARN from ${T.dE.cvdFloor}).</p>
    <div class="row head"><span>Accent</span><span>Normal vision</span><span>Deuteranopia</span><span>Protanopia</span><span>Closest pair with the accent</span></div>`;
    for (const k of ACCENT_ORDER) {
      const A = tok.accents[k][th], S = tok.semantics[k][th];
      const cols = { accent: A.text.hex, ...Object.fromEntries(SEMANTICS.order.map((s) => [s, S[s].fg.hex])) };
      const grp = (kind) => `<span class="grp">${Object.entries(cols).map(([n, hex]) => { const c = simulate(hex, kind); return `<span class="chip" style="background:${N.raised.hex};color:${c};box-shadow:inset 0 0 0 1px ${N.hairline.hex}">${GLYPH[n]}${WORD[n]}</span>`; }).join('')}</span>`;
      const rows = tok.cvd[k][th].used.slice(0, 4);
      const worst = rows.reduce((a, b) => (b.score < a.score ? b : a));
      const moved = SEMANTICS.order.filter((s) => S[s].shifted);
      html += `<div class="row"><span class="name">${ACCENTS[k].label}</span>${grp('normal')}${grp('deutan')}${grp('protan')}<span class="st"><span class="${worst.status}">${worst.status === 'ok' ? 'pass' : worst.status}</span> · ${worst.pair.replace('accent / ', 'vs ')} ${worst.normal.toFixed(0)} / ${worst.deutan.toFixed(1)} / ${worst.protan.toFixed(1)}${moved.length ? ' · moved: ' + moved.join(', ') : ''}</span></div>`;
    }
    html += '</div>';
  }
  writeFileSync(join(HERE, 'out', 'sheet-cvd.html'), html);
  return [1900, 2 * (22 + 24 + 28 + 40 + 24 + ACCENT_ORDER.length * 38 + 24)];
}

// ---------- palette page ----------
const lum = (hex) => { const c = [1, 3, 5].map((i) => { const v = parseInt(hex.slice(i, i + 2), 16) / 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]; };
const ink = (hex) => (lum(hex) > 0.36 ? '#0e0e0e' : '#ffffff');
function palettePage() {
  let html = `<!doctype html><meta charset="utf-8"><style>${base}
  .ramp{display:grid;grid-template-columns:120px repeat(12,1fr);align-items:stretch;margin-bottom:8px}
  .ramp>span{align-self:center;font-weight:600}.ramp div{height:66px;padding:7px 8px;font-size:11px;line-height:1.3}.ramp b{display:block}
  .roles{display:grid;grid-template-columns:120px repeat(12,1fr);font-size:10.5px;opacity:.7;margin:6px 0 16px}.roles span{padding-right:8px}
  .acc{display:grid;grid-template-columns:150px repeat(6,1fr);gap:8px;align-items:center;margin-bottom:8px}
  .acc i{display:grid;place-items:center;height:40px;border-radius:8px;font-style:normal;font-weight:600;font-size:12px}
  .acc.head{font-size:11.5px;opacity:.7;margin-bottom:4px}.acc .name{font-weight:600}
  h2{font-size:13px;margin:22px 0 10px}
  </style>`;
  for (const th of THEMES) {
    const N = tok.neutrals.neutral[th];
    html += `<div class="th" style="background:${N.bg.hex};color:${N.text1.hex}"><h1>Demido Studio palette, ${th} theme</h1><p>Every value is the 8-bit sRGB result, with the OKLCH of that result. Neutral steps run from the outermost surface to the solid.</p>`;
    for (const tk of Object.keys(TEMPS)) {
      html += `<div class="ramp"><span>${TEMPS[tk].label}</span>${ORDER[th].map((k, i) => { const c = tok.neutrals[tk][th][k]; return `<div style="background:${c.hex};color:${ink(c.hex)}"><b>${i + 1} ${k}</b><span class="mono">${c.hex}<br>${c.oklch.replace('oklch(', '').replace(')', '')}</span></div>`; }).join('')}</div>`;
    }
    html += `<div class="roles"><span></span>${ORDER[th].map((k) => `<span>${STEPS.find((x) => x[0] === k)[1]}</span>`).join('')}</div>`;
    html += `<h2>Accent candidates</h2><div class="acc head"><span></span><span>Base (mark and fill)</span><span>Hover</span><span>Pressed</span><span>Subtle tint</span><span>As text, on raised</span><span>As text, on tint</span></div>`;
    for (const k of ACCENT_ORDER) {
      const A = tok.accents[k][th];
      const fill = (c) => `<i style="background:${c.hex};color:${A.onAccent}"><span class="mono">${c.hex}</span></i>`;
      html += `<div class="acc"><span class="name">${ACCENTS[k].label}</span>${fill(A.base)}${fill(A.hover)}${fill(A.pressed)}<i style="background:${A.tint.hex};color:${N.text2.hex};box-shadow:inset 0 0 0 1px ${A.line.hex}"><span class="mono">${A.tint.hex}</span></i><i style="background:${N.raised.hex};color:${A.text.hex};box-shadow:inset 0 0 0 1px ${N.hairline.hex}">Loaded <span class="mono">&nbsp;${A.text.hex}</span></i><i style="background:${A.tint.hex};color:${A.text.hex};box-shadow:inset 0 0 0 1px ${A.line.hex}">Loaded</i></div>`;
    }
    const S = tok.semanticsBase[th];
    html += `<h2>Signals (base set)</h2><div class="acc">${['<span class="name">Text and glyph on tint</span>', ...SEMANTICS.order.map((s) => `<i style="background:${S[s].tint.hex};color:${S[s].fg.hex};box-shadow:inset 0 0 0 1px ${S[s].line.hex};grid-auto-flow:column;gap:6px">${GLYPH[s]}${SEMANTICS.meta[s].label} <span class="mono">${S[s].fg.hex}</span></i>`)].join('')}</div>`;
    html += `<div class="acc">${['<span class="name">Solid fill</span>', ...SEMANTICS.order.map((s) => `<i style="background:${S[s].solid.hex};color:${S[s].onSolid}">${SEMANTICS.meta[s].label} <span class="mono">&nbsp;${S[s].solid.hex}</span></i>`)].join('')}</div>`;
    html += '</div>';
  }
  writeFileSync(join(HERE, 'out', 'sheet-palette.html'), html);
  return [1900, 2 * (22 + 24 + 34 + 3 * 74 + 46 + 44 + 24 + ACCENT_ORDER.length * 48 + 44 + 2 * 48 + 30)];
}
const a = cvdPage(), b = palettePage();
console.log(`sheet-cvd ${a[0]} ${a[1]}\nsheet-palette ${b[0]} ${b[1]}`);
