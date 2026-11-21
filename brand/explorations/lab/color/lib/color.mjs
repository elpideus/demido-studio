// Colour maths for the Demido Studio palette lab.
// OKLCH <-> sRGB, gamut mapping, WCAG 2 contrast, APCA Lc, CVD simulation, OKLab delta E.
// No dependencies.

// ---------- sRGB transfer ----------
export const toLinear = (c) => (c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
export const toGamma = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);

// ---------- OKLab (Bjorn Ottosson, 2020) ----------
export function linearToOklab([r, g, b]) {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}
export function oklabToLinear([L, a, b]) {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}
export const oklchToOklab = ([L, C, h]) => [L, C * Math.cos((h * Math.PI) / 180), C * Math.sin((h * Math.PI) / 180)];
export function oklabToOklch([L, a, b]) {
  const C = Math.hypot(a, b);
  let h = (Math.atan2(b, a) * 180) / Math.PI;
  if (h < 0) h += 360;
  return [L, C, C < 1e-5 ? 0 : h];
}

const inGamut = (rgb, eps = 1e-5) => rgb.every((c) => c >= -eps && c <= 1 + eps);
const clamp01 = (c) => Math.min(1, Math.max(0, c));

// OKLCH -> linear sRGB with chroma reduction (hold L and h) when out of gamut.
export function oklchToLinearMapped([L, C, h]) {
  let lin = oklabToLinear(oklchToOklab([L, C, h]));
  if (inGamut(lin)) return { lin: lin.map(clamp01), C, clipped: false };
  let lo = 0, hi = C;
  for (let i = 0; i < 40; i++) {
    const mid = (lo + hi) / 2;
    if (inGamut(oklabToLinear(oklchToOklab([L, mid, h])))) lo = mid; else hi = mid;
  }
  lin = oklabToLinear(oklchToOklab([L, lo, h])).map(clamp01);
  return { lin, C: lo, clipped: true };
}

export const linToRgb8 = (lin) => lin.map((c) => Math.round(clamp01(toGamma(c)) * 255));
export const rgb8ToHex = (rgb) => '#' + rgb.map((v) => v.toString(16).padStart(2, '0')).join('');
export const hexToRgb8 = (hex) => {
  const h = hex.replace('#', '');
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
};
export const rgb8ToLinear = (rgb) => rgb.map((v) => toLinear(v / 255));
export const hexToOklch = (hex) => oklabToOklch(linearToOklab(rgb8ToLinear(hexToRgb8(hex))));
export const hexToOklab = (hex) => linearToOklab(rgb8ToLinear(hexToRgb8(hex)));

// A colour object: requested OKLCH, the 8-bit sRGB it resolves to, and the OKLCH of that 8-bit value.
export function color(L, C, h) {
  const m = oklchToLinearMapped([L, C, h]);
  const rgb = linToRgb8(m.lin);
  const hex = rgb8ToHex(rgb);
  const [l2, c2, h2] = hexToOklch(hex);
  return { L, C: m.C, h, hex, rgb, clipped: m.clipped, actual: [l2, c2, h2] };
}
export const fmtOklch = (c, dp = 3) => {
  const [L, C, h] = c.actual ?? c;
  return `oklch(${L.toFixed(dp)} ${C.toFixed(dp)} ${C < 0.0015 ? 0 : Math.round(h)})`;
};
// Maximum in-gamut chroma for a given L and h.
export function maxChroma(L, h) {
  let lo = 0, hi = 0.45;
  for (let i = 0; i < 40; i++) {
    const mid = (lo + hi) / 2;
    if (inGamut(oklabToLinear(oklchToOklab([L, mid, h])))) lo = mid; else hi = mid;
  }
  return lo;
}

// ---------- WCAG 2.x ----------
export function wcagLum(hex) {
  const [r, g, b] = rgb8ToLinear(hexToRgb8(hex));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
export function wcag(hexA, hexB) {
  const a = wcagLum(hexA), b = wcagLum(hexB);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

// ---------- APCA (APCA-W3 0.0.98G-4g constants) ----------
// Returns Lc: positive = dark text on light background, negative = light text on dark background.
export function apca(textHex, bgHex) {
  const Y = (hex) => {
    const [r, g, b] = hexToRgb8(hex).map((v) => Math.pow(v / 255, 2.4));
    return 0.2126729 * r + 0.7151522 * g + 0.072175 * b;
  };
  const blkThrs = 0.022, blkClmp = 1.414;
  const soft = (y) => (y > blkThrs ? y : y + Math.pow(blkThrs - y, blkClmp));
  const txt = soft(Y(textHex)), bg = soft(Y(bgHex));
  if (Math.abs(bg - txt) < 0.0005) return 0;
  let out;
  if (bg > txt) {
    const s = (Math.pow(bg, 0.56) - Math.pow(txt, 0.57)) * 1.14;
    out = s < 0.1 ? 0 : s - 0.027;
  } else {
    const s = (Math.pow(bg, 0.65) - Math.pow(txt, 0.62)) * 1.14;
    out = s > -0.1 ? 0 : s + 0.027;
  }
  return out * 100;
}

// ---------- CVD simulation: Machado, Oliveira & Fernandes (2009), severity 1.0, linear RGB ----------
const MACHADO = {
  protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
  deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
  tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.3039]],
};
export function simulate(hex, kind) {
  if (kind === 'normal') return hex;
  const m = MACHADO[kind];
  const lin = rgb8ToLinear(hexToRgb8(hex));
  const out = m.map((row) => clamp01(row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2]));
  return rgb8ToHex(linToRgb8(out));
}
// OKLab Euclidean distance x100 (the scale the dataviz validator thresholds are calibrated on).
export function deltaE(hexA, hexB, kind = 'normal') {
  const a = hexToOklab(simulate(hexA, kind)), b = hexToOklab(simulate(hexB, kind));
  return 100 * Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

// ---------- solvers ----------
// Find the lightness closest to L0 (moving in direction dir = +1 lighter / -1 darker) at which pred(color) holds.
export function solveL(L0, C, h, dir, pred, step = 0.002) {
  let L = L0;
  for (let i = 0; i < 600; i++) {
    const c = color(L, C, h);
    if (pred(c)) return c;
    L += dir * step;
    if (L <= 0 || L >= 1) break;
  }
  return null;
}
// Mix two colours in OKLab (t = share of b), return a colour object.
export function mix(hexA, hexB, t) {
  const a = hexToOklab(hexA), b = hexToOklab(hexB);
  const lab = [0, 1, 2].map((i) => a[i] * (1 - t) + b[i] * t);
  const [L, C, h] = oklabToOklch(lab);
  return color(L, C, h);
}
