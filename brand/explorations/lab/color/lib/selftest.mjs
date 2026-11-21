// Sanity checks for lib/color.mjs against published reference values.
import { apca, wcag, hexToOklch, color, simulate, deltaE } from './color.mjs';

const near = (a, b, tol) => Math.abs(a - b) <= tol;
let fail = 0;
const check = (name, got, want, tol) => {
  const ok = near(got, want, tol);
  if (!ok) fail++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}: got ${got.toFixed(3)} want ${want}`);
};

// APCA reference pairs published with APCA-W3 0.0.98G-4g
check('APCA #888 on #fff', apca('#888888', '#ffffff'), 63.06, 0.1);
check('APCA #fff on #888', apca('#ffffff', '#888888'), -68.54, 0.1);
check('APCA #000 on #aaa', apca('#000000', '#aaaaaa'), 58.146, 0.1);
check('APCA #aaa on #000', apca('#aaaaaa', '#000000'), -56.24, 0.1);
check('APCA #123 on #def', apca('#112233', '#ddeeff'), 91.67, 0.1);
check('APCA #def on #123', apca('#ddeeff', '#112233'), -93.07, 0.1);
check('APCA #123 on #444', apca('#112233', '#444444'), 8.32, 0.1);
check('APCA #444 on #123', apca('#444444', '#112233'), -7.53, 0.1);

// WCAG
check('WCAG black/white', wcag('#000000', '#ffffff'), 21, 0.001);
check('WCAG #777/#fff', wcag('#777777', '#ffffff'), 4.478, 0.01);
check('WCAG #767676/#fff', wcag('#767676', '#ffffff'), 4.54, 0.01);

// OKLCH reference values (CSS Color 4 / oklch.com)
const [L1, C1, h1] = hexToOklch('#ff0000');
check('OKLCH red L', L1, 0.628, 0.001); check('OKLCH red C', C1, 0.2577, 0.001); check('OKLCH red h', h1, 29.23, 0.05);
const [L2, C2, h2] = hexToOklch('#0000ff');
check('OKLCH blue L', L2, 0.452, 0.001); check('OKLCH blue C', C2, 0.3132, 0.001); check('OKLCH blue h', h2, 264.05, 0.05);
check('OKLCH #121212 L', hexToOklch('#121212')[0], 0.1822, 0.001);

// round trip
const c = color(0.7, 0.15, 250);
check('round trip L', c.actual[0], 0.7, 0.004); check('round trip h', c.actual[2], 250, 1.2);

// CVD: red and green collapse for a deuteranope much more than for normal vision
console.log('sim deutan #ff0000 ->', simulate('#ff0000', 'deutan'), ' #00ff00 ->', simulate('#00ff00', 'deutan'));
console.log('dE red/green normal', deltaE('#e5484d', '#30a46c').toFixed(1), 'deutan', deltaE('#e5484d', '#30a46c', 'deutan').toFixed(1), 'protan', deltaE('#e5484d', '#30a46c', 'protan').toFixed(1));
console.log(fail ? `${fail} FAILED` : 'all checks passed');
process.exit(fail ? 1 : 0);
