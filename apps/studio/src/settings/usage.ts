import { formatTime } from '@/lib/format';
import type { Allowance } from '@/lib/types';

const DAY = 86_400_000;

export const ALLOWANCE_LABELS: Record<Allowance['kind'], string> = {
  freeRequests: 'Free requests today',
  keyCredit: 'Credit on this key',
};

/** "38", or "$7.25" for credit. */
export function amount(a: Allowance, value: number): string {
  return a.kind === 'keyCredit' ? `$${value.toFixed(2)}` : String(Math.round(value));
}

/** How much is left, from 0 to 1. */
export function share(a: Allowance): number {
  return a.limit > 0 ? Math.max(0, Math.min(1, a.remaining / a.limit)) : 0;
}

export function tone(a: Allowance): 'accent' | 'warning' | 'danger' {
  const left = share(a);
  return left <= 0.1 ? 'danger' : left <= 0.25 ? 'warning' : 'accent';
}

/** "resets at 02:00" within a day, otherwise the day: "resets Mon, 12 Oct". Null when it never does. */
export function resets(a: Allowance, now = Date.now()): string | null {
  if (a.resetsAt === null) return null;
  if (a.resetsAt - now <= DAY) return `resets at ${formatTime(a.resetsAt)}`;
  const day = new Date(a.resetsAt).toLocaleDateString([], { weekday: 'short', day: 'numeric', month: 'short' });
  return `resets ${day}`;
}

/** What is left, said in a few words: "39 free requests left", "$7.25 credit left". */
export function leftInShort(a: Allowance): string {
  if (a.kind === 'keyCredit') return `${amount(a, a.remaining)} credit left`;
  const n = Math.round(a.remaining);
  return `${n} free request${n === 1 ? '' : 's'} left`;
}

/** The allowance closest to running out, which the model list shows. */
export function tightest(allowances: Allowance[]): Allowance | null {
  return allowances.reduce<Allowance | null>((low, a) => (low === null || share(a) < share(low) ? a : low), null);
}
