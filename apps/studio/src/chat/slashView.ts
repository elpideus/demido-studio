// Slash commands in the composer: which commands fit what is typed after a `/`, and which
// command a message runs. The commands themselves run in the backend (see src-tauri/src/slash.rs).

import { formatTokens } from '@/lib/format';
import type { Message, SlashCommand, SummaryStats } from '@/lib/types';

/** The command name being typed: what follows a leading `/` while nothing follows it yet. */
export function typedName(text: string): string | null {
  const match = /^\/(\S*)$/.exec(text.trimStart());
  return match ? match[1]!.toLowerCase() : null;
}

/**
 * The commands that fit `typed`: names starting with it first, then names with a part starting
 * with it (`analyze` finds `market-analysis:analyze`), then names or skills holding it.
 */
export function matchCommands(commands: SlashCommand[], typed: string): SlashCommand[] {
  const query = typed.toLowerCase();
  const rank = (c: SlashCommand): number => {
    if (c.name.startsWith(query)) return 0;
    if (c.name.split(/[:_-]/).some((part) => part.startsWith(query))) return 1;
    if (c.name.includes(query) || (c.skillName ?? '').toLowerCase().includes(query)) return 2;
    return -1;
  };
  return commands
    .map((command, i) => ({ command, rank: rank(command), i }))
    .filter((e) => e.rank >= 0)
    .sort((a, b) => a.rank - b.rank || a.i - b.i)
    .map((e) => e.command);
}

/** The command `text` runs, named by its first word, and what follows the name. */
export function commandIn(commands: SlashCommand[], text: string): { command: SlashCommand; args: string } | null {
  const match = /^\/(\S+)(?:\s+([\s\S]*))?$/.exec(text.trim());
  if (!match) return null;
  const name = match[1]!.toLowerCase();
  const command = commands.find((c) => c.name === name);
  return command ? { command, args: (match[2] ?? '').trim() } : null;
}

/** Whether picking `command` from the list runs it: nothing has to be typed after its name. */
export function runsWhenPicked(command: SlashCommand): boolean {
  return !command.args || command.args.trim().startsWith('[');
}

/** What the composer holds once `command` is picked to be completed. */
export function completion(command: SlashCommand): string {
  return command.args ? `/${command.name} ` : `/${command.name}`;
}

/** How a summary message reads in the chat: one line, and more for its tooltip. */
export function summaryLine(message: Message): { label: string; detail: string | null } {
  const s = (message.stats ?? {}) as SummaryStats;
  switch (message.status) {
    case 'streaming':
    case 'running': {
      const part = s.parts && s.parts > 1 ? ` (part ${s.part ?? 1} of ${s.parts})` : '';
      return { label: `Compacting the conversation${part}…`, detail: null };
    }
    case 'error':
      return { label: 'Could not compact the conversation', detail: message.error };
    case 'cancelled':
      return { label: 'Compacting stopped', detail: 'The conversation was left as it was.' };
    default: {
      const parts = [s.auto ? 'Compacted automatically' : 'Conversation compacted'];
      if (s.messages) parts.push(`${s.messages} ${s.messages === 1 ? 'message' : 'messages'}`);
      if (s.tokensBefore && s.tokensAfter) {
        parts.push(`${formatTokens(s.tokensBefore)} → ${formatTokens(s.tokensAfter)} tokens`);
      }
      const by = [
        s.model && `Summarized by ${s.model}`,
        s.durationMs !== undefined && `in ${(s.durationMs / 1000).toFixed(1)}s`,
      ]
        .filter(Boolean)
        .join(' ');
      return { label: parts.join(' · '), detail: by || null };
    }
  }
}
