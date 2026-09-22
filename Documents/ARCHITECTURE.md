# BinanceGrid Architecture

Status: current system  
Last updated: 2026-09-21

## Runtime

BinanceGrid is a Rust/Axum web application deployed on Render from GitHub `main`.

```text
Browser
  ↓ authenticated HTTP / WebSocket
Rust + Axum on Render
  ├─ Market service (live Binance public data, in memory)
  ├─ Historical data APIs
  ├─ Authentication/session layer
  └─ SQLite on Render persistent disk
```

The browser is a client only. Backend work does not depend on the browser remaining open.

## Deployment

- Repository: `efealt/BinanceGrid`
- Branch: `main`
- Render region: Frankfurt
- Build: `cargo build --release`
- Start: `./target/release/binance-grid`
- Health: `/api/health`
- Persistent disk: `/var/data`
- Production DB: `/var/data/binance_grid.sqlite3`
- Local default DB: `data/binance_grid.sqlite3`

Local and Render databases are separate files using the same migration-defined schema.

## Authentication

The hosted application is private.

- `/login` establishes a single-user session.
- Sessions are stored server-side in memory and expire after 12 hours or service restart.
- The session cookie is `HttpOnly`, `SameSite=Strict`, and `Secure` on Render.
- `/logout` invalidates the session and clears cached protected content.
- Protected HTML and API responses use no-store cache headers.
- `/api/health` is the only always-public application endpoint and returns only `{"status":"ok"}`.
- Credentials are supplied through Render environment variables and are never committed.

Authentication events are stored in SQLite as login success, login failure, and logout with timestamp, source IP, and user-agent. Passwords, session tokens, and page navigation are not logged.

## Market data

Market View supports Binance Spot and USD-M perpetual markets.

- REST bootstraps recent candles.
- Binance WebSockets provide candles, best bid/ask, partial depth, and recent trades.
- Rust owns the feed and keeps working state in memory.
- The browser receives an initial snapshot plus bounded live updates through the authenticated application WebSocket.
- Market View does not write its live working feed to SQLite.

## Historical data

The Data Downloader imports Binance public 1-minute ZIP archives into SQLite.

- Completed months use monthly archives.
- The current partial month uses daily archives through yesterday.
- Already imported archives are skipped.
- Spot and USD-M perpetual data remain separate datasets.
- Import receipts, coverage, checksums, and candle data are persisted.

Backtest reads stored historical data and can display 1-minute data or UTC-aggregated 1-hour/1-day views. Research diagnostics are computed from the stored series and rendered in the browser.

## Backtest engine

The backend now has a deterministic historical replay engine built on the canonical trading-run model. A completed historical candle becomes visible to normal `on_candle` strategy logic only at its close. Orders that were already resting before a candle is processed may fill from that candle's OHLC range; orders created from that candle's completed information cannot retroactively use its earlier open/high/low. The engine processes eligible resting orders before the candle-close strategy decision, keeps deterministic portfolio/order state, and persists decisions, intents, order states, fills, position snapshots, and equity snapshots through the shared Phase 1 storage contract.

Before the first active backtest candle is processed, the strategy receives an `on_start` hook. It may see the immediately preceding completed candle when one exists and may place initial resting orders at the first active candle's open-time boundary. This supports grid initialization without granting access to the first active candle's future high/low/close.

The strategy interface is mode-neutral and the simulated execution component is reusable by later Paper mode. Execution assumptions are explicit run metadata and currently support fees, spread/slippage, latency, touch vs trade-through limit fills, and deterministic partial fills. The Phase 2 engine itself contains no trading strategy.

## Phase 3 grid fixture and real-data regression

Phase 3 adds one deliberately simple mode-neutral `StaticGridStrategy` as an engineering fixture. It is configured through the shared strategy interface with an anchor source, spacing in basis points, levels per side, quantity per order, and time-in-force. The initial grid is emitted only through `on_start`; normal candle callbacks do not contain a Backtest-specific execution path.

The repository contains a frozen full real-market XAGUSDT USD-M 1-minute SQLite regression fixture at `test-data/xagusdt_1m_2026.sqlite3`. It contains 369,480 Binance public-data candles from 2026-01-07 10:00 UTC through 2026-09-20 23:59 UTC, uses schema version 4, contains no authentication rows or production trading-run history, and is checksum-locked by `test-data/xagusdt_1m_2026.manifest.json`.

Core trading CI copies the frozen fixture to a temporary writable database and runs the production Backtest engine twice over 369,479 active candles after one pre-roll candle. The Phase 3 engineering configuration uses a previous-close anchor, 100 bps spacing, three levels per side, quantity 1, GTC limits, 4 bps simulated fees, touch fills, zero latency, and full fills. These are regression-fixture values, not a trading recommendation or optimized strategy parameters.

The locked semantic regression result contains 1 decision, 6 order intents, 6 orders, 6 fills, 6 position snapshots, and 369,479 equity snapshots. Its semantic result SHA-256 is `e7c2fff2605a4642df7ea1ab4f26ef3ca5896fbc49c63527b28aba7e6a1f6e93`. CI fails if the fixture checksum or the deterministic semantic baseline changes unexpectedly.

Backtest equity persistence batches consecutive equity snapshots without changing event order. Buffered equity is flushed before any later fill/strategy event and at run completion, preserving the canonical event sequence while making full-history regression practical.

Phase 3 remains backend-only. The strategy code is deployed on Render, but Phase 3 did not claim or perform a production-database XAGUSDT run because no safe backend invocation surface exists yet. The master Phase 3.5 UI checkpoint will expose authenticated user-triggered Backtest execution and inspection.

## UI

- **Console** — current bot-console UI shell.
- **Market** — live market chart, quote, depth, trades, and indicators.
- **Data Downloader** — historical dataset creation, catalog, date editing, and missing-data import.
- **Backtest** — stored OHLCV charting and research diagnostics.
- **Security** — authenticated login-history audit view.

## Storage

SQLite uses migrations, WAL mode, foreign keys, and busy timeouts.

It stores:
- historical datasets and import receipts;
- historical OHLCV and related research data;
- live-capture tables used by the dedicated capture path;
- authentication audit events;
- the canonical trading-run model shared by future Backtest, Paper, and Live execution.

## Canonical trading-run persistence

Backtest, Paper, and Live use one mode-neutral persistence contract. Each run has a unique run ID, mode, strategy/version/configuration, instrument, optional comparison ID, and append-ordered events for decisions, order intents, order states, fills, position snapshots, and equity/PnL snapshots.

Trading values such as prices, quantities, fees, balances, and PnL are stored as canonical decimal text rather than binary floating-point. Event time, exchange time, receive time, and persistence time remain distinct. Order-state changes are append-only, and normal storage APIs do not delete Paper/Live history.

## Source of truth

- GitHub `main` is the code source of truth.
- Render is the production runtime.
- `Documents/ARCHITECTURE.md` describes the implemented architecture only.
