import type { FileAttachment } from '../types'

/** Mirrors the backend's image test (commands.rs: `mime_type` starts_with "image/"). */
export const isImageAttachment = (a: FileAttachment) => a.mimeType?.startsWith('image/') ?? false

/**
 * Strip image attachments when the target model has no vision support. A text-only model
 * (llama-server without an mmproj) answers image content with a 500, and attachments can
 * reach a blind model two ways the attach button doesn't cover: they survive a model
 * switch, and the conversation cache replays them on every later send.
 *
 * Text attachments are never dropped: those are inlined as text and work anywhere.
 */
export const dropImagesIfBlind = (
  atts: FileAttachment[] = [],
  visionSupported: boolean,
): FileAttachment[] => (visionSupported ? atts : atts.filter(a => !isImageAttachment(a)))
