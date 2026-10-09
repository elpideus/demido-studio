// What the composer's context ring shows: how far a chat is from being compacted, in a word and
// in a sentence. The numbers come from the backend (`context_usage`), measured as a turn measures
// them before compacting; the message being written is added here, estimated the same way.

import type { ContextUsage } from '@/lib/types';
import { formatTokens } from '@/lib/format';

/** How near the ring is to full. */
export type ContextLevel = 'low' | 'near' | 'full';

/** Shares of the way to full at which the ring turns amber, then red. */
export const NEAR = 0.75;
export const FULL = 0.92;

/** Tokens of `text` as the backend estimates them: one per three bytes of UTF-8. */
export function estimateTokens(text: string): number {
  return text ? Math.floor(new TextEncoder().encode(text).length / 3) + 1 : 0;
}

export interface ContextView {
  /** Tokens of the next request, the message being written included. */
  used: number;
  /** Where the ring is full: where the chat compacts, or with auto-compact off, where the model
   * starts to lose its oldest messages. */
  limit: number;
  /** `used` of `limit`, from 0 to 1. */
  share: number;
  /** `share` in whole percent, 100 only once it is reached. */
  percent: number;
  /** `percent` as shown: `<1%` for a start, so a chat with something in it never reads empty. */
  percentLabel: string;
  level: ContextLevel;
  /** What the tooltip says under the percentage. */
  headline: string;
  /** What happens next, in a sentence or two. */
  outlook: string;
}

/** The view of `usage` with `draft` tokens being written. */
export function contextView(usage: ContextUsage, draft = 0, running = false): ContextView {
  const used = usage.used + draft;
  const room = Math.max(1, usage.window - usage.reserve);
  const limit = Math.max(1, usage.threshold ?? room);
  const share = Math.min(1, used / limit);
  const percent = used >= limit ? 100 : Math.min(99, Math.floor(share * 100));
  const level: ContextLevel = share >= FULL ? 'full' : share >= NEAR ? 'near' : 'low';
  const left = formatTokens(Math.max(0, limit - used));
  const next = running ? 'before the next step' : 'with your next message';
  const summaryNote = 'The earlier messages become a summary the model reads instead.';

  let headline: string;
  let outlook: string;
  if (usage.threshold !== null) {
    if (used < limit) {
      headline = `${left} tokens until auto-compact`;
      outlook = `The chat compacts in about ${left} tokens. ${summaryNote}`;
    } else if (usage.canCompact) {
      headline = `Compacts ${next}`;
      outlook = `The chat compacts ${next}. ${summaryNote}`;
    } else {
      headline = 'Nothing older to compact yet';
      outlook =
        'The chat is past the point where it compacts, but there are no earlier turns long enough to ' +
        'summarize. If the window fills, the model stops seeing the oldest messages.';
    }
  } else if (used < limit) {
    headline = `${left} tokens until the window fills`;
    outlook =
      'Auto-compact is off. Once the window fills, the model stops seeing the oldest messages. ' +
      '/compact summarizes them when you choose.';
  } else {
    headline = 'Window full: oldest messages left out';
    outlook =
      'Auto-compact is off and the window is full, so the model no longer sees the oldest messages. ' +
      '/compact summarizes them so it can.';
  }
  const percentLabel = percent === 0 && used > 0 ? '<1%' : `${percent}%`;
  const warning = usage.threshold !== null ? unreachableNote(usage, model) : null;
  return { used, limit, share, percent, percentLabel, level, headline, outlook, warning };
}
