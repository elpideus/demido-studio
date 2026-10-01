import { beforeEach, describe, expect, it } from 'vitest';

import type { Message, ToolCall, ToolResult } from '@/lib/types';
import { summarizeFiles, turnBlocks, type Block, type CallEntry } from './steps';

let seq = 0;

beforeEach(() => {
  seq = 0;
});

function message(fields: Partial<Message>): Message {
  seq += 1;
  return {
    id: `m${seq}`,
    chatId: 'chat',
    seq,
    role: 'assistant',
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

function call(name: string, args: Record<string, unknown> = {}): ToolCall {
  seq += 1;
  return { id: `c${seq}`, name, arguments: JSON.stringify(args) };
}

const write = (path: string) => call('write_file', { path, content: 'x' });
const read = (path: string) => call('read_file', { path });
const list = () => call('list_files');

function result(c: ToolCall, fields: Partial<Message> = {}, tr: Partial<ToolResult> = {}): Message {
  return message({
    role: 'tool',
    toolCallId: c.id,
    toolName: c.name,
    toolResult: { label: c.name, args: {}, ok: true, durationMs: 0, ...tr },
    ...fields,
  });
}

/** An assistant step with its calls answered by finished tool messages. */
function step(calls: ToolCall[], fields: Partial<Message> = {}): Message[] {
  return [message({ toolCalls: calls, ...fields }), ...calls.map((c) => result(c))];
}

const kinds = (blocks: Block[]) => blocks.map((b) => b.kind);

/** Text long enough to be an answer in its own right, not a line about the next file. */
const explanation =
  'The site has three parts.\n\nThe page itself, a stylesheet, and a small script that loads the products.';

function entry(c: ToolCall, fields: Partial<Message> = {}, tr: Partial<ToolResult> = {}): CallEntry {
  return { call: c, result: result(c, fields, tr), orphaned: false };
}

describe('turnBlocks', () => {
  it('folds a run of quiet write steps into one bundle, keeping their reasoning', () => {
    const messages = Array.from({ length: 12 }, (_, i) =>
      step([write(`src/f${i}.js`)], { reasoning: `plan ${i}` }),
    ).flat();
    const blocks = turnBlocks(messages, false);
    expect(kinds(blocks)).toEqual(['files']);
    const bundle = blocks[0]!;
    if (bundle.kind !== 'files') throw new Error('expected a bundle');
    expect(bundle.id).toBe(messages[0]!.id);
    expect(bundle.steps.flatMap((s) => s.calls)).toHaveLength(12);
    expect(bundle.steps.map((s) => s.message.reasoning)).toEqual(Array.from({ length: 12 }, (_, i) => `plan ${i}`));
    expect(bundle.steps.every((s) => s.calls.every((c) => c.result && !c.orphaned))).toBe(true);
  });

  it('splits the run at a step that says something', () => {
    const messages = [
      ...step([write('a.js')]),
      ...step([write('b.js')]),
      message({ content: 'Now the styles.' }),
      ...step([write('c.css')]),
      ...step([write('d.css')]),
    ];
    expect(kinds(turnBlocks(messages, false))).toEqual(['files', 'step', 'files']);
  });

  it('folds a short line said with each file call into the bundle', () => {
    const messages = [
      ...step([write('one.txt')], { content: 'First, one.txt.' }),
      ...step([write('two.txt')], { content: 'Next, two.txt.' }),
      ...step([write('three.txt')], { content: 'Last, three.txt.' }),
      message({ content: 'All three files are written.' }),
    ];
    const blocks = turnBlocks(messages, false);
    expect(kinds(blocks)).toEqual(['files', 'step']);
    expect(blocks[0]!.kind === 'files' && blocks[0]!.steps.map((s) => s.lead ?? false)).toEqual([false, false, false]);
  });

  it('never folds away a question, a list, or the last words of the turn', () => {
    const question = 'Do you also want a dark theme?';
    const asking = [...step([write('index.html')]), ...step([write('style.css')], { content: question })];
    const asked = turnBlocks([...asking, message({ content: '' })], false);
    expect(asked.some((b) => b.kind === 'step' && b.step.message.content === question)).toBe(true);

    const listed = turnBlocks(
      [...step([write('a.js')], { content: 'I will create:\n- a.js\n- b.js' }), ...step([write('b.js')])],
      false,
    );
    expect(kinds(listed)).toEqual(['step', 'files']);

    const last = turnBlocks([...step([write('a.js')]), ...step([write('b.js')], { content: 'Here you go.' })], false);
    expect(last.some((b) => b.kind === 'step' && b.step.message.content === 'Here you go.')).toBe(true);
  });

  it('keeps a lone file call under the text that came with it', () => {
    const blocks = turnBlocks(step([write('b.js')], { content: 'Writing b.' }), false);
    expect(kinds(blocks)).toEqual(['step']);
    expect(blocks[0]!.kind === 'step' && blocks[0]!.step.calls).toHaveLength(1);
    const long = turnBlocks(step([write('b.js')], { content: explanation }), false);
    expect(long[0]!.kind === 'step' && long[0]!.step.calls).toHaveLength(1);
  });

  it('shows a longer text above the bundle of the file calls it comes with', () => {
    const blocks = turnBlocks(step([write('a.js'), write('b.js'), write('c.js')], { content: explanation }), false);
    expect(kinds(blocks)).toEqual(['step', 'files']);
    const [text, bundle] = blocks;
    expect(text!.kind === 'step' && text!.step.calls).toEqual([]);
    expect(bundle!.kind === 'files' && bundle!.steps[0]!.lead).toBe(true);
    expect(bundle!.kind === 'files' && bundle!.steps[0]!.calls).toHaveLength(3);
  });

  it('lets a step with a longer text start a run the quiet steps after it join', () => {
    const messages = [...step([write('a.js')], { content: explanation }), ...step([write('b.css')])];
    const blocks = turnBlocks(messages, false);
    expect(kinds(blocks)).toEqual(['step', 'files']);
    expect(blocks[1]!.kind === 'files' && blocks[1]!.steps.flatMap((s) => s.calls)).toHaveLength(2);
  });

  it('matches each call with its own step when a provider reuses call ids', () => {
    const first = { id: 'call_0', name: 'write_file', arguments: '{"path":"a.js"}' };
    const second = { id: 'call_0', name: 'write_file', arguments: '{"path":"b.js"}' };
    const messages = [
      message({ toolCalls: [first] }),
      result(first, {}, { durationMs: 100 }),
      message({ toolCalls: [second] }),
      result(second, { status: 'running' }),
    ];
    const [bundle] = turnBlocks(messages, true);
    const calls = bundle!.kind === 'files' ? bundle!.steps.flatMap((s) => s.calls) : [];
    expect(calls.map((c) => c.result?.status)).toEqual(['done', 'running']);
    expect(summarizeFiles(calls).status).toBe('running');
  });

  it('leaves a single file call as it is', () => {
    expect(kinds(turnBlocks(step([write('a.js')]), false))).toEqual(['step']);
  });

  it('breaks the run at a step that failed', () => {
    const messages = [
      ...step([write('a.js')]),
      ...step([write('b.js')], { status: 'error', error: 'Boom' }),
      ...step([write('c.js')]),
    ];
    expect(kinds(turnBlocks(messages, false))).toEqual(['step', 'step', 'step']);
  });

  it('never bundles other tools', () => {
    const messages = [...step([write('a.js')]), ...step([call('run_python', { code: '1' })]), ...step([write('b.js')])];
    expect(kinds(turnBlocks(messages, false))).toEqual(['step', 'step', 'step']);
    expect(kinds(turnBlocks(step([write('a.js'), call('run_python'), write('b.js')]), false))).toEqual(['step']);
  });

  it('bundles one step that makes several file calls', () => {
    const blocks = turnBlocks(step([write('a.js'), write('b.js'), write('c.js')]), false);
    expect(kinds(blocks)).toEqual(['files']);
    expect(blocks[0]!.kind === 'files' && blocks[0]!.steps[0]!.calls.length).toBe(3);
  });

  it('keeps a step that waits for approval out of the bundle', () => {
    const c = read('a.csv');
    const waiting = [message({ toolCalls: [c] }), result(c, { status: 'awaitingApproval' })];
    const messages = [...step([write('a.js')]), ...step([write('b.js')]), ...waiting];
    expect(kinds(turnBlocks(messages, true))).toEqual(['files', 'step']);
  });

  it('keeps the bundle keyed by its first step while it grows, with the streaming step after it', () => {
    const first = step([write('a.js')]);
    const pending = write('b.js');
    const live = [...first, message({ toolCalls: [pending] }), message({ status: 'streaming', reasoning: 'next' })];
    const blocks = turnBlocks(live, true);
    expect(kinds(blocks)).toEqual(['files', 'step']);
    expect(blocks[0]!.kind === 'files' && blocks[0]!.id).toBe(first[0]!.id);
    const calls = blocks[0]!.kind === 'files' ? blocks[0]!.steps.flatMap((s) => s.calls) : [];
    expect(calls[1]).toMatchObject({ result: undefined, orphaned: false });
  });

  it('marks calls with no answer as orphaned once the turn is over', () => {
    const c = write('a.js');
    const [block] = turnBlocks([message({ toolCalls: [c] })], false);
    expect(block?.kind === 'step' && block.step.calls[0]!.orphaned).toBe(true);
    const [liveBlock] = turnBlocks([message({ toolCalls: [c] })], true);
    expect(liveBlock?.kind === 'step' && liveBlock.step.calls[0]!.orphaned).toBe(false);
  });
});

describe('summarizeFiles', () => {
  it('counts files by verb', () => {
    const writes = Array.from({ length: 8 }, (_, i) => entry(write(`f${i}.js`)));
    expect(summarizeFiles(writes).label).toBe('Wrote 8 files');
    expect(summarizeFiles([entry(read('a.csv')), ...writes, entry(read('b.csv'))]).label).toBe('Wrote 8 files, read 2');
    expect(summarizeFiles([entry(read('a.csv')), entry(read('b.csv')), entry(read('c.csv'))]).label).toBe(
      'Read 3 files',
    );
    expect(summarizeFiles([entry(list()), entry(list())]).label).toBe('Listed the workspace');
    expect(summarizeFiles([entry(list()), entry(write('a.js')), entry(read('b.js'))]).label).toBe(
      'Wrote 1 file, read 1, listed the workspace',
    );
  });

  it('counts a file written twice once', () => {
    const calls = [entry(write('src/App.js')), entry(write('./src\\App.js')), entry(write('src/index.js'))];
    const summary = summarizeFiles(calls);
    expect(summary.label).toBe('Wrote 2 files');
    expect(summary.status).toBe('done');
  });

  it('reports failed calls', () => {
    const calls = [
      entry(write('a.js')),
      entry(write('b.js'), {}, { ok: false, display: { error: 'Disk full' } }),
      entry(write('c.js')),
    ];
    expect(summarizeFiles(calls)).toMatchObject({ status: 'failed', label: 'Wrote 2 files', failure: '1 failed' });
    expect(summarizeFiles(calls.filter((_, i) => i !== 1)).failure).toBeNull();
  });

  it('counts a failed write that was retried once, and says when nothing worked', () => {
    const retried = [entry(write('a.js'), {}, { ok: false }), entry(write('a.js'))];
    expect(summarizeFiles(retried)).toMatchObject({ status: 'failed', label: 'Wrote 1 file', failure: '1 failed' });
    const failed = [entry(write('a.js'), {}, { ok: false }), entry(read('b.js'), {}, { ok: false })];
    expect(summarizeFiles(failed)).toMatchObject({ label: 'Tried to write 1 file, read 1', failure: '2 failed' });
  });

  it('adds up the durations', () => {
    const calls = [entry(write('a.js'), {}, { durationMs: 120 }), entry(write('b.js'), {}, { durationMs: 30 })];
    expect(summarizeFiles(calls).durationMs).toBe(150);
  });

  it('names the call in progress while working', () => {
    const running = write('src/App.js');
    const calls = [
      entry(write('a.js')),
      entry(running, { status: 'running' }, { label: 'Writing src/App.js', ok: undefined }),
      { call: write('b.js'), result: undefined, orphaned: false },
    ];
    expect(summarizeFiles(calls)).toMatchObject({ status: 'running', label: 'Writing src/App.js…', count: '3 files' });
  });

  it('names the next call when none has started yet', () => {
    const calls = [entry(write('a.js')), { call: read('data.csv'), result: undefined, orphaned: false }];
    expect(summarizeFiles(calls).label).toBe('Reading data.csv…');
    const unreadable = {
      call: { id: 'x', name: 'write_file', arguments: '{oops' },
      result: undefined,
      orphaned: false,
    };
    expect(summarizeFiles([entry(list()), unreadable]).label).toBe('Working on files…');
  });

  it('says when the turn stopped before the calls ran', () => {
    const calls = [write('a.js'), write('b.js'), read('c.js')].map((c) => ({
      call: c,
      result: undefined,
      orphaned: true,
    }));
    expect(summarizeFiles(calls)).toMatchObject({
      status: 'stopped',
      label: 'Stopped before writing 2 files, reading 1',
    });
    const partly = [entry(write('a.js')), { call: write('b.js'), result: undefined, orphaned: true }];
    expect(summarizeFiles(partly)).toMatchObject({ status: 'stopped', label: 'Wrote 1 file' });
  });
});
