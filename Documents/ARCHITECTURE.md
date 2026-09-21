# Binance Grid Architecture

Status: living document  
Last updated: 2026-09-21

## Core decision

The live and paper trading paths run on live market data and keep their working state in memory. SQLite is not part of their hot path. A separate capture process may persist public market data asynchronously for research and later reconstruction.

Render is the intended always-on production deployment target. The hosted Rust backend owns long-running work; browser sessions are authenticated clients of that backend and must never own or determine the lifetime of a bot runtime.

## Development and operating model

```text
User / quant analyst
        ↓
ChatGPT Chat + connected GitHub
        ↓
GitHub main (code source of truth)
        ↓
Render build + always-on runtime
        ↓
Authenticated private application
        ↓
Browser client / ChatGPT-assisted inspection
```

- ChatGPT Chat with connected GitHub access is the primary coding and maintenance interface for this project.
- GitHub `main` is the canonical codebase. Approved code and documentation changes are made there directly.
- Render is the primary runtime and deployment-verification environment. Build failures, hosted behavior, persistence, market connectivity, and later backend bot behavior are verified against the Render deployment.
- The user's Mac remains optional rather than being a required development or deployment gate. It may still be used for local research, heavy backtests, ML training, or tasks that specifically require local execution.
- Deployed pages, diagnostics, historical data, market feeds, APIs, WebSockets, and future trading controls are part of one private application and require authentication.
- Operational diagnosis does not depend on a public observer surface. ChatGPT can inspect GitHub source/history and, when connected, Render deploy status and runtime logs; authenticated browser access is used for rendered application verification.
- This workflow does not make the browser part of the trading runtime; the authenticated browser remains a client of the Render-hosted backend.

## Deployment and persistence

```text
GitHub main → Render Web Service → Rust/Axum backend
                                  ↓
                         persistent SQLite disk
```

- Render is the intended production host for the always-on application.
- Production SQLite lives on a Render persistent disk. It must not depend on Render's ephemeral service filesystem for durable state.
- Local development keeps a separate local SQLite database. Local and production databases may contain different data while sharing the same migration-defined schema.
- Deployment configuration such as the listening port, database path, credentials, and other secrets belongs in environment configuration rather than source code.
- Closing, refreshing, or disconnecting a browser must not stop backend market feeds, bot runtimes, or other server-owned work.

## Access boundary

The hosted application uses a full private-access model rather than a public observer/private control split.

```text
Unauthenticated internet
        ↓
minimal health check
        +
authentication entrypoint only if required
        ↓
authenticated BinanceGrid application
        ↓
pages + APIs + WebSockets + diagnostics + controls
```

- All application pages, static application assets, historical-data views, Data Downloader functions, Backtest views, Market View feeds, APIs, WebSockets, diagnostics, bot state, and future trading controls require server-enforced authentication.
- The Render health-check route is the only route that is always intentionally unauthenticated. It returns only minimal process-health status and never exposes market data, database contents, bot state, credentials, secrets, or configuration.
- If the selected authentication design needs a login/session bootstrap endpoint, only the minimum authentication route(s) needed to establish the session may be unauthenticated. They must expose no application data.
- Authentication is enforced by the Rust/Axum backend before protected application content or endpoints are served. Client-side hiding is not authorization.
- Credentials, session signing material, Binance API keys, and other secrets are supplied through environment configuration or equivalent server-side secret storage and are never committed to Git or embedded in client JavaScript.
- GitHub and Render operational access remain separate from application login. Repository inspection and Render runtime logs can be used to diagnose failures even when the web application itself is unavailable.
- The concrete single-user implementation uses a Rust/Axum login form at `/login`, opaque in-memory session IDs stored in an `HttpOnly; SameSite=Strict` cookie, and `/logout` session invalidation. Sessions expire after 12 hours and are invalidated by a service restart. Render-hosted cookies are marked `Secure`.
- Authentication is enabled by default on Render and can be explicitly disabled only with `BINANCE_GRID_AUTH_MODE=disabled` for controlled setup/development. When enabled, `BINANCE_GRID_AUTH_USERNAME` and a password of at least 12 characters in `BINANCE_GRID_AUTH_PASSWORD` are required at startup; missing/invalid production credentials fail closed.
- The full protected router includes application pages/static assets, `/api/data/...`, `/api/market/...`, and the market WebSocket handshake. `/api/health`, `/login`, and `/logout` are outside that protected router; only the login/session routes perform authentication work and expose no application data.
- This full-private boundary is the target access model for the hosted application; the active Render roadmap tracks its implementation and verification.

## Live and paper trading

```text
Live market data → in-memory bot runtimes → live or paper executor → bot snapshots → UI
```

- Each bot is an independent in-memory runtime with its own bot ID, strategy, symbol, and state.
- One grid bot runs one symbol. Multiple bots may run at the same time.
- Bot runtimes are backend-owned and independent of any browser session.
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
- Market View: analyzes one selected market independently. It does not create bots, stage strategies, or run backtests. It supports Binance Spot markets and USDⓈ-M perpetual futures, bootstraps the latest 1,000 candles into memory, then keeps the current candle, best bid/ask quote, partial order-book depth, and recent public trades updated from the product's public WebSocket streams. Rust processes the exchange events continuously; the authenticated browser receives one initial snapshot followed by bounded, coalesced WebSocket updates instead of periodic REST polling. The application WebSocket endpoint itself is private and requires authentication even though its upstream Binance source is public. It does not write market data to SQLite. The browser renders the feed with TradingView Lightweight Charts; chart interaction stays in the UI layer.
- Chart annotations are reusable time-window overlays configured by the chart caller. Market View enables the UTC weekend background overlay. Backtest enables selectable UTC weekend and after-hours shading; metals (`XAG`, `XAU`, `XPT`, and `XPD`) use the 22:00–07:00 UTC overnight window, while other symbols use the default 16:00–20:00 UTC research window. Overlay preferences persist in browser storage while the chart changes timeframe.
- Chart indicators are reusable UI-layer modules under `web/chart/indicators/`. Indicator calculations are separate from chart rendering and toolbar state. Market View initially exposes Simple Moving Average and Bollinger Bands with editable parameters; active settings remain in browser memory and are not written to SQLite.
- Backtest: runs a selected strategy on stored historical data only. Beneath the OHLCV chart, its Calendar Effects diagnostics recompute return distributions, candle ranges, base-volume profiles, average returns by UTC hour or weekday, and compounded hourly returns with mean summaries for the full sample, weekends, remaining weekday hours, post-New York/pre-Asia (21:00–00:00 UTC), and middle Asia (02:00–05:00 UTC) at the selected 1-minute, 1-hour, or 1-day chart interval. Diagnostic visuals use Apache ECharts while the OHLCV chart remains on Lightweight Charts. Session-based rows are unavailable at daily resolution because a daily candle spans the full day. The cumulative hourly return card is available at the 1-hour interval and groups one-hour open-to-open returns by UTC entry hour, with no trading costs applied. A separate rolling same-hour prediction card uses a 30-occurrence window selectable over the full sample, weekdays, or weekends: for each UTC hour, a positive average of the prior 30 same-hour returns predicts positive for the next occurrence; the card reports correct and false predictions by hour.
- Data Downloader: stores user-defined 1-minute kline entries in SQLite, including provider, symbol, market type, name, interval, a persisted UTC start date, and current download coverage metadata. Saved catalog entries expose a modal start-date editor; after saving an earlier date, Download missing reuses the updated date and fills only archive history not already recorded. It imports Binance public-data ZIPs only: completed calendar months use monthly archives and the current partial month uses daily archives through yesterday. Each run skips ZIPs already recorded as imported, so it requests only missing history. A process-wide single-run guard and a two-second delay between archives prevent concurrent or rapid archive requests. Spot uses `data/spot/...`; USDⓈ-M perpetual uses `data/futures/um/...`; they remain separate SQLite instruments and datasets.

## Change rule

This document is the architecture source of truth. Any agreed architectural change must update this document in the same change.
