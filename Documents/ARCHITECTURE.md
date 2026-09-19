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
Binance archive/API data → normalized historical tables in SQLite → backtest runner → Backtest UI
```

- SQLite is the historical research and backtesting store; it is not used by the live execution path.
- `historical_datasets` is the canonical catalog for one instrument, data kind, and interval. `historical_imports` records each daily/monthly ZIP or REST backfill that populated it, including coverage, checksum, row count, and outcome.
- Historical tables are separated by data shape: traded OHLCV, mark/index/premium reference candles, raw trades, aggregate trades, funding-rate events, best-bid/ask quotes, and depth snapshots with levels.
- Historical instrument rules, fee schedules, and maintenance-margin brackets are versioned separately so a backtest can apply the rules that were effective at the time.
- The Backtest page begins with an Initial Data Selector: it selects one stored OHLCV dataset and renders its complete candle series with OHLC candles and base-asset volume in separate chart panes. The selector can display the stored 1-minute series or UTC-aggregated 1-hour/1-day views; aggregation uses first open, extreme high/low, last close, and summed volume/trade fields. Strategy inputs and backtest outputs remain separate, subsequent slices.
- Backtesting is isolated from live and paper trading.
- Data download and management belong to a separate Data Downloader page, not the Backtest page.

## Market data storage

```text
Binance WebSocket capture → raw live events + normalized live tables → SQLite
Binance historical download → dataset catalog + import receipt + normalized historical rows → SQLite
```

- `market_instruments` is the shared identity for a Binance symbol and market type, so Spot and USDⓈ-M perpetual data cannot be confused.
- Live capture stores the raw WebSocket payload and normalized kline updates, book-ticker quotes, top-depth snapshots and levels, and trades. The raw event log remains the source record for later parsing or candle reconstruction.
- Historical backtest data and live WebSocket captures remain physically separate. The source-labelled `market_candles` view only joins traded historical candles with live-capture candles when a research workflow explicitly needs both.
- The current Market View still keeps its working feed in memory; only the dedicated capture command writes to SQLite.

## UI boundaries

- Console: shows a top-row bot selector and the detail view for one selected bot. Selecting a card changes only the UI view; it does not affect any bot runtime.
- The console receives a list of bot snapshots from the backend; it does not run strategy logic.
- Market View: analyzes one selected market independently. It does not create bots, stage strategies, or run backtests. It supports Binance Spot markets and USDⓈ-M perpetual futures, bootstraps the latest 1,000 candles into memory, then keeps the current candle, best bid/ask quote, partial order-book depth, and recent public trades updated from the product's public WebSocket streams. Rust processes the exchange events continuously; the browser receives one initial snapshot followed by bounded, coalesced WebSocket updates instead of periodic REST polling. It does not write market data to SQLite. The browser renders the feed with TradingView Lightweight Charts; chart interaction stays in the UI layer.
- Chart annotations are reusable time-window overlays configured by the chart caller. Market View enables the UTC weekend background overlay. Backtest enables selectable UTC weekend and after-hours shading; metals (`XAG`, `XAU`, `XPT`, and `XPD`) use the 22:00–07:00 UTC overnight window, while other symbols use the default 16:00–20:00 UTC research window. Overlay preferences persist in browser storage while the chart changes timeframe.
- Chart indicators are reusable UI-layer modules under `web/chart/indicators/`. Indicator calculations are separate from chart rendering and toolbar state. Market View initially exposes Simple Moving Average and Bollinger Bands with editable parameters; active settings remain in browser memory and are not written to SQLite.
- Backtest: runs a selected strategy on stored historical data only. Beneath the OHLCV chart, its Calendar Effects diagnostics recompute return distributions, candle ranges, base-volume profiles, average returns by UTC hour or weekday, and compounded hourly returns with mean summaries for the full sample, weekends, remaining weekday hours, post-New York/pre-Asia (21:00–00:00 UTC), and middle Asia (02:00–05:00 UTC) at the selected 1-minute, 1-hour, or 1-day chart interval. Diagnostic visuals use Apache ECharts while the OHLCV chart remains on Lightweight Charts. Session-based rows are unavailable at daily resolution because a daily candle spans the full day. The cumulative hourly return card is available at the 1-hour interval and groups one-hour open-to-open returns by UTC entry hour, with no trading costs applied. A separate rolling same-hour prediction card uses a 30-occurrence window selectable over the full sample, weekdays, or weekends: for each UTC hour, a positive average of the prior 30 same-hour returns predicts positive for the next occurrence; the card reports correct and false predictions by hour.
- Data Downloader: stores user-defined 1-minute kline entries in SQLite, including provider, symbol, market type, name, interval, a persisted UTC start date, and current download coverage metadata. Saved catalog entries expose a modal start-date editor; after saving an earlier date, Download missing reuses the updated date and fills only archive history not already recorded. It imports Binance public-data ZIPs only: completed calendar months use monthly archives and the current partial month uses daily archives through yesterday. Each run skips ZIPs already recorded as imported, so it requests only missing history. A process-wide single-run guard and a two-second delay between archives prevent concurrent or rapid archive requests. Spot uses `data/spot/...`; USDⓈ-M perpetual uses `data/futures/um/...`; they remain separate SQLite instruments and datasets.

## Change rule

This document is the architecture source of truth. Any agreed architectural change must update this document in the same change.
