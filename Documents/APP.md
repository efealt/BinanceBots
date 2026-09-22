# BinanceGrid App

BinanceGrid is a private quantitative market-data and research application for Binance, written in Rust with a plain HTML/CSS/JavaScript frontend.

## Current pages

- **Console** — bot-console interface shell.
- **Market** — live Binance Spot / USD-M market view with candles, quote, depth, trades, SMA and Bollinger overlays.
- **Data Downloader** — imports and catalogs Binance historical 1-minute ZIP data in SQLite.
- **Backtest** — loads stored OHLCV, aggregates timeframes, and runs calendar/return diagnostics.
- **Security** — shows persisted login success, login failure, and logout events.

## Production

The app runs on Render from GitHub `main`, uses a persistent SQLite disk, and requires single-user login for all application pages, APIs, and WebSocket feeds. Only the minimal health endpoint is public.

## Data model

Live Market View data is kept in backend memory. Historical research data and authentication audit history are persisted in SQLite. Local and production databases are separate but use the same migrations.
