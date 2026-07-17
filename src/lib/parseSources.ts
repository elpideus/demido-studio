export interface Source {
  label: string
  url: string
  /** Hostname without `www.`, used for the favicon and as a label fallback. */
  domain: string
}

export interface SplitSources {
  /** The message with the sources footer removed. */
  body: string
  /** Sources in the order the model listed them; empty when there is no footer. */
  sources: Source[]
}

// `Sources:` on its own line — bare, bold, or as a heading, since models drift on the decoration
// even when the rider is explicit. Everything after it must be link bullets to count as a footer.
const HEADING_RE = /^\s*(?:#{1,6}\s*)?(?:\*\*|__)?\s*sources?\s*:?\s*(?:\*\*|__)?\s*$/i
const BULLET_RE = /^\s*(?:[-*+]|\d+[.)])\s*\[([^\]]+)\]\(\s*(\S+?)\s*\)\s*$/

function domainOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return ''
  }
}

/**
 * Peels a trailing sources footer off an assistant message so it can render as chips instead of
 * a plain markdown list.
 *
 * Only the *tail* of the message is considered: a "Sources:" list in the middle is part of the
 * answer (the model quoting a page, writing docs about this very feature) and is left in the body.
 * Only `http(s)` links are lifted — anything else in the list means the block is not a footer, so
 * the whole thing stays in the body rather than rendering half of it twice.
 */
export function splitSources(content: string): SplitSources {
  const lines = content.split('\n')

  let i = lines.length
  while (i > 0 && lines[i - 1].trim() === '') i--

  const bullets: Source[] = []
  while (i > 0 && BULLET_RE.test(lines[i - 1])) {
    const [, label, url] = BULLET_RE.exec(lines[i - 1])!
    if (!/^https?:\/\//i.test(url)) return { body: content, sources: [] }
    const domain = domainOf(url)
    bullets.unshift({ label: label.trim() || domain || url, url, domain })
    i--
  }

  // Dedupe over the list in reading order, so a repeated url keeps its first label and position.
  const seen = new Set<string>()
  const sources = bullets.filter(s => {
    const key = s.url.toLowerCase()
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })

  if (!sources.length || i === 0 || !HEADING_RE.test(lines[i - 1])) {
    return { body: content, sources: [] }
  }

  return { body: lines.slice(0, i - 1).join('\n').trimEnd(), sources }
}
