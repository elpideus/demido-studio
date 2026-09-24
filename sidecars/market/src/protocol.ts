// The wire protocol with Demido Studio: one JSON object per line on stdin and stdout.
//
//   request   {"id": 1, "method": "quote", "params": {...}}
//   response  {"id": 1, "result": ...}  or  {"id": 1, "error": {"code": "...", "message": "..."}}
//   event     {"event": "stream.update", "params": {...}}
//
// stdout carries nothing else: console.log is redirected to stderr in main.ts.

export class RpcError extends Error {
  code: string;

  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

function write(message: unknown): void {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

export function reply(id: number, result: unknown): void {
  write({ id, result: result ?? null });
}

export function fail(id: number, error: unknown): void {
  const e =
    error instanceof RpcError
      ? error
      : new RpcError('ERROR', error instanceof Error ? error.message : String(error));
  write({ id, error: { code: e.code, message: e.message } });
}

export function emit(event: string, params: unknown): void {
  write({ event, params });
}

export function log(level: 'info' | 'warn' | 'error', message: string): void {
  emit('log', { level, message });
}

/** A candle as Demido stores it: seconds since the epoch, OHLCV. */
export interface Bar {
  t: number;
  o: number;
  h: number;
  l: number;
  c: number;
  v: number;
}
