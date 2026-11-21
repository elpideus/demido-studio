/* The files a session can point at with @. Made up for the preview: a real session reads its folder from disk.
   A folder is an object, a file is its size as text. */
type Tree = { [name: string]: Tree | string }

const TREES: Record<string, Tree> = {
  '~/code/billing-service': {
    src: {
      auth: { 'mod.rs': '0.4 KB', 'session.rs': '3.3 KB', 'token.rs': '2.2 KB' },
      billing: { 'invoice.rs': '4.1 KB', 'mod.rs': '0.6 KB', 'tax.rs': '2.8 KB' },
      'lib.rs': '0.8 KB',
      'main.rs': '1.9 KB',
    },
    tests: { 'auth_test.rs': '5.0 KB', 'billing_test.rs': '6.4 KB' },
    docs: { 'CHANGELOG.md': '9.8 KB', 'README.md': '3.2 KB' },
    '.gitignore': '0.1 KB',
    'Cargo.lock': '48 KB',
    'Cargo.toml': '0.9 KB',
  },
  '~/code/demido-studio': {
    apps: { web: { src: { 'main.tsx': '0.5 KB' }, 'package.json': '0.7 KB' } },
    brand: { css: { 'demido.css': '1.1 KB' }, logo: { 'demido-symbol.svg': '1.4 KB' } },
    docs: { 'BRIEF.md': '41 KB', 'DECISIONS.md': '22 KB', 'IDEAS.md': '14 KB', 'PLAN.md': '3.9 KB' },
    'LICENSE': '35 KB',
    'README.md': '1.6 KB',
  },
  '~/Documents/tax-2026': {
    receipts: {
      '2026-01': { 'rail-ticket.pdf': '84 KB', 'software.pdf': '112 KB' },
      '2026-02': { 'hardware.pdf': '201 KB' },
      '2026-03': { 'hotel-lisbon.pdf': '156 KB', 'rail-ticket.pdf': '79 KB' },
    },
    'income.csv': '6.2 KB',
    'summary.md': '2.4 KB',
  },
  '~/Documents/Notes': {
    models: { 'quantization.md': '3.8 KB', 'small-models.md': '5.1 KB' },
    travel: { 'lisbon.md': '2.2 KB' },
    'inbox.md': '0.9 KB',
  },
}

/** Folders the session picker offers. The real one opens the system's folder chooser. */
export const KNOWN_FOLDERS = Object.keys(TREES)

/** A session that has only Demido's own private folder has no folder "of yours" assigned. */
export const isAssigned = (folder: string) => !folder.startsWith('~/Demido/sessions/')
export const folderName = (folder: string) => folder.split('/').filter(Boolean).pop() ?? folder

export interface FileEntry { name: string; dir: boolean; hint: string }

/** What is inside `dir` (for example "src/billing/") of the session folder. Folders first. */
export function listFolder(folder: string, dir: string): FileEntry[] {
  let node: Tree | string | undefined = TREES[folder]
  for (const part of dir.split('/').filter(Boolean)) node = node && typeof node === 'object' ? node[part] : undefined
  if (!node || typeof node !== 'object') return []
  return Object.entries(node)
    .map(([name, v]) => (typeof v === 'string' ? { name, dir: false, hint: v } : { name, dir: true, hint: `${Object.keys(v).length} items` }))
    .sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name))
}
