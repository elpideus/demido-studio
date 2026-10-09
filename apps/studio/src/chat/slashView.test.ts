import { describe, expect, it } from 'vitest';

import type { Message, SlashCommand } from '@/lib/types';
import { commandIn, completion, matchCommands, runsWhenPicked, summaryLine, typedName } from './slashView';

function command(name: string, args: string | null = null, skillName: string | null = null): SlashCommand {
  return { name, description: '', args, skill: skillName ? 'skill' : null, skillName };
}

const commands = [
  command('compact', '[what to keep]'),
  command('autocompact', '<tokens | off | auto>'),
  command('analyze', '<symbol> [timeframe]', 'Market analysis'),
  command('market-analysis:compact', null, 'Market analysis'),
];

function summary(fields: Partial<Message>): Message {
  return {
    id: 's',
    chatId: 'chat',
    seq: 1,
    role: 'summary',
    content: '',
    attachments: [],
    reasoning: null,
    toolCalls: [],
    toolCallId: null,
    toolName: null,
    toolResult: null,
    modelId: null,
    status: 'done',
    error: null,
    stats: null,
    providerMeta: null,
    createdAt: 0,
    ...fields,
  };
}

describe('slash commands', () => {
  it('reads the name while it is being typed', () => {
    expect(typedName('/')).toBe('');
    expect(typedName('/Comp')).toBe('comp');
    expect(typedName('  /comp')).toBe('comp');
    expect(typedName('/compact ')).toBeNull();
    expect(typedName('/compact keep the plan')).toBeNull();
    expect(typedName('hello /comp')).toBeNull();
  });

  it('lists names that start with what was typed first', () => {
    const names = (typed: string) => matchCommands(commands, typed).map((c) => c.name);
    expect(names('')).toEqual(['compact', 'autocompact', 'analyze', 'market-analysis:compact']);
    expect(names('comp')).toEqual(['compact', 'market-analysis:compact', 'autocompact']);
    expect(names('market')).toEqual(['market-analysis:compact', 'analyze']);
    expect(names('zzz')).toEqual([]);
  });

  it('finds the command a message runs', () => {
    expect(commandIn(commands, '/compact')).toEqual({ command: commands[0], args: '' });
    expect(commandIn(commands, '/AutoCompact  12.5k ')).toEqual({ command: commands[1], args: '12.5k' });
    expect(commandIn(commands, '/analyze BTCUSD\n4h')?.args).toBe('BTCUSD\n4h');
    expect(commandIn(commands, '/usr/bin is a folder')).toBeNull();
    expect(commandIn(commands, 'compact')).toBeNull();
  });

  it('runs a picked command unless it needs something typed after it', () => {
    expect(runsWhenPicked(commands[0]!)).toBe(true);
    expect(runsWhenPicked(commands[1]!)).toBe(false);
    expect(runsWhenPicked(commands[3]!)).toBe(true);
    expect(completion(commands[1]!)).toBe('/autocompact ');
    expect(completion(commands[3]!)).toBe('/market-analysis:compact');
  });
});

describe('summary messages', () => {
  it('say what was compacted', () => {
    const line = summaryLine(
      summary({
        content: 'Goal: …',
        stats: {
          auto: true,
          messages: 24,
          tokensBefore: 18_200,
          tokensAfter: 1_100,
          model: 'Qwen',
          durationMs: 4_200,
        } as Message['stats'],
      }),
    );
    expect(line).toEqual({
      label: 'Compacted automatically · 24 messages · 18K → 1.1K tokens',
      detail: 'Summarized by Qwen in 4.2s',
    });
    expect(summaryLine(summary({ stats: { messages: 1 } as Message['stats'] })).label).toBe(
      'Conversation compacted · 1 message',
    );
  });

  it('show their progress and how they ended', () => {
    expect(summaryLine(summary({ status: 'streaming' })).label).toBe('Compacting the conversation…');
    expect(summaryLine(summary({ status: 'streaming', stats: { part: 2, parts: 3 } as Message['stats'] })).label).toBe(
      'Compacting the conversation (part 2 of 3)…',
    );
    expect(summaryLine(summary({ status: 'error', error: 'The model is not loaded.' }))).toEqual({
      label: 'Could not compact the conversation',
      detail: 'The model is not loaded.',
    });
    expect(summaryLine(summary({ status: 'cancelled' })).label).toBe('Compacting stopped');
  });
});
