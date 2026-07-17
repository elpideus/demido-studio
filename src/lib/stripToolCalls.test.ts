import { describe, it, expect } from 'vitest'
import { stripToolCallMarkup } from './stripToolCalls'

describe('stripToolCallMarkup', () => {
  const BLOCK =
    '<tool_call> <function=graphify_query> <parameter=kind> query </parameter> ' +
    '<parameter=query> main entry points and core modules </parameter> </function> </tool_call>'

  it('leaves text without tool_call markup untouched', () => {
    expect(stripToolCallMarkup('Just a normal answer.')).toBe('Just a normal answer.')
  })

  it('removes a lone tool_call block entirely', () => {
    expect(stripToolCallMarkup(BLOCK)).toBe('')
  })

  it('removes a tool_call block but keeps surrounding prose', () => {
    const text = `Here is what I found.\n\n${BLOCK}\n\nDone.`
    expect(stripToolCallMarkup(text)).toBe('Here is what I found.\n\nDone.')
  })

  it('removes multiple blocks', () => {
    const text = `${BLOCK}\n${BLOCK}\nresult`
    expect(stripToolCallMarkup(text)).toBe('result')
  })

  it('keeps an unclosed trailing block in non-streaming mode', () => {
    const partial = 'Thinking…\n<tool_call> <function=x'
    expect(stripToolCallMarkup(partial)).toBe('Thinking…\n<tool_call> <function=x')
  })

  it('drops an unclosed trailing block while streaming', () => {
    const partial = 'Thinking…\n<tool_call> <function=x'
    expect(stripToolCallMarkup(partial, true)).toBe('Thinking…')
  })
})
