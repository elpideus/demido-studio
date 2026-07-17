import type { Artifact } from '../types'

const AUTO_PROMOTE_LINES = 15

const LANG_EXT: Record<string, string> = {
  html: '.html', css: '.css', javascript: '.js', js: '.js',
  typescript: '.ts', ts: '.ts', tsx: '.tsx', jsx: '.jsx',
  python: '.py', py: '.py', rust: '.rs', go: '.go',
  java: '.java', sql: '.sql', json: '.json', jsonc: '.jsonc', json5: '.json5', markdown: '.md',
  md: '.md', yaml: '.yaml', yml: '.yml', shell: '.sh',
  bash: '.sh', sh: '.sh', c: '.c', cpp: '.cpp',
  mermaid: '.mmd', latex: '.tex', tex: '.tex',
}

export function getExtension(type: string): string {
  return LANG_EXT[type.toLowerCase()] ?? '.txt'
}

/** Artifact type for a filename, so files off disk render like artifacts of that language. */
export function getTypeForFile(name: string): string {
  const dot = name.lastIndexOf('.')
  if (dot === -1) return 'text'
  const ext = name.slice(dot).toLowerCase()
  const hit = Object.entries(LANG_EXT).find(([, e]) => e === ext)
  return hit ? hit[0] : 'text'
}

export interface ParsedSegment {
  text?: string
  artifact?: Artifact
}

export function parseArtifacts(content: string, messageId: string): ParsedSegment[] {
  const segments = extractXmlArtifacts(content, messageId)
  return segments.flatMap(seg => {
    if (seg.artifact) return [seg]
    if (!seg.text) return []
    return extractCodeBlockArtifacts(seg.text, messageId, seg)
  })
}

function extractXmlArtifacts(content: string, messageId: string): ParsedSegment[] {
  const segments: ParsedSegment[] = []
  const re = /<artifact\s+([^>]*)>([\s\S]*?)<\/artifact>/gi
  let lastIndex = 0
  let idx = 0
  let match: RegExpExecArray | null

  while ((match = re.exec(content)) !== null) {
    if (match.index > lastIndex) segments.push({ text: content.slice(lastIndex, match.index) })
    const attrs = match[1]
    const body = match[2]
    const typeM = /type="([^"]*)"/.exec(attrs)
    const titleM = /title="([^"]*)"/.exec(attrs)
    const identM = /identifier="([^"]*)"/.exec(attrs)
    segments.push({
      artifact: {
        id: `${messageId}-x${idx++}`,
        messageId,
        type: typeM?.[1] ?? 'text',
        title: titleM?.[1] ?? 'Artifact',
        content: body.trim(),
        identifier: identM?.[1],
      },
    })
    lastIndex = match.index + match[0].length
  }

  if (lastIndex < content.length) segments.push({ text: content.slice(lastIndex) })
  return segments
}

function extractCodeBlockArtifacts(text: string, messageId: string, parentSeg: ParsedSegment): ParsedSegment[] {
  const segments: ParsedSegment[] = []
  // Use a unique prefix derived from parent to keep IDs stable
  const prefix = parentSeg.text?.slice(0, 8).replace(/\W/g, '') ?? 'cb'
  const re = /^```(\w+)\n([\s\S]*?)^```/gm
  let lastIndex = 0
  let idx = 0
  let match: RegExpExecArray | null

  while ((match = re.exec(text)) !== null) {
    const lang = match[1]
    const body = match[2]
    const lineCount = body.split('\n').length

    const alwaysPromote = false
    if (lang && (alwaysPromote || lineCount >= AUTO_PROMOTE_LINES)) {
      if (match.index > lastIndex) segments.push({ text: text.slice(lastIndex, match.index) })
      const title = lang.charAt(0).toUpperCase() + lang.slice(1)
      segments.push({
        artifact: {
          id: `${messageId}-${prefix}-c${idx++}`,
          messageId,
          type: lang,
          title,
          content: body.trimEnd(),
        },
      })
      lastIndex = match.index + match[0].length
    }
  }

  if (lastIndex < text.length) segments.push({ text: text.slice(lastIndex) })
  return segments.length > 1 || segments.some(s => s.artifact) ? segments : [{ text }]
}

export interface StreamingArtifactHint {
  title: string
  type: string
  complete: boolean
}

export function parseStreamingSegments(content: string): Array<{ text?: string; artifactHint?: StreamingArtifactHint }> {
  const artifactIdx = content.indexOf('<artifact')
  const codeMatch = /^```(\w+)\n/m.exec(content)
  const codeIdx = codeMatch ? codeMatch.index : -1

  const hasArtifact = artifactIdx !== -1
  const hasCode = codeIdx !== -1
  if (!hasArtifact && !hasCode) return content ? [{ text: content }] : []

  const segments: Array<{ text?: string; artifactHint?: StreamingArtifactHint }> = []

  // Whichever opening appears first
  if (hasArtifact && (!hasCode || artifactIdx <= codeIdx)) {
    if (artifactIdx > 0) segments.push({ text: content.slice(0, artifactIdx) })
    const rest = content.slice(artifactIdx)
    const closeIdx = rest.indexOf('</artifact>')
    const attrMatch = /<artifact\s+([^>]*)/.exec(rest)
    const attrs = attrMatch?.[1] ?? ''
    const title = /title="([^"]*)"/.exec(attrs)?.[1] ?? 'Artifact'
    const type = /type="([^"]*)"/.exec(attrs)?.[1] ?? 'text'
    if (closeIdx === -1) {
      segments.push({ artifactHint: { title, type, complete: false } })
    } else {
      segments.push({ artifactHint: { title, type, complete: true } })
      const after = rest.slice(closeIdx + '</artifact>'.length)
      if (after) segments.push(...parseStreamingSegments(after))
    }
  } else {
    // Code fence wins
    const lang = codeMatch![1]
    const openLen = codeMatch![0].length
    if (codeIdx > 0) segments.push({ text: content.slice(0, codeIdx) })
    const afterOpen = content.slice(codeIdx + openLen)
    const closeMatch = /^```\s*$/m.exec(afterOpen)
    const title = lang.charAt(0).toUpperCase() + lang.slice(1)
    if (!closeMatch) {
      // Fence still open, show spinner regardless of line count
      segments.push({ artifactHint: { title, type: lang, complete: false } })
    } else {
      const body = afterOpen.slice(0, closeMatch.index)
      const lineCount = body.split('\n').length
      if (lineCount >= AUTO_PROMOTE_LINES) {
        segments.push({ artifactHint: { title, type: lang, complete: true } })
      } else {
        // Won't be promoted, render as plain text
        const raw = content.slice(codeIdx, codeIdx + openLen + closeMatch.index + closeMatch[0].length)
        segments.push({ text: raw })
      }
      const after = afterOpen.slice(closeMatch.index + closeMatch[0].length)
      if (after) segments.push(...parseStreamingSegments(after))
    }
  }

  return segments
}

export const ARTIFACT_INSTRUCTIONS = `When your response includes a complete, self-contained artifact (code, script, HTML page, markdown document, data file) that is ${AUTO_PROMOTE_LINES}+ lines long, wrap it in an artifact tag:

<artifact type="TYPE" title="TITLE" identifier="IDENTIFIER">
content here
</artifact>

Supported types: html, css, javascript, typescript, python, rust, go, java, sql, json, markdown, bash, yaml, c, cpp, mermaid, latex.
Use artifacts for substantial reusable content. Do NOT use them for short inline snippets or illustrative examples.

CRITICAL: when modifying an existing artifact:
- Use the EXACT SAME title and identifier as the original artifact.
- Do NOT create a new artifact with a different title. The user expects to see an updated version of the same artifact, not a new one.
- If the user asks to edit, update, modify, fix, or improve an artifact from earlier in the conversation, always output it with the same title and identifier.

For new artifacts, choose a short descriptive title and a kebab-case identifier (e.g. identifier="digital-rain"). Reuse that identifier on every subsequent edit.`
