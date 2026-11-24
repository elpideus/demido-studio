# 33. Markets: price feeds, the chart, and a Pine-compatible engine

Written 2026-11-24, directly, for plan item 9's Markets (DECISIONS M2, M3, M4, M5, M7, M9). Nothing here places an
order, and nothing reads TradingView (M6, M8, M10 stay as they are).

How sure each line is: **Read** (the provider's own page, 24 November 2026), **Tried** (asked from this computer
the same day, without a key).

## Price feeds

| Feed | What | Key | Tried | Terms |
|---|---|---|---|---|
| **Kraken**, `GET https://api.kraken.com/0/public/OHLC?pair=XBTEUR&interval=1440` | Candles of a crypto pair: time, open, high, low, close, VWAP, volume, count; intervals 1, 5, 15, 30, 60, 240, 1440, 10080, 21600 minutes. "Returns up to 720 of the most recent entries", and "the last entry … is for the current, not-yet-committed timeframe" (Read, its documentation) | None | Answered 720 daily candles of XBT/EUR | Its legal pages need scripts to show their text, and were not read |
| **Coinbase Exchange**, `GET https://api.exchange.coinbase.com/products/BTC-EUR/candles?granularity=86400` | Candles: time, low, high, open, close, volume, newest first | None | Answered | "a … limited license, solely for you"; you may not "Redistribute, display, or disseminate the Market Data … to any third party" without consent (<https://www.coinbase.com/legal/market_data>). A person's own Demido showing them prices is "for you"; a Demido server showing others would not be |
| **Frankfurter** (`api.frankfurter.dev`), the European Central Bank's reference rates | Daily rates of about 30 currencies against the euro, history from 1999 | None | Answered EUR→USD 1.0939 for 23 November | The service is MIT-licensed and open; the rates are the ECB's, published for information |
| Stooq (stocks, CSV) | Daily candles of stocks and indices | None | **Refused**: "This site requires JavaScript to verify your browser" | Reading it would mean getting past a check meant for people (N7): not used |
| Alpha Vantage, Twelve Data, Finnhub and the like (stocks) | Stocks with a key | A key | Not tried | Each its own; they wait for a key of the person's, as search did (N17) |

So: **crypto from Kraken first, Coinbase second; currencies from the ECB through Frankfurter (daily only);
stocks not built** (no keyless feed that does not hide behind a check for people).

## The chart

- **TradingView's Lightweight Charts** is Apache-2.0, and its NOTICE reads "TradingView Lightweight Charts™
  Copyright (с) 2025 TradingView, Inc. https://www.tradingview.com/" (Read): the licence asks that notice to be
  kept, and its README asks for an attribution link in the page.
- **Demido's own** draws candles, a volume pane and lines with SVG, as the preview does already (M2, M14):
  nothing more to load, nothing to attribute, the brand's own colours (M1), and the same code for every theme.
  For a few hundred candles at a time SVG is quick enough.

**Taken: Demido's own** (M9 decided). What is given up: Lightweight Charts' smooth panning of tens of thousands of
points.

## Indicators (M2)

Worked out by the core, not the window, so that a model and a Pine script use the same numbers: SMA, EMA
(seeded with the SMA of its first window, as TradingView's `ta.ema` is), Bollinger Bands (SMA ± k population
standard deviations), RSI (Wilder's smoothing, as `ta.rsi`), MACD (12, 26, 9 by default), volume. Checked in
tests against values worked out by hand.

## A Pine-compatible engine of Demido's own (M3, M4, M5)

PineTS is AGPL-3.0 (M4) and PyneCore's compiler is a closed service: neither is taken. An engine of Demido's own,
written in Rust, for **a first part of the language**, enough for the indicators people and small models write:

- `//@version=5` or `6`, `indicator(title, overlay=…)`;
- `name = expression`, `var` names, `input.int`, `input.float`, `input.bool` (their default is used);
- the series `open`, `high`, `low`, `close`, `volume`, `hl2`, `hlc3`, `ohlc4`, and `x[n]` for n bars back;
- numbers, `true`, `false`, `na`, `+ - * / %`, comparisons, `and`, `or`, `not`, `cond ? a : b`;
- `ta.sma`, `ta.ema`, `ta.rma`, `ta.rsi`, `ta.stdev`, `ta.highest`, `ta.lowest`, `ta.change`, `ta.crossover`,
  `ta.crossunder`, `ta.tr`, `ta.atr`; `math.abs`, `math.max`, `math.min`, `math.sqrt`, `math.pow`, `math.log`,
  `math.round`; `nz`, `na()`;
- `plot(series, title, color=…)`, `hline(price, title)`, `color.<name>` and `#rrggbb`.

Evaluated a whole series at a time, which is what these scripts mean. What it is not, and says so: `if`, `for`,
`while`, user functions, `request.*`, strategies, drawings (`line.new`, `label.new`), tables. An error names its
line and column and what was expected, in words a model can act on: the loop M3 asks for (write, run, read the
error, mend). It is not TradingView's engine, and a script may behave differently there (M5).

## Not done

Live ticks by WebSocket (the candles are asked again while the screen is open, no more often than every 15 s),
stocks, drawings kept, alerts, saved layouts, backtesting.
