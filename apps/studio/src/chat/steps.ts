// How an assistant turn is laid out: each model step on its own, except runs of workspace file
// calls, which fold into one card so a model writing a dozen files does not fill the page.

import type { Message, ToolCall } from '@/lib/types';

/** A tool call with the tool message that answers it. */
export interface CallEntry {
  call: ToolCall;
  /** The tool message holding the call's status and result; absent until it starts. */
  result: Message | undefined;
  /** The turn ended before this call could run. */
  orphaned: boolean;
}

/** One assistant message and its tool calls. */
export interface Step {
  message: Message;
  calls: CallEntry[];
  /** The message's reasoning and text are drawn above the bundle; only its calls are inside. */
  lead?: boolean;
}

export type Block = { kind: 'step'; step: Step } | { kind: 'files'; id: string; steps: Step[] };

type Verb = 'write' | 'read' | 'list';

const FILE_TOOLS = new Map<string, Verb>([
  ['write_file', 'write'],
  ['read_file', 'read'],
  ['list_files', 'list'],
]);

/** Rough reasoning time in seconds, from its length and the generation speed. */
export function thoughtSeconds(message: Message): number | undefined {
  const tps = message.stats?.tokensPerSecond;
  return message.reasoning && tps ? message.reasoning.length / 4 / tps : undefined;
}

/**
 * Splits a turn's messages into what the chat draws. Two or more file calls in a row, from steps
 * that make no other calls, become one `files` block keyed by its first message, so it grows in
 * place while the model keeps going. A short line said along with the calls ("Now the styles.")
 * goes into the bundle with them; a step that says more, or the turn's last words, keeps its text
 * in the flow and its calls start the run. `live` is whether the turn is still running.
 */
export function turnBlocks(messages: Message[], live: boolean): Block[] {
  const blocks: Block[] = [];
  // The last thing the model said this turn is its answer so far, never folded away.
  const lastWords = messages.findLastIndex((m) => m.role === 'assistant' && m.content.trim() !== '');
  let run: Step[] = [];
  /** Where the lead step of the run is drawn, to hand its calls back if no bundle forms. */
  let head = -1;
  const flush = () => {
    if (run.reduce((n, s) => n + s.calls.length, 0) >= 2) {
      blocks.push({ kind: 'files', id: run[0]!.message.id, steps: run });
    } else {
      for (const step of run) {
        if (step.lead && head >= 0) blocks[head] = { kind: 'step', step: { message: step.message, calls: step.calls } };
        else blocks.push({ kind: 'step', step });
      }
    }
    run = [];
    head = -1;
  };
  messages.forEach((m, index) => {
    if (m.role !== 'assistant') return;
    // A step's own tool messages come after it and before the next step. Looking no further
    // matters: some providers reuse call ids from one step to the next.
    const rest = messages.slice(index + 1);
    const next = rest.findIndex((x) => x.role === 'assistant');
    const own = (next === -1 ? rest : rest.slice(0, next)).filter((x) => x.role === 'tool');
    const step: Step = {
      message: m,
      calls: m.toolCalls.map((call) => {
        const result = own.find((t) => t.toolCallId === call.id);
        return { call, result, orphaned: !live && !result };
      }),
    };
    if (!onlyFileCalls(step)) {
      flush();
      blocks.push({ kind: 'step', step });
    } else if (!m.content.trim() || (isNarration(m.content) && index !== lastWords)) {
      run.push(step);
    } else {
      flush();
      head = blocks.push({ kind: 'step', step: { message: m, calls: [] } }) - 1;
      run.push({ ...step, lead: true });
    }
  });
  flush();
  return blocks;
}

/** Longest text that still reads as a line about the files rather than an answer. */
const NARRATION_MAX = 160;

/**
 * A single line that only says what the next files are ("Now the styles."). Anything longer, on
 * several lines, or asking something is left in the chat where it cannot be missed.
 */
function isNarration(content: string): boolean {
  const text = content.trim();
  return text.length <= NARRATION_MAX && !/[\r\n?]/.test(text);
}

/** A step that only calls file tools, did not fail, and waits on nobody. */
function onlyFileCalls({ message, calls }: Step): boolean {
  return (
    message.status !== 'error' &&
    message.status !== 'cancelled' &&
    calls.length > 0 &&
    calls.every((c) => FILE_TOOLS.has(c.call.name) && c.result?.status !== 'awaitingApproval')
  );
}

type CallState = 'running' | 'failed' | 'stopped' | 'done';

function stateOf({ result, orphaned }: CallEntry): CallState {
  const status = result?.status;
  if (status === 'running' || status === 'awaitingApproval' || (!result && !orphaned)) return 'running';
  if (status === 'cancelled' || !result) return 'stopped';
  if (result.toolResult?.ok === false || status === 'error') return 'failed';
  return 'done';
}

const paths = new WeakMap<ToolCall, string>();

/** The workspace path a file call names, as the backend resolves it. Parsed once per call. */
function pathOf(call: ToolCall): string {
  let path = paths.get(call);
  if (path === undefined) {
    path = parsePath(call);
    paths.set(call, path);
  }
  return path;
}

function parsePath(call: ToolCall): string {
  try {
    const path = (JSON.parse(call.arguments) as { path?: unknown } | null)?.path;
    return typeof path === 'string'
      ? path
          .trim()
          .replace(/\\/g, '/')
          .replace(/^(\.\/)+/, '')
      : '';
  } catch {
    return '';
  }
}

/** What a call is doing, as the backend words it, for calls that have not started yet. */
function describe(call: ToolCall): string | null {
  const path = pathOf(call);
  if (call.name === 'list_files') return 'Listing workspace files';
  if (!path) return null;
  return call.name === 'read_file' ? `Reading ${path}` : `Writing ${path}`;
}

/** `wrote 8 files, read 2, listed the workspace`: distinct paths per verb, in that order. */
function countByVerb(calls: CallEntry[], words: Record<Verb, string>): string {
  const paths: Record<Verb, Set<string>> = { write: new Set(), read: new Set(), list: new Set() };
  for (const { call } of calls) {
    const verb = FILE_TOOLS.get(call.name);
    if (verb) paths[verb].add(verb === 'list' ? 'workspace' : pathOf(call) || call.id);
  }
  const parts: string[] = [];
  for (const verb of ['write', 'read'] as const) {
    const n = paths[verb].size;
    if (n) parts.push(parts.length ? `${words[verb]} ${n}` : `${words[verb]} ${n} ${n === 1 ? 'file' : 'files'}`);
  }
  if (paths.list.size) parts.push(`${words.list} the workspace`);
  return parts.join(', ');
}

export interface FilesSummary {
  status: CallState;
  /** What worked once finished, e.g. `Wrote 8 files, read 2`; the call in progress, `Writing src/App.js…`, before. */
  label: string;
  /** Muted file count shown next to the label while working, e.g. `5 files`. */
  count: string | null;
  /** `1 failed`, when any call failed. */
  failure: string | null;
  /** Total time of the calls that finished. */
  durationMs: number | undefined;
}

/** The header of a bundle of file calls. */
export function summarizeFiles(calls: CallEntry[]): FilesSummary {
  const states = calls.map(stateOf);
  const failed = states.filter((s) => s === 'failed').length;
  const status: CallState = states.includes('running')
    ? 'running'
    : failed
      ? 'failed'
      : states.includes('stopped')
        ? 'stopped'
        : 'done';
  const durations = calls.map((c) => c.result?.toolResult?.durationMs).filter((ms): ms is number => ms !== undefined);
  const durationMs = durations.length ? durations.reduce((a, b) => a + b, 0) : undefined;
  const failure = failed ? `${failed} failed` : null;

  if (status === 'running') {
    const active =
      calls.findLast((c) => c.result?.status === 'running') ?? calls.find((_, i) => states[i] === 'running');
    const label = active && (active.result?.toolResult?.label ?? describe(active.call));
    const files = new Set(calls.filter((c) => c.call.name !== 'list_files').map((c) => pathOf(c.call) || c.call.id));
    return {
      status,
      label: label ? `${label}…` : 'Working on files…',
      count: files.size ? `${files.size} ${files.size === 1 ? 'file' : 'files'}` : null,
      failure,
      durationMs,
    };
  }
  const worked = calls.filter((_, i) => states[i] === 'done');
  const tried = calls.filter((_, i) => states[i] === 'failed');
  const done = countByVerb(worked, { write: 'wrote', read: 'read', list: 'listed' });
  const label = worked.length
    ? done.charAt(0).toUpperCase() + done.slice(1)
    : tried.length
      ? `Tried to ${countByVerb(tried, { write: 'write', read: 'read', list: 'list' })}`
      : `Stopped before ${countByVerb(calls, { write: 'writing', read: 'reading', list: 'listing' })}`;
  return { status, label, count: null, failure, durationMs };
}
