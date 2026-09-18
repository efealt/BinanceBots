# Binance Grid Architecture

Status: living document  
Last updated: 2026-09-17

## Core decision

The live and paper trading paths run on live market data and keep their working state in memory. SQLite is not part of their hot path. A separate capture process may persist public market data asynchronously for research and later reconstruction.

## Live and paper trading

```text
Live market data → in-memory bot runtimes → live or paper executor → bot snapshots → UI
```

- Each bot is an independent in-memory runtime with its own bot ID, strategy, symbol, and state.
- One grid bot runs one symbol. Multiple bots may run at the same time.
- Live and paper use the same live market data and strategy engine.
- Only execution differs: exchange orders for live trading, simulated fills for paper trading.
- Grid decisions, order handling, position state, and UI reads must not wait on SQLite.
- The UI reads backend snapshots; it does not sit in the execution path.
- On restart, the exchange is the source of truth for open orders, balances, and order updates.
- A background order/fill journal may be added later for audit or recovery, but only if it never blocks execution.

## Historical backtesting

```text
OHLCV download → SQLite → backtest runner → Backtest UI
```

- SQLite stores downloaded historical OHLCV data for backtests.
- Historical data is organized as `historical_datasets` plus `historical_ohlcv`, with its Binance REST source and time range recorded.
- The Backtest page selects an existing strategy and stored OHLCV data, then runs one backtest.
- Backtesting is isolated from live and paper trading.
- Data download and management belong to a separate Data Downloader page, not the Backtest page.

## Market data storage

```text
Binance WebSocket capture → raw live events + normalized live tables → SQLite
Binance historical download → historical dataset + OHLCV rows → SQLite
```

- `market_instruments` is the shared identity for a Binance symbol and market type, so Spot and USDⓈ-M perpetual data cannot be confused.
- Live capture stores the raw WebSocket payload and normalized kline updates, book-ticker quotes, top-depth snapshots and levels, and trades. The raw event log remains the source record for later parsing or candle reconstruction.
- Historical OHLCV and live WebSocket data remain physically separate, then can be selected together through the source-labelled `market_candles` view when a research workflow explicitly needs both.
- The current Market View still keeps its working feed in memory; only the dedicated capture command writes to SQLite.

## UI boundaries

- Console: shows a top-row bot selector and the detail view for one selected bot. Selecting a card changes only the UI view; it does not affect any bot runtime.
- The console receives a list of bot snapshots from the backend; it does not run strategy logic.
- Market View: analyzes one selected market independently. It does not create bots, stage strategies, or run backtests. It supports Binance Spot markets and USDⓈ-M perpetual futures, bootstraps the latest 1,000 candles into memory, then keeps the current candle, best bid/ask quote, partial order-book depth, and recent public trades updated from the product's public WebSocket streams. Rust processes the exchange events continuously; the browser receives one initial snapshot followed by bounded, coalesced WebSocket updates instead of periodic REST polling. It does not write market data to SQLite. The browser renders the feed with TradingView Lightweight Charts; chart interaction stays in the UI layer.
- Chart annotations are reusable overlays configured by the chart caller. The current Market View enables a UTC weekend background overlay; the same chart component can enable it for Backtest later without duplicating page logic.
- Chart indicators are reusable UI-layer modules under `web/chart/indicators/`. Indicator calculations are separate from chart rendering and toolbar state. Market View initially exposes Simple Moving Average and Bollinger Bands with editable parameters; active settings remain in browser memory and are not written to SQLite.
- Data Diagnostics: read-only analysis of saved WebSocket captures or downloaded candles. WebSocket diagnostics load all trades, quotes, and captured partial-depth snapshots. Separate charts show executed prices, aggressive volume, trade sizes, spread, activity, cumulative volume delta, quote-size imbalance, and historical depth. Time charts use actual receipt timestamps on a linear UTC axis, preserve simultaneous events, and share a user-selected interval (full capture by default). Histograms include every selected observation; activity buckets disclose their duration. No raw JSON or event tables are exposed. REST diagnostics fetch every candle page for price, volume, and return distributions. Rendering and calculations are separate modules under web/diagnostics; no external chart dependency is needed by this page. The page never downloads exchange data or writes storage.
- Backtest: runs a selected strategy on stored historical data only.
- Data Downloader: stores user-defined historical data entries in SQLite, including provider, symbol, market type, name, interval, and the current download coverage metadata. The first slice only saves definitions and exposes a clearly non-functional download action; archive backfills and incremental Binance downloads are separate follow-up work.

## Change rule

This document is the architecture source of truth. Any agreed architectural change must update this document in the same change.
