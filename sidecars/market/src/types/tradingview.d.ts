// Minimal typings for @mathieuc/tradingview (the package ships JavaScript with JSDoc only).
// Only the parts Demido uses are declared.

declare module '@mathieuc/tradingview' {
  export interface PricePeriod {
    time: number;
    open: number;
    close: number;
    max: number;
    min: number;
    volume: number;
  }

  export interface MarketInfos {
    description?: string;
    short_description?: string;
    exchange?: string;
    listed_exchange?: string;
    currency_code?: string;
    currency_id?: string;
    pricescale?: number;
    minmov?: number;
    type?: string;
    pro_name?: string;
    full_name?: string;
    name?: string;
    timezone?: string;
    session?: string;
    base_currency?: string;
    [key: string]: unknown;
  }

  export interface ChartSession {
    readonly periods: PricePeriod[];
    readonly infos: MarketInfos;
    setMarket(symbol: string, options?: { timeframe?: string; range?: number; to?: number; session?: string }): void;
    setSeries(timeframe?: string, range?: number, reference?: number | null): void;
    fetchMore(count?: number): void;
    onSymbolLoaded(cb: () => void): void;
    onUpdate(cb: (changes: string[]) => void): void;
    onError(cb: (...err: unknown[]) => void): void;
    delete(): void;
  }

  export interface QuoteMarket {
    onLoaded(cb: () => void): void;
    onData(cb: (data: Record<string, unknown>) => void): void;
    onError(cb: (...err: unknown[]) => void): void;
    close(): void;
  }

  export interface QuoteSession {
    Market: new (symbol: string, session?: string) => QuoteMarket;
    delete(): void;
  }

  export class Client {
    constructor(options?: { token?: string; signature?: string; server?: string; location?: string; DEBUG?: boolean });
    readonly isOpen: boolean;
    readonly isLogged: boolean;
    onConnected(cb: () => void): void;
    onDisconnected(cb: () => void): void;
    onError(cb: (...err: unknown[]) => void): void;
    Session: {
      Quote: new (options?: { fields?: 'all' | 'price'; customFields?: string[] }) => QuoteSession;
      Chart: new () => ChartSession;
    };
    end(): Promise<void>;
  }

  export interface SearchResult {
    id: string;
    exchange: string;
    fullExchange: string;
    symbol: string;
    description: string;
    type: string;
  }

  export interface User {
    id: number | string;
    username: string;
    authToken: string;
  }

  export function searchMarketV3(search: string, filter?: string, offset?: number): Promise<SearchResult[]>;
  export function getUser(session: string, signature?: string, location?: string): Promise<User>;

  const TradingView: {
    Client: typeof Client;
    searchMarketV3: typeof searchMarketV3;
    getUser: typeof getUser;
  };
  export default TradingView;
}

declare module '@mathieuc/tradingview/src/miscRequests.js' {
  const miscRequests: {
    getUser: (
      session: string,
      signature?: string,
      location?: string,
    ) => Promise<{ id: number | string; username: string; authToken: string }>;
    [key: string]: unknown;
  };
  export default miscRequests;
}
