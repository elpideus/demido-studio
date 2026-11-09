// Which of a broker's MetaTrader symbols is which Dukascopy instrument. Brokers decorate names
// (EURUSDm, EURUSD.a, EURUSD.raw, EURUSD#, GER40.cash) and use index and commodity tickers
// (US30, GER40, USOIL), so a name is tried as given, then without what follows a separator, then
// without trailing lower-case letters, through the same resolver TradingView symbols use. Stock
// CFDs never match (they are no history source). A currency or metal pair must also agree with the
// symbol's base and profit currencies when the broker states them. When several symbols match one
// instrument, the undecorated name wins, then the instrument's own name over an alias (XAUUSD over
// GOLD), then one shown in Market Watch, then the shortest.
//
// API: instrumentFor(symbol) -> instrument | null;  mapSymbols(symbols) -> Map<instrument, symbol>

import { historyInstrument, instrumentInfo } from '../dukascopy.ts';

export interface BrokerSymbol {
  name: string;
  path?: string;
  description?: string;
  /** Base and profit currency, as the broker states them ('' when it does not). */
  base?: string;
  profit?: string;
  digits: number;
  visible?: boolean;
}

/** Spellings to try, the name itself first. */
function candidates(name: string): string[] {
  const out = [name];
  const head = name.split(/[._#+\-!/ ]/)[0] ?? '';
  if (head && head !== name) out.push(head);
  const bare = head.replace(/[a-z]+$/, '');
  if (bare && bare.length >= 3 && !out.includes(bare)) out.push(bare);
  return out;
}

function pairOf(instrument: string): [string, string] | null {
  const m = /^([A-Z]{3})\/([A-Z]{3})$/.exec(instrumentInfo(instrument).name);
  return m ? [m[1]!, m[2]!] : null;
}

interface Match {
  instrument: string;
  /** The name as given (nothing undecorated). */
  exact: boolean;
  /** The spelling is the instrument's own name, not an alias. */
  direct: boolean;
}

/** The Dukascopy instrument a broker symbol quotes. */
function match(symbol: BrokerSymbol): Match | null {
  for (const [i, spelling] of candidates(symbol.name.trim()).entries()) {
    // A ticker with a colon would read as an exchange prefix.
    const instrument = historyInstrument(spelling.replace(/:/g, ''));
    if (!instrument) continue;
    const pair = pairOf(instrument);
    const base = symbol.base?.trim().toUpperCase();
    const profit = symbol.profit?.trim().toUpperCase();
    if (pair && base && profit && (base !== pair[0] || profit !== pair[1])) continue;
    return { instrument, exact: i === 0, direct: spelling.toLowerCase().replace(/[^a-z0-9]/g, '') === instrument };
  }
  return null;
}

export function instrumentFor(symbol: BrokerSymbol): string | null {
  return match(symbol)?.instrument ?? null;
}

export function mapSymbols(symbols: readonly BrokerSymbol[]): Map<string, BrokerSymbol> {
  const best = new Map<string, { symbol: BrokerSymbol; m: Match }>();
  const better = (a: { symbol: BrokerSymbol; m: Match }, b: { symbol: BrokerSymbol; m: Match }) => {
    if (a.m.exact !== b.m.exact) return a.m.exact;
    if (a.m.direct !== b.m.direct) return a.m.direct;
    if (Boolean(a.symbol.visible) !== Boolean(b.symbol.visible)) return Boolean(a.symbol.visible);
    if (a.symbol.name.length !== b.symbol.name.length) return a.symbol.name.length < b.symbol.name.length;
    return a.symbol.name < b.symbol.name;
  };
  for (const symbol of symbols) {
    const m = match(symbol);
    if (!m) continue;
    const candidate = { symbol, m };
    const current = best.get(m.instrument);
    if (!current || better(candidate, current)) best.set(m.instrument, candidate);
  }
  return new Map([...best].map(([instrument, { symbol }]) => [instrument, symbol]));
}
