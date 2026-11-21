# 09. Markets: TradingView accounts and Pine Script

Written 2026-11-20, directly (no research run). Asked by the owner: load a user's TradingView indicators and
candle colours when they sign in, and run Pine Script indicators locally so they never count against an account.

Each finding says how well it is checked. **Read** means I opened the page myself. **Search** means I have only
search results and their summaries; check before relying on it.

## 1. Reading a TradingView account from another app

| Finding | Checked |
|---|---|
| TradingView's terms forbid automated data collection by any means ("scripts, APIs, screen scraping, data mining, robots") and for any purpose. The Help Center adds that outside software, tools, scripts, bots and extensions are not allowed and that features are for manual use only. | **Read**: Help Center, "Why is my account banned due to suspicious activity?" |
| The penalty is a ban: temporary for the first violations, permanent after repeated ones. It covers all charting features of the account. | **Read**, same page |
| TradingView's market data is licensed for display only. Tools that enable other use are not allowed. | Search (policies page; snippet was garbled) |
| There is no official API through which another app reads a user's saved layouts, indicators or colours on tradingview.com. The "save and load" API belongs to the Charting Library, a product for sites that host TradingView's chart themselves and store layouts on their own server. | Search (Charting Library docs) |

**What follows.** Copying a user's indicators and colours automatically means reading the account by program,
with the user's own login. That is exactly what the terms forbid. The user risks the account. A project that
ships a tool for it takes a risk too. There is no allowed automatic route today.

**What is allowed.** Anything the user does by hand: reading their own settings, copying the source of their
own scripts out of the Pine Editor, pasting it elsewhere.

**What can never be copied, by any route.** Scripts whose source is closed (invite-only and protected ones).
Only their output is visible on TradingView.

## 2. Running Pine Script locally

Pine Script is TradingView's language. Its real runtime runs on TradingView's servers. Running a script
anywhere else needs a separate engine that imitates it.

| Candidate | What it is | Licence | Checked |
|---|---|---|---|
| PineTS (LuxAlgo, earlier QuantForge) | A runtime in TypeScript for Node.js and the browser. Takes Pine v5 and v6 source. Its docs call native Pine support experimental. | AGPL-3.0, or a paid commercial licence | Licence **read** on the GitHub page. The rest: search |
| PyneCore (PyneSys) | A Python runtime that behaves like Pine. | Runtime: Apache-2.0. The compiler that turns Pine into Python is closed and offered as a paid online service with an API key. | Search |
| An engine of our own | Parser and interpreter for the language, plus the built-in functions (several hundred). | Ours | — |

Notes for the decision, which is not made:

- **AGPL.** The brief allows AGPL parts only through GPLv3 section 13 or as a separate process. PineTS fits
  either way, but Demido has a server mode, and AGPL's network clause then applies to that part. For an open
  project that is workable. It needs a proper licence read first.
- **PyneCore is out** as it stands: turning Pine into something it can run needs an online paid service. That
  is neither local nor free of accounts.
- **No engine matches TradingView exactly.** Scripts can behave differently there (data, bar timing, functions
  the engine lacks). The UI must say so.
- **Model-written scripts are untrusted code.** Whatever engine runs them, it runs in the sandbox.
- **Names.** "Pine Script" is TradingView's trademark. Say "compatible with", and state that Demido is not
  affiliated.

## 3. "No limit on indicators"

TradingView's free plan limits indicators per chart (two, according to several third-party pages; **search**,
not confirmed on TradingView's pricing page). Demido's own chart has no such limit, because Demido works the
indicators out itself. That is a different product doing its own work, not a way around TradingView's plans.
Demido must not draw into TradingView's chart or lift limits inside TradingView.

## 4. Drawing the chart

| Candidate | Licence | Note | Checked |
|---|---|---|---|
| Lightweight Charts (TradingView) | Apache-2.0 | The licence asks for TradingView's attribution notice and a link to tradingview.com in the app. | Search |
| A renderer of our own | Ours | The preview uses plain SVG; fine for a few hundred candles. | Built |

## 5. Where prices come from

Not from TradingView (section 1). Public exchange feeds for crypto are the obvious start. Stocks and currencies
need a data provider whose terms allow it. **Not researched.**

## Sources

- TradingView Help Center, "Why is my account banned due to suspicious activity?":
  <https://www.tradingview.com/support/solutions/43000674726-why-is-my-account-banned-due-to-suspicious-activity/>
- TradingView, Terms of Use, Policies and Disclaimers: <https://www.tradingview.com/policies/>
- TradingView Charting Library, saving and loading charts:
  <https://www.tradingview.com/charting-library-docs/latest/saving_loading/>
- PineTS: <https://github.com/LuxAlgo/PineTS>
- PyneCore: <https://pypi.org/project/pynesys-pynecore/>
- Lightweight Charts: <https://www.npmjs.com/package/lightweight-charts>
