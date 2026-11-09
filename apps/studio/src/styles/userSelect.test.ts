import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const ROOTS = ['..', '../../../../packages/ui/src'].map((dir) => fileURLToPath(new URL(dir, import.meta.url)));

function cssFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === 'node_modules' ? [] : cssFiles(path);
    return entry.name.endsWith('.css') ? [path] : [];
  });
}

describe('user-select', () => {
  // WebKitGTK ignores the unprefixed property, so on Linux the whole UI turns selectable without it.
  it('always comes with its -webkit- form', () => {
    const missing = ROOTS.flatMap(cssFiles).flatMap((file) =>
      readFileSync(file, 'utf8')
        .split('\n')
        .flatMap((line, i, lines) =>
          /^\s*user-select:/.test(line) && !/^\s*-webkit-user-select:/.test(lines[i - 1] ?? '')
            ? [`${file}:${i + 1}`]
            : [],
        ),
    );
    expect(missing).toEqual([]);
  });
});
