/* The files a tool is made of. Every tool is a folder: what models read, what people read, what Demido
   reads to run it. You may edit any of them; an edited tool counts as "not audited" until audited again.
   In the preview the files are written from the tool's fake data. */
import type { Tool } from '@/data/types'

/** Who reads a file, and when. This is what an edit costs. */
export type Reader = 'always' | 'picked' | 'people' | 'runtime'
export const READER: Record<Reader, string> = {
  always: 'Models read this in every conversation',
  picked: 'Models read this only after they pick the tool',
  people: 'For people. Models never see it',
  runtime: 'Demido reads this to run the tool. Models never see it',
}
export interface ToolFile { path: string; text: string; reads: Reader; /** Why it cannot be edited, if it cannot. */ locked?: string }

export const tokens = (text: string) => Math.round(text.split(/\s+/).filter(Boolean).length * 1.3)
/** A file you added yourself: instructions if it is Markdown or text, otherwise something the tool runs. */
export const readerOf = (path: string): Reader => (/\.(md|txt)$/i.test(path) ? 'picked' : 'runtime')

export function filesOf(t: Tool): ToolFile[] {
  const fns = t.functions.map((f) => ({ name: f.name, does: f.summary, effect: f.effect }))
  if (t.kind === 'skill')
    return [
      { path: 'SKILL.md', reads: 'picked', text: `---\nname: ${t.id}\ndescription: ${t.summary}\n---\n\n# ${t.name}\n\n${t.guidance}\n\n## When to use it\n\nUse this skill when the task matches the description above. Read the reference before you write.\n\n## Steps\n\n1. Read \`references/style.md\`.\n2. Do the work in small steps.\n3. Run \`scripts/check.py\` on the result and fix what it reports.\n` },
      { path: 'references/style.md', reads: 'picked', text: `# Style\n\nShort sentences. One idea each.\nName things the same way every time.\nSay what changed and why, not how you felt about it.\n` },
      { path: 'scripts/check.py', reads: 'runtime', text: `#!/usr/bin/env python3\n"""Checks the result. Prints one problem per line; prints nothing when all is well."""\nimport sys\n\nfor n, line in enumerate(open(sys.argv[1], encoding="utf-8"), 1):\n    if len(line) > 100:\n        print(f"{n}: longer than 100 characters")\n` },
    ]
  if (t.kind === 'mcp')
    return [
      { path: 'server.json', reads: 'runtime', text: JSON.stringify({ command: 'npx', args: ['-y', `${t.id}-mcp-server`], env: { API_TOKEN: 'kept in your system keyring' }, start: 'with Demido' }, null, 2) + '\n' },
      { path: 'descriptions.txt', reads: 'always', locked: 'Sent by the server every time it starts, so there is nothing here to edit. Convert the tool to a Demido tool to write what models read.', text: fns.map((f) => `${f.name}\n  ${f.does}`).join('\n\n') + '\n' },
    ]
  return [
    { path: 'index.txt', reads: 'always', text: `${t.name}: ${t.summary}\n` },
    { path: 'guidance.md', reads: 'picked', text: t.guidance.endsWith('\n') ? t.guidance : `${t.guidance}\n` },
    { path: 'functions.json', reads: 'picked', locked: t.kind === 'builtin' ? 'These functions are part of Demido itself. Their guidance is yours to edit; their shape is not.' : undefined, text: JSON.stringify(fns, null, 2) + '\n' },
    { path: 'tool.toml', reads: 'runtime', text: `name = "${t.id}"\nversion = "${t.version}"\npublisher = "${t.publisher}"\n\n[permissions]\ncan = [${t.can.map((c) => `"${c}"`).join(', ')}]\ncannot = [${t.cannot.map((c) => `"${c}"`).join(', ')}]\n\n[start]\nwhen = "on first use"\n` },
    { path: 'README.md', reads: 'people', text: `# ${t.name}\n\n${t.summary}\n\nPublished by ${t.publisher}. Version ${t.version}.\n` },
  ]
}
