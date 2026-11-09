// The bridge process (metatrader/bridge.py) and its line protocol: one JSON request per line on its
// stdin, one answer per line on its stdout, matched by id. Started from the launch spec the app
// sends (on Linux: wine running a Windows Python in the terminal's prefix). Closing its stdin ends
// it; `close` waits a little for that, then kills it.
//
// API: new Bridge(spec, {log}), call(method, params, timeoutMs), close(), alive

import { type ChildProcess, spawn } from 'node:child_process';
import { createInterface } from 'node:readline';

import { RpcError } from '../protocol.ts';

export interface LaunchSpec {
  program: string;
  args: string[];
  env?: Record<string, string>;
  cwd?: string;
}

/** The bridge's own codes (NO_PACKAGE, INIT_FAILED, AUTH_FAILED, ...), BRIDGE_EXITED or TIMEOUT. */
export class BridgeError extends RpcError {}

interface Pending {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

export interface BridgeOptions {
  log?: (level: 'info' | 'warn' | 'error', message: string) => void;
  /** Tests: another way to start it. */
  spawn?: (spec: LaunchSpec) => ChildProcess;
}

const STDERR_LINES = 20;

export class Bridge {
  readonly #child: ChildProcess;
  readonly #pending = new Map<number, Pending>();
  readonly #stderr: string[] = [];
  readonly #exited: Promise<void>;
  #seq = 1;
  #alive = true;

  constructor(spec: LaunchSpec, opts: BridgeOptions = {}) {
    const log = opts.log ?? (() => {});
    this.#child = (opts.spawn ?? start)(spec);
    this.#exited = new Promise((resolve) => {
      const done = (why: string) => {
        if (!this.#alive) return;
        this.#alive = false;
        const tail = this.#stderr.slice(-3).join(' | ');
        for (const [, p] of this.#pending) {
          clearTimeout(p.timer);
          p.reject(new BridgeError('BRIDGE_EXITED', `The MetaTrader bridge ${why}${tail ? `: ${tail}` : ''}`));
        }
        this.#pending.clear();
        resolve();
      };
      this.#child.once('error', (error) => done(`could not start (${error.message})`));
      this.#child.once('exit', (code, signal) => done(`ended (${signal ?? `code ${code}`})`));
    });
    createInterface({ input: this.#child.stdout!, crlfDelay: Infinity }).on('line', (line) => this.#answer(line));
    createInterface({ input: this.#child.stderr!, crlfDelay: Infinity }).on('line', (line) => {
      const text = line.trim();
      if (!text) return;
      this.#stderr.push(text);
      if (this.#stderr.length > STDERR_LINES) this.#stderr.shift();
      log('info', `MetaTrader bridge: ${text.slice(0, 300)}`);
    });
    // A bridge that died must not take the service with it.
    this.#child.stdin!.on('error', () => {});
  }

  get alive(): boolean {
    return this.#alive;
  }

  call<T = unknown>(method: string, params: Record<string, unknown>, timeoutMs: number): Promise<T> {
    if (!this.#alive) return Promise.reject(new BridgeError('BRIDGE_EXITED', 'The MetaTrader bridge is not running'));
    const id = this.#seq++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(
          new BridgeError('TIMEOUT', `MetaTrader did not answer ${method} within ${Math.round(timeoutMs / 1000)} s`),
        );
      }, timeoutMs);
      timer.unref?.();
      this.#pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer });
      this.#child.stdin!.write(`${JSON.stringify({ id, method, params })}\n`);
    });
  }

  /** Ends its stdin (it restores the terminal's account and leaves), then kills it after `graceMs`. */
  async close(graceMs = 10_000): Promise<void> {
    if (!this.#alive) return;
    this.#child.stdin!.end();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const grace = new Promise<void>((resolve) => (timer = setTimeout(resolve, graceMs)));
    await Promise.race([this.#exited, grace]);
    clearTimeout(timer);
    if (this.#alive) this.#child.kill();
    await this.#exited;
  }

  #answer(line: string): void {
    let message: { id?: unknown; result?: unknown; error?: { code?: unknown; message?: unknown } };
    try {
      message = JSON.parse(line);
    } catch {
      return;
    }
    const pending = typeof message.id === 'number' ? this.#pending.get(message.id) : undefined;
    if (!pending) return;
    this.#pending.delete(message.id as number);
    clearTimeout(pending.timer);
    if (message.error) {
      pending.reject(new BridgeError(String(message.error.code ?? 'ERROR'), String(message.error.message ?? 'failed')));
    } else {
      pending.resolve(message.result ?? null);
    }
  }
}

function start(spec: LaunchSpec): ChildProcess {
  return spawn(spec.program, spec.args, {
    cwd: spec.cwd,
    env: { ...process.env, ...spec.env },
    stdio: ['pipe', 'pipe', 'pipe'],
    // No console window for python.exe on Windows.
    windowsHide: true,
  });
}
