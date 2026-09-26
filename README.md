# BinanceBots

Private Rust/Axum quantitative research and trading console for Binance.

## Current production identity

- GitHub source of truth: `efealt/BinanceBots` on `main`
- Render service: **BinanceBots**
- Production URL: `https://binancegrid.onrender.com` (accepted legacy Render slug)
- Production binary: `./target/release/binance-bots`
- Production database: `/var/data/binance_bots.sqlite3`
- Local default database: `data/binance_bots.sqlite3`
- Runtime environment variables use the `BINANCE_BOTS_*` namespace

The hosted application is private. Browser sessions are clients only; long-running Live-Paper runtimes and market services are backend-owned.

## Local run

Run `cargo run`, then open `http://127.0.0.1:8080`.

To capture one minute of public Binance WebSocket market data into SQLite:

```text
cargo run -- --capture-live --duration-seconds 60
```

The default capture is BTCUSDT Spot. Use `--symbol`, `--interval`, `--market-type`, `--duration-seconds`, and `--database` to change it. No API keys are required for public market data.

## Project documents

- `AGENTS.md` — working/development rules
- `Documents/APP.md` — current application surface
- `Documents/ARCHITECTURE.md` — implemented architecture source of truth
- `Roadmaps/00-master-trading-system-roadmap.md` — remaining staged trading-system work

Phases 1–4.9 are complete. The next substantive phase is **Phase 5 — Paper-period historical replay**.
