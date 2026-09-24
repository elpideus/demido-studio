// Bundles the market service and its dependencies into one ESM file, so the installed app
// needs only Node.js to run it: no node_modules, no npm at install time.
//
//   node build.mjs [outfile]      default: ../../.dev/resources/sidecars/market.mjs

import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { build } from 'esbuild';

const here = path.dirname(fileURLToPath(import.meta.url));
const outfile = path.resolve(process.argv[2] ?? path.join(here, '../../.dev/resources/sidecars/market.mjs'));
mkdirSync(path.dirname(outfile), { recursive: true });

await build({
  entryPoints: [path.join(here, 'src/main.ts')],
  outfile,
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node22',
  minify: false,
  legalComments: 'eof',
  // CommonJS dependencies call require(); give the ESM bundle one.
  banner: {
    js: "import { createRequire as __demidoRequire } from 'node:module'; const require = __demidoRequire(import.meta.url);",
  },
  logLevel: 'warning',
});
console.log(`market service bundled to ${outfile}`);
