import { describe, it, expect } from 'vitest'
import { dropImagesIfBlind, isImageAttachment } from './attachments'
import type { FileAttachment } from '../types'

const img: FileAttachment = { name: 'shot.png', content: 'data:image/png;base64,AAAA', mimeType: 'image/png' }
const txt: FileAttachment = { name: 'notes.md', content: '# hi', mimeType: 'text/markdown' }
const bare: FileAttachment = { name: 'blob', content: 'x' }

describe('isImageAttachment', () => {
  it('matches image mime types only', () => {
    expect(isImageAttachment(img)).toBe(true)
    expect(isImageAttachment(txt)).toBe(false)
    expect(isImageAttachment(bare)).toBe(false)
  })
})

describe('dropImagesIfBlind', () => {
  it('keeps images when the model has vision', () => {
    expect(dropImagesIfBlind([img, txt], true)).toEqual([img, txt])
  })

  it('drops images when the model is text-only', () => {
    expect(dropImagesIfBlind([img, txt], false)).toEqual([txt])
  })

  it('never drops text attachments', () => {
    expect(dropImagesIfBlind([txt, bare], false)).toEqual([txt, bare])
  })

  it('handles the undefined (no cached attachments) case', () => {
    expect(dropImagesIfBlind(undefined, false)).toEqual([])
  })
})
