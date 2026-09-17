# Binance Grid Architecture

Status: living document  
Last updated: 2026-09-17

## Core decision

The live and paper trading paths run on live market data and keep their working state in memory. SQLite is not part of their hot path.

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
- The Backtest page selects an existing strategy and stored OHLCV data, then runs one backtest.
- Backtesting is isolated from live and paper trading.
- Data download and management belong to a separate Data Management page, not the Backtest page.

## UI boundaries

- Console: shows a top-row bot selector and the detail view for one selected bot. Selecting a card changes only the UI view; it does not affect any bot runtime.
- The console receives a list of bot snapshots from the backend; it does not run strategy logic.
- Market View: analyzes one selected market independently. It does not create bots, stage strategies, or run backtests. It bootstraps the latest 1,000 Binance candles into memory, then keeps the current candle updated from the public kline WebSocket. It does not write market data to SQLite. The browser renders the feed with TradingView Lightweight Charts; chart interaction stays in the UI layer.
- Chart annotations are reusable overlays configured by the chart caller. The current Market View enables a UTC weekend background overlay; the same chart component can enable it for Backtest later without duplicating page logic.
- Backtest: runs a selected strategy on stored historical data only.
- Data Management: will manage historical data downloads and storage when explicitly requested.

## Change rule

This document is the architecture source of truth. Any agreed architectural change must update this document in the same change.
