// The MetaTrader card's choices and text, kept apart from React so they can be tested.

import type {
  MetaTraderAccount,
  MetaTraderDetection,
  MetaTraderSetup,
  MetaTraderStatus,
  MetaTraderTerminal,
  Settings,
} from '@/lib/types';

/** The dialog's one choice for three settings: which source downloads first, and whether the other
 *  fills in for it. */
export type SourceOrder = 'fallback' | 'preferred' | 'only' | 'off';

type SourceSettings = Pick<Settings, 'marketSource' | 'metatraderFallback' | 'dukascopyFallback'>;

export function sourceOrder(s: SourceSettings): SourceOrder {
  if (s.marketSource === 'metatrader') return s.dukascopyFallback ? 'preferred' : 'only';
  return s.metatraderFallback ? 'fallback' : 'off';
}

export function orderFields(order: SourceOrder): {
  source: 'dukascopy' | 'metatrader';
  fallback: boolean;
  dukascopyFallback: boolean;
} {
  // MetaTrader first keeps its fallback on, for when Dukascopy comes first again; Dukascopy first
  // keeps Dukascopy's, for when MetaTrader comes first again.
  if (order === 'preferred' || order === 'only') {
    return { source: 'metatrader', fallback: true, dukascopyFallback: order === 'preferred' };
  }
  return { source: 'dukascopy', fallback: order === 'fallback', dukascopyFallback: true };
}

export const ORDER_OPTIONS: Array<{ value: SourceOrder; label: string }> = [
  { value: 'fallback', label: 'Dukascopy first' },
  { value: 'preferred', label: 'MetaTrader first' },
  { value: 'only', label: 'MetaTrader only' },
  { value: 'off', label: 'Dukascopy only' },
];

export const ORDER_TEXT: Record<SourceOrder, string> = {
  fallback:
    'Dukascopy downloads; MetaTrader takes over the days Dukascopy cannot serve while it is rate limited or down.',
  preferred: 'MetaTrader downloads every day it has; Dukascopy fills in what it lacks, such as older history.',
  only: 'Only MetaTrader downloads, and Dukascopy is never asked. Days the terminal does not have stay missing, and downloads wait while it cannot be used.',
  off: 'Only Dukascopy downloads. MetaTrader stays set up, unused.',
};

/** The person accepted the current notice. */
export function consented(settings: Pick<Settings, 'metatraderConsent'> | null, version: string): boolean {
  return !!settings && settings.metatraderConsent === version;
}

const sameAccount = (a: MetaTraderAccount, login: number | null, server: string | null) =>
  a.login === login && a.server === server;

/** The terminal the settings name when it is still there, else the first found. */
export function chosenTerminal(
  detection: MetaTraderDetection | null,
  settings: Pick<Settings, 'metatraderTerminal'> | null,
): MetaTraderTerminal | null {
  const terminals = detection?.terminals ?? [];
  return terminals.find((t) => t.path === settings?.metatraderTerminal) ?? terminals[0] ?? null;
}

/** The account the settings name on that terminal, else the one it was last on. */
export function chosenAccount(
  terminal: MetaTraderTerminal | null,
  settings: Pick<Settings, 'metatraderTerminal' | 'metatraderLogin' | 'metatraderServer'> | null,
): MetaTraderAccount | null {
  if (!terminal) return null;
  const saved =
    settings?.metatraderTerminal === terminal.path
      ? terminal.accounts.find((a) => sameAccount(a, settings.metatraderLogin, settings.metatraderServer))
      : undefined;
  return saved ?? terminal.accounts.find((a) => a.last) ?? terminal.accounts[0] ?? null;
}

export const accountKey = (a: MetaTraderAccount) => `${a.login}@${a.server}`;

export const accountLabel = (a: MetaTraderAccount) => `${a.login} · ${a.server}${a.last ? ' (last used)' : ''}`;

// A forex week of 1-minute bars (24 h, 5 days); indices trade fewer hours, so they reach further back.
const BARS_PER_MONTH = (1440 * 5 * 52) / 12;

/** How far back "Max bars in chart" lets the terminal serve 1-minute history, for currency pairs. */
export function depthText(maxBars: number | null): string | null {
  if (!maxBars || maxBars <= 0) return null;
  const months = maxBars / BARS_PER_MONTH;
  if (months < 1) return 'under a month of 1-minute history';
  if (months < 18) {
    const n = Math.round(months);
    return `about ${n} month${n === 1 ? '' : 's'} of 1-minute history`;
  }
  const years = Math.round(months / 12);
  return `about ${years} years of 1-minute history`;
}

/** What the service's MetaTrader source is doing, in a few words. */
export function stateText(status: MetaTraderStatus | null, now = Date.now()): string {
  if (!status) return 'Used once the market service runs';
  const retry = status.retryAt && status.retryAt > now ? ` · again in ${minutes(status.retryAt - now)}` : '';
  switch (status.state) {
    case 'off':
      return 'Not used';
    case 'idle':
      return status.mode === 'preferred' || status.mode === 'only'
        ? 'Used for the next download'
        : 'Ready when Dukascopy cannot serve';
    case 'starting':
      return 'Opening MetaTrader';
    case 'connecting':
      return status.message ?? 'Connecting';
    case 'ready':
      return 'Connected';
    case 'downloading':
      return status.message ?? 'Downloading';
    case 'paused':
      return (status.message ?? 'Paused') + retry;
    case 'error':
      return (status.message ?? 'Failed') + retry;
  }
}

const minutes = (ms: number) => {
  const m = Math.ceil(ms / 60_000);
  return m <= 1 ? '1 min' : `${m} min`;
};

/** The setup line while the bridge installs, or after it failed. */
export function setupText(setup: MetaTraderSetup | null): string | null {
  if (!setup) return null;
  if (setup.installing) return setup.message ? `Installing the bridge: ${setup.message}` : 'Installing the bridge';
  if (setup.error) return `The bridge could not be installed: ${setup.error}`;
  return null;
}
