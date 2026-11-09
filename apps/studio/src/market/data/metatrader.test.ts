// The MetaTrader card's choices and text.

import { describe, expect, it } from 'vitest';

import type { MetaTraderDetection, MetaTraderStatus, MetaTraderTerminal } from '@/lib/types';
import {
  chosenAccount,
  chosenTerminal,
  consented,
  depthText,
  orderFields,
  setupText,
  sourceOrder,
  stateText,
} from './metatrader';

const terminal = (over: Partial<MetaTraderTerminal> = {}): MetaTraderTerminal => ({
  path: '/home/me/.mt5/drive_c/Program Files/MetaTrader 5/terminal64.exe',
  windowsPath: 'C:\\Program Files\\MetaTrader 5\\terminal64.exe',
  name: 'MetaTrader 5',
  prefix: '/home/me/.mt5',
  dataDir: '/home/me/.mt5/drive_c/Program Files/MetaTrader 5',
  portable: true,
  accounts: [
    { login: 111, server: 'Broker-Live', last: true },
    { login: 222, server: 'Broker-Demo', last: false },
  ],
  maxBars: 500_000,
  ...over,
});

const detection = (terminals: MetaTraderTerminal[]): MetaTraderDetection => ({
  supported: true,
  reason: null,
  terminals,
  setup: { installed: false, installing: false, message: null, error: null },
  consentVersion: '2026-10',
});

const status = (over: Partial<MetaTraderStatus> = {}): MetaTraderStatus => ({
  mode: 'fallback',
  state: 'idle',
  message: null,
  account: null,
  terminal: null,
  clock: null,
  markets: null,
  days: 0,
  retryAt: null,
  switched: false,
  tied: false,
  ...over,
});

describe('the source choice', () => {
  it('maps the three settings to one choice and back', () => {
    const of = (marketSource: 'dukascopy' | 'metatrader', metatraderFallback: boolean, dukascopyFallback: boolean) =>
      sourceOrder({ marketSource, metatraderFallback, dukascopyFallback });
    expect(of('dukascopy', true, false)).toBe('fallback');
    expect(of('dukascopy', false, true)).toBe('off');
    expect(of('metatrader', false, true)).toBe('preferred');
    expect(of('metatrader', true, false)).toBe('only');
    for (const order of ['fallback', 'preferred', 'only', 'off'] as const) {
      const { source, fallback, dukascopyFallback } = orderFields(order);
      expect(of(source, fallback, dukascopyFallback)).toBe(order);
    }
    // Each keeps the other side's fallback on for when the order turns around.
    expect(orderFields('only').fallback).toBe(true);
    expect(orderFields('off').dukascopyFallback).toBe(true);
  });

  it('counts only the current notice as consent', () => {
    expect(consented({ metatraderConsent: '2026-10' }, '2026-10')).toBe(true);
    expect(consented({ metatraderConsent: '2026-01' }, '2026-10')).toBe(false);
    expect(consented({ metatraderConsent: null }, '2026-10')).toBe(false);
    expect(consented(null, '2026-10')).toBe(false);
  });
});

describe('the terminal and account', () => {
  const other = terminal({ path: '/opt/broker/terminal64.exe', name: 'Broker MT5', accounts: [] });
  const saved = {
    metatraderTerminal: terminal().path,
    metatraderLogin: 222,
    metatraderServer: 'Broker-Demo',
  };

  it('keeps the saved ones while they are still there', () => {
    const found = detection([other, terminal()]);
    expect(chosenTerminal(found, saved)?.name).toBe('MetaTrader 5');
    expect(chosenAccount(chosenTerminal(found, saved), saved)?.login).toBe(222);
  });

  it('falls back to the first terminal and the account it was last on', () => {
    expect(chosenTerminal(detection([other, terminal()]), { metatraderTerminal: '/gone.exe' })?.name).toBe(
      'Broker MT5',
    );
    expect(chosenAccount(terminal(), { ...saved, metatraderLogin: 333 })?.login).toBe(111);
    // The saved account belongs to another terminal.
    expect(chosenAccount(terminal(), { ...saved, metatraderTerminal: other.path })?.login).toBe(111);
    expect(chosenAccount(other, saved)).toBeNull();
    expect(chosenTerminal(detection([]), saved)).toBeNull();
  });
});

describe('the text', () => {
  it('says how deep Max bars reaches', () => {
    expect(depthText(500_000)).toBe('about 16 months of 1-minute history');
    expect(depthText(100_000)).toBe('about 3 months of 1-minute history');
    expect(depthText(10_000)).toBe('under a month of 1-minute history');
    expect(depthText(5_000_000)).toBe('about 13 years of 1-minute history');
    expect(depthText(null)).toBeNull();
  });

  it('says what the source is doing, with when it is tried again', () => {
    const now = 1_000_000;
    expect(stateText(null)).toBe('Used once the market service runs');
    expect(stateText(status(), now)).toBe('Ready when Dukascopy cannot serve');
    expect(stateText(status({ mode: 'preferred' }), now)).toBe('Used for the next download');
    expect(stateText(status({ mode: 'only' }), now)).toBe('Used for the next download');
    expect(stateText(status({ state: 'off', mode: 'off' }), now)).toBe('Not used');
    expect(stateText(status({ state: 'error', message: 'Log in failed', retryAt: now + 125_000 }), now)).toBe(
      'Log in failed · again in 3 min',
    );
    expect(stateText(status({ state: 'paused', message: 'Paused', retryAt: now - 1 }), now)).toBe('Paused');
  });

  it('reports the installation', () => {
    expect(setupText({ installed: false, installing: true, message: 'Downloading cpython', error: null })).toBe(
      'Installing the bridge: Downloading cpython',
    );
    expect(setupText({ installed: false, installing: false, message: null, error: 'offline' })).toBe(
      'The bridge could not be installed: offline',
    );
    expect(setupText({ installed: true, installing: false, message: null, error: null })).toBeNull();
  });
});
