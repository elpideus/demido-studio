import { describe, it, expect } from 'vitest'
import { splitSources } from './parseSources'

describe('splitSources', () => {
  it('lifts a trailing sources footer off the body', () => {
    const { body, sources } = splitSources(
      'Answer text.\n\nSources:\n- [Reddit](https://reddit.com/r/a/post)\n- [Medium](https://medium.com/some/article)\n'
    )
    expect(body).toBe('Answer text.')
    expect(sources).toEqual([
      { label: 'Reddit', url: 'https://reddit.com/r/a/post', domain: 'reddit.com' },
      { label: 'Medium', url: 'https://medium.com/some/article', domain: 'medium.com' },
    ])
  })

  it('accepts the decorations models add to the heading', () => {
    for (const heading of ['**Sources:**', '## Sources', 'Sources', '__Source:__']) {
      const { sources } = splitSources(`Text.\n\n${heading}\n- [BBC](https://bbc.co.uk/news/1)`)
      expect(sources).toHaveLength(1)
    }
  })

  it('strips www from the favicon domain', () => {
    const { sources } = splitSources('T\n\nSources:\n- [BBC](https://www.bbc.co.uk/news/1)')
    expect(sources[0].domain).toBe('bbc.co.uk')
  })

  it('dedupes repeated urls, keeping list order', () => {
    const { sources } = splitSources(
      'T\n\nSources:\n- [A](https://x.com/1)\n- [B](https://y.com/2)\n- [C](https://X.com/1)'
    )
    expect(sources.map(s => s.url)).toEqual(['https://x.com/1', 'https://y.com/2'])
  })

  it('leaves a mid-message sources list alone', () => {
    const text = 'Sources:\n- [A](https://a.com/1)\n\nAnd then more prose.'
    expect(splitSources(text)).toEqual({ body: text, sources: [] })
  })

  it('ignores a footer whose links are not http', () => {
    const text = 'T\n\nSources:\n- [Notes](file:///c/notes.md)'
    expect(splitSources(text)).toEqual({ body: text, sources: [] })
  })

  it('needs a heading — bare trailing links are body content', () => {
    const text = 'See:\n- [A](https://a.com/1)'
    expect(splitSources(text)).toEqual({ body: text, sources: [] })
  })

  it('needs links — a bare Sources heading is body content', () => {
    const text = 'Prose.\n\nSources:'
    expect(splitSources(text)).toEqual({ body: text, sources: [] })
  })

  it('passes plain messages through untouched', () => {
    expect(splitSources('Hello.')).toEqual({ body: 'Hello.', sources: [] })
  })

  it('handles a message that is only a footer', () => {
    const { body, sources } = splitSources('Sources:\n- [A](https://a.com/1)')
    expect(body).toBe('')
    expect(sources).toHaveLength(1)
  })
})
