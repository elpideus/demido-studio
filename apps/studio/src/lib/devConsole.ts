// WebKitGTK writes the page's console to the terminal with the first argument only, so React's
// warnings reach it as "two children with the same key, `%s`", naming neither the key nor the
// component. In development the arguments are written into the message first, followed by the
// components that rendered the element at fault.

import * as React from 'react';

const PLACEHOLDER = /%[sdifoOc%]/g;

function show(value: unknown): string {
  if (typeof value === 'string') return value;
  if (typeof value !== 'object' || value === null) return String(value);
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

/** `console`'s arguments with the values its placeholders stand for written into the message. */
export function formatConsole(args: unknown[]): unknown[] {
  const [message, ...values] = args;
  if (typeof message !== 'string' || !message.includes('%')) return args;
  let used = 0;
  const text = message.replace(PLACEHOLDER, (placeholder) => {
    if (placeholder === '%%') return '%';
    if (used >= values.length) return placeholder;
    const value = values[used++];
    return placeholder === '%c' ? '' : show(value);
  });
  return [text, ...values.slice(used)];
}

export function installDevConsole(): void {
  for (const level of ['error', 'warn'] as const) {
    const write = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      const out = formatConsole(args);
      // Only while React renders, and only in its development build.
      const owners = React.captureOwnerStack?.();
      if (owners && typeof out[0] === 'string') out[0] += owners;
      write(...out);
    };
  }
}
