---
name: Market analysis
description: How to answer questions about prices, trends and market data with the market and Python tools.
---

Use this for any question about a market: a price, how something moved, a comparison, a chart.

1. **Find the symbol.** TradingView symbols are `EXCHANGE:TICKER`. Common ones: `FX:EURUSD`, `OANDA:XAUUSD` (gold), `BINANCE:BTCUSDT`, `SP:SPX`, `NASDAQ:NDX`, `NASDAQ:AAPL`. If you are not sure, call `market_search` first and pick the most liquid match.
2. **Current price or today's move:** call `market_quote`. Report the price, the change and the day's range.
3. **History or trends:** call `market_candles` with a timeframe that fits the question (`1h` for days, `1d` for weeks to months, `1w` for years). Ask for enough candles (for example 90 daily candles for "the last three months"). For years of forex, metals or index history without TradingView, use `market_history`.
4. **Analysis:** the candles are saved as a CSV file in the workspace (the tool result gives its path). Load it with pandas in `run_python` instead of reading it yourself:
   ```python
   import pandas as pd
   df = pd.read_csv("data/FX_EURUSD_1d_....csv", parse_dates=["time"])
   ```
   Print the numbers you need. For a chart, use matplotlib and `plt.savefig("chart.png", dpi=120, bbox_inches="tight")`.
5. **Answer** with the key numbers first, then a short interpretation. Use a table for comparisons. Say where the data came from (TradingView or Dukascopy) and the time range covered.

Never present analysis as financial advice.
