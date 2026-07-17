// Some local models (Qwen-family especially) emit tool calls as literal `<tool_call>…</tool_call>`
// markup in the *content* channel instead of the API's structured tool-call field — e.g.
//   <tool_call> <function=graphify_query> <parameter=kind> query </parameter> ... </function> </tool_call>
// When that happens the call is executed via the reasoning-channel recovery path, but the raw markup
// is still sitting in the visible message and renders as prose. Strip it so it never shows.

// A complete block: `<tool_call>` … `</tool_call>` (attributes tolerated, spans newlines).
const TOOL_CALL_BLOCK = /<tool_call\b[^>]*>[\s\S]*?<\/tool_call>/gi
// A trailing block whose `</tool_call>` has not streamed in yet — dropped mid-stream so it never flashes.
const TOOL_CALL_OPEN_TAIL = /<tool_call\b[^>]*>[\s\S]*$/i

/**
 * Remove `<tool_call>…</tool_call>` markup a model leaked into the content channel.
 * With `streaming`, also drops an unclosed trailing block so it doesn't flash before it completes.
 */
export function stripToolCallMarkup(text: string, streaming = false): string {
  if (!text.includes('<tool_call')) return text
  let out = text.replace(TOOL_CALL_BLOCK, '')
  if (streaming) out = out.replace(TOOL_CALL_OPEN_TAIL, '')
  // Collapse the blank gap the removal leaves behind, then trim the edges.
  return out.replace(/\n{3,}/g, '\n\n').trim()
}
