# BinanceBots App

BinanceBots is a private quantitative market-data, backtesting, and trading application for Binance, written in Rust with a plain HTML/CSS/JavaScript frontend.

## Current pages

- **Console** — low-bandwidth read-only status for persisted Bots and their active/latest Runs.
- **Market** — live Binance Spot / USD-M market view with candles, quote, depth, trades, and indicators.
- **Data Downloader** — catalogs and imports Binance historical 1-minute ZIP data into SQLite, with archive progress, failed-archive detail, editable start dates, and missing-data retries.
- **Backtest** — runs deterministic historical strategy replays on stored data and provides persisted run analysis.
- **Trading** — persistent Bot management plus detailed Live-Paper control/inspection, live market charting, orders, fills, portfolio state, Run activity, technical audit, and immutable Bot-scoped Run history. Live-Real-Account remains locked.
- **Security** — persisted login success, login failure, and logout audit events.

## Production

The app deploys from GitHub `efealt/BinanceBots` `main` to the existing Render service **BinanceBots**. The accepted public URL remains `https://binancegrid.onrender.com`.

Production uses `./target/release/binance-bots`, the persistent database `/var/data/binance_bots.sqlite3`, and `BINANCE_BOTS_*` runtime environment variables. All application pages, APIs, and WebSocket feeds require server-enforced single-user authentication; only the minimal health endpoint is always public.

## Data and runtime model

Live market working data is backend-owned. Historical market data, authentication audit records, Bots, immutable Runs, decisions, orders, fills, positions, and equity/PnL records are persisted in SQLite.

Browser sessions are presentation/control clients only. Live-Paper runtimes continue on the backend when the Trading page is closed or refreshed.

## Current roadmap position

Phases 1–4.9 are complete. The next substantive phase is **Phase 5 — Paper-period historical replay**, followed by Paper ↔ Backtest validation before any real-account order submission is enabled.
