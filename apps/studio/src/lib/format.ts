import { convertFileSrc } from '@tauri-apps/api/core';

export { formatBytes, formatCount, formatDuration } from '@demido/ui';

/** A URL the webview can load for a file on disk (images, avatars). */
export function fileUrl(path: string | null | undefined): string | null {
  return path ? convertFileSrc(path) : null;
}

const DAY = 86_400_000;

/** Which group of the chat list a timestamp belongs to. */
export function recencyGroup(ms: number, now = Date.now()): string {
  const startOfToday = new Date(now);
  startOfToday.setHours(0, 0, 0, 0);
  const t = startOfToday.getTime();
  if (ms >= t) return 'Today';
  if (ms >= t - DAY) return 'Yesterday';
  if (ms >= t - 7 * DAY) return 'Previous 7 days';
  if (ms >= t - 30 * DAY) return 'Previous 30 days';
  return 'Older';
}

export function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

export function formatDateTime(ms: number): string {
  return new Date(ms).toLocaleString([], {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** Prices keep the precision the instrument trades at. */
export function formatPrice(value: number | undefined | null, pricescale = 0): string {
  if (value === undefined || value === null || !Number.isFinite(value)) return '—';
  const decimals =
    pricescale > 0
      ? Math.min(8, Math.max(0, Math.round(Math.log10(pricescale))))
      : Math.abs(value) >= 1000
        ? 2
        : Math.abs(value) >= 1
          ? 4
          : 6;
  return value.toLocaleString(undefined, { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
}

export function formatPercent(value: number | undefined | null, digits = 2): string {
  if (value === undefined || value === null || !Number.isFinite(value)) return '—';
  const sign = value > 0 ? '+' : '';
  return `${sign}${value.toFixed(digits)}%`;
}

export function formatTokens(n: number | undefined | null): string {
  if (n === undefined || n === null) return '—';
  return n >= 1000 ? `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1)}K` : String(n);
}
