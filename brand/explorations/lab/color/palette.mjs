// Demido Studio colour lab: the palette SPEC (designer intent, in OKLCH).
// build.mjs resolves this to sRGB, measures it, and fixes what fails.
// Nothing here is a final value: every number is re-measured after 8-bit rounding.

// ---------- neutral ramp: 12 steps ----------
// key, role, dark L, light L, chroma weight (multiplied by the temperature's base chroma)
// Order is the dark theme's lightness order. In the light theme the same 12 roles run the other way.
export const STEPS = [
  ['sunken',   'Sunken (sidebar, wells, code)',        0.168, 0.955, 1.0],
  ['bg',       'App background',                       0.192, 0.975, 1.0],
  ['raised',   'Raised surface (cards, composer)',     0.222, 0.992, 1.0],
  ['overlay',  'Overlay (menus, dialogs)',             0.252, 1.000, 1.0],
  ['fill',     'Interactive fill (hover, selected)',   0.288, 0.928, 1.1],
  ['hairline', 'Hairline border (dividers, cards)',    0.325, 0.890, 1.3],
  ['disabled', 'Disabled text and icons',              0.455, 0.720, 1.5],
  ['border',   'Strong border (control boundaries)',   0.540, 0.620, 1.6],
  ['text3',    'Tertiary text (pending, captions)',    0.640, 0.520, 1.6],
  ['text2',    'Secondary text',                       0.770, 0.410, 1.5],
  ['text1',    'Primary text',                         0.930, 0.220, 1.0],
  ['solid',    'Solid (logo, mono primary fill)',      0.985, 0.165, 0.5],
];

// Display order per theme, so each ramp reads monotonically (dark: deepest surface first; light: whitest surface first).
export const ORDER = {
  dark: ['sunken', 'bg', 'raised', 'overlay', 'fill', 'hairline', 'disabled', 'border', 'text3', 'text2', 'text1', 'solid'],
  light: ['overlay', 'raised', 'bg', 'sunken', 'fill', 'hairline', 'disabled', 'border', 'text3', 'text2', 'text1', 'solid'],
};

// Three temperatures. c is the base chroma of surfaces; text and borders scale it by cw.
export const TEMPS = {
  neutral: { label: 'True neutral', h: 0, cDark: 0, cLight: 0 },
  warm: { label: 'Warm (h 80)', h: 80, cDark: 0.0055, cLight: 0.0045 },
  cool: { label: 'Cool (h 255)', h: 255, cDark: 0.0075, cLight: 0.0050 },
};

// Surfaces a text or border step must hold against, per theme.
export const SURFACES = ['sunken', 'bg', 'raised', 'overlay'];

// ---------- accent candidates ----------
// dark: the "live" lamp on the dark cockpit.
// light: the designer's first guess for the light theme: the hue at a lightness where it still
// looks like itself. build.mjs darkens it until it holds 3:1 as a mark and reports how far it moved.
export const ACCENTS = {
  lime:    { label: 'Lime',            note: 'a "live" green-yellow',                 dark: [0.905, 0.215, 124], light: [0.74, 0.20, 127] },
  green:   { label: 'Signal green',    note: 'indicator-lamp green',                  dark: [0.805, 0.200, 150], light: [0.64, 0.19, 150] },
  mint:    { label: 'Mint',            note: 'added: a calmer, rounder green',        dark: [0.860, 0.130, 168], light: [0.62, 0.12, 170] },
  cyan:    { label: 'Aqua / cyan',     note: 'cool and luminous',                     dark: [0.855, 0.130, 196], light: [0.66, 0.115, 205] },
  blue:    { label: 'Ion blue',        note: 'the default focus hue',                 dark: [0.780, 0.120, 250], light: [0.56, 0.195, 256] },
  orchid:  { label: 'Orchid',          note: 'added: a softer, cooler magenta',       dark: [0.800, 0.140, 318], light: [0.55, 0.215, 320] },
  magenta: { label: 'Magenta line',    note: 'the active route on a flight display',  dark: [0.780, 0.185, 338], light: [0.56, 0.246, 338] },
  orange:  { label: 'Flight orange',   note: 'international orange',                  dark: [0.700, 0.200, 42],  light: [0.64, 0.200, 42] },
  yellow:  { label: 'Tungsten yellow', note: 'a warm filament',                       dark: [0.880, 0.140, 87],  light: [0.74, 0.15, 82] },
  mono:    { label: 'Monochrome',      note: 'no hue: white on dark, black on light', dark: [0.985, 0, 0],       light: [0.165, 0, 0] },
};
export const ACCENT_ORDER = ['lime', 'green', 'mint', 'cyan', 'blue', 'orchid', 'magenta', 'orange', 'yellow', 'mono'];

// ---------- semantic set ----------
// Each signal has a hue window that keeps its meaning and a lightness window that keeps it legible.
// build.mjs searches the grid for the set whose worst pair is furthest apart under normal vision,
// deuteranopia and protanopia. Under red-green CVD hue collapses and only lightness and the
// blue-yellow axis are left, so the search ends up staggering lightness (caution lightest, danger darkest).
// For each accent the search runs again with the accent in the set, and moves a signal only if it must.
export const SEMANTICS = {
  order: ['success', 'caution', 'danger', 'info'],
  meta: {
    success: { label: 'Success', glyph: 'check in a disc' },
    caution: { label: 'Caution', glyph: 'triangle with !' },
    danger:  { label: 'Danger',  glyph: 'octagon with x' },
    info:    { label: 'Info',    glyph: 'i in a ring' },
  },
  grid: {
    dark: {
      success: { C: 0.165, L: [0.74, 0.77, 0.80, 0.83, 0.86], h: [140, 146, 153, 161, 169] },
      caution: { C: 0.150, L: [0.83, 0.86, 0.89, 0.92], h: [66, 72, 80, 88, 96] },
      danger:  { C: 0.190, L: [0.66, 0.69, 0.72, 0.75], h: [14, 20, 26, 32] },
      info:    { C: 0.125, L: [0.70, 0.73, 0.76, 0.79, 0.82], h: [226, 236, 246, 256, 266, 276] },
    },
    light: {
      success: { C: 0.140, L: [0.40, 0.43, 0.46, 0.49, 0.52, 0.55], h: [140, 146, 153, 161, 169] },
      caution: { C: 0.120, L: [0.47, 0.50, 0.53, 0.56, 0.59], h: [56, 64, 72, 80, 88] },
      danger:  { C: 0.210, L: [0.40, 0.43, 0.46, 0.49, 0.52, 0.55], h: [14, 20, 26, 32] },
      info:    { C: 0.160, L: [0.38, 0.42, 0.46, 0.50, 0.54], h: [230, 240, 250, 260, 270] },
    },
  },
  // the canonical value in each window (ties and "stay near base" are measured from here)
  home: {
    dark:  { success: [0.80, 153], caution: [0.86, 88], danger: [0.72, 26], info: [0.76, 246] },
    light: { success: [0.52, 153], caution: [0.56, 72], danger: [0.52, 26], info: [0.50, 250] },
  },
};

// thresholds
export const T = {
  body: { wcag: 4.5, lc: 75 },        // primary text (brief)
  secondary: { wcag: 4.5, lc: 60 },   // 4.5:1 is the brief; Lc 60 is the lab's own target
  tertiary: { wcag: 4.5, lc: 45 },    // lab's own targets: tertiary is still text (pending items, captions)
  boundary: { wcag: 3.0 },            // UI component boundaries, state marks, focus ring (brief)
  accentText: { wcag: 4.5, lc: 60 },
  onAccent: { wcag: 4.5, lc: 60 },
  semText: { wcag: 4.5, lc: 45 },     // signals are a glyph plus a short bold label, never body text; Lc 45 is the lab's own floor
  // colour-difference floors, OKLab Euclidean distance x100 (the dataviz validator's scale)
  dE: { normal: 15, cvd: 8, cvdFloor: 6 },
};

// Reference categorical slots (the validated default set from the dataviz skill), used for chart series 2-4.
export const DATA_SLOTS = [
  { name: 'blue', light: '#2a78d6', dark: '#3987e5' },
  { name: 'orange', light: '#eb6834', dark: '#d95926' },
  { name: 'aqua', light: '#1baf7a', dark: '#199e70' },
  { name: 'yellow', light: '#eda100', dark: '#c98500' },
  { name: 'magenta', light: '#e87ba4', dark: '#d55181' },
  { name: 'green', light: '#008300', dark: '#0ca30c' },
  { name: 'violet', light: '#6250d6', dark: '#9085e9' },
  { name: 'red', light: '#e34948', dark: '#e66767' },
];

// Neighbour brand colours, from public marks and UIs as the lab remembers them (approximate; not fetched).
export const NEIGHBOURS = [
  { name: 'Claude (terracotta)', hex: '#d97757' },
  { name: 'ChatGPT (legacy green)', hex: '#10a37f' },
  { name: 'Gemini (blue)', hex: '#4285f4' },
  { name: 'Gemini (violet end)', hex: '#9b72cb' },
  { name: 'LM Studio (indigo-violet)', hex: '#6c5ce7' },
  { name: 'AnythingLLM (sky)', hex: '#46c8ff' },
  { name: 'Jan (blue)', hex: '#2563eb' },
  { name: 'Msty (violet)', hex: '#7c3aed' },
  { name: 'OpenCode (peach)', hex: '#fab283' },
  { name: 'NVIDIA (green)', hex: '#76b900' },
  { name: 'T-Mobile (magenta)', hex: '#e20074' },
  { name: 'Ollama / Open WebUI / Cursor (mono)', hex: '#ffffff' },
];

// The lab's recommendation (set after looking at the sheets; build.mjs prints the kit for it).
export const RECOMMEND = { neutral: 'neutral', accent: 'magenta', primary: 'mono', runnerUp: 'cyan' };

// Scan mode: DEMIDO_SCAN='{"m340":{"label":"m340","dark":[0.78,0.19,340],"light":[0.56,0.25,340]}}' node build.mjs --verbose
// adds throwaway accent variants so they can be measured without touching the spec. Nothing is written in scan mode.
export const SCAN = !!process.env.DEMIDO_SCAN;
if (SCAN) for (const [k, v] of Object.entries(JSON.parse(process.env.DEMIDO_SCAN))) { ACCENTS[k] = { note: 'scan', ...v }; ACCENT_ORDER.push(k); }
