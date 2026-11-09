// How much the MetaTrader terminal writes to disk, so a download that makes it rewrite its history
// caches is stopped early (Alkaphram's terminal once wrote 2.7 TB in two hours when asked for deep
// ranges over and over). Linux: /proc/<pid>/io of the terminal's Wine process (same user, readable).
// Windows: the bridge reads the process's I/O counters. Elsewhere, or when unreadable: null.
//
// API: procWrites(match) -> () => Promise<number | null>  (bytes written by the first process whose
//      command line `match` accepts)

import fsp from 'node:fs/promises';

export function procWrites(match: (cmdline: string) => boolean): () => Promise<number | null> {
  let pid: number | null = null;

  const read = async (p: number): Promise<number | null> => {
    try {
      const text = await fsp.readFile(`/proc/${p}/io`, 'ascii');
      const line = text.split('\n').find((l) => l.startsWith('write_bytes:'));
      return line ? Number(line.split(/\s+/)[1]) : null;
    } catch {
      return null;
    }
  };

  const find = async (): Promise<number | null> => {
    let names: string[];
    try {
      names = await fsp.readdir('/proc');
    } catch {
      return null;
    }
    for (const name of names) {
      if (!/^\d+$/.test(name)) continue;
      try {
        const cmdline = (await fsp.readFile(`/proc/${name}/cmdline`, 'utf8')).replace(/\0/g, ' ');
        if (match(cmdline)) return Number(name);
      } catch {
        // Gone, or not ours.
      }
    }
    return null;
  };

  return async () => {
    if (process.platform !== 'linux') return null;
    if (pid !== null) {
      const bytes = await read(pid);
      if (bytes !== null) return bytes;
    }
    pid = await find();
    return pid === null ? null : read(pid);
  };
}
