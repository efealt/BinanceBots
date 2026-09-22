# BinanceGrid Architecture

Status: current system  
Last updated: 2026-09-22

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

## Strategy organization

Strategy implementations are separated from the shared trading engine under `src/trading/strategies/`.

Current structure:

```text
src/trading/
  mod.rs
  types.rs
  portfolio.rs
  simulation.rs
  strategies/
    mod.rs
    static_grid.rs
    dynamic_grid.rs
    volatility_grid.rs
    mean_reversion.rs
    breakout.rs
```

`src/trading/types.rs` owns the shared mode-neutral `Strategy` interface and strategy input/output types. `portfolio.rs` owns portfolio/accounting state, and `simulation.rs` owns simulated execution. Strategy implementations do not live inside the Backtest engine.

The Phase 3 fixture now lives in `src/trading/strategies/static_grid.rs`. The dynamic-grid, volatility-grid, mean-reversion, and breakout modules are intentionally reserved placeholders only; no trading rules have been invented for them.

One strategy concept should have one module. If a future strategy becomes large, that module may become its own folder while continuing to implement the same shared `Strategy` interface. Backtest, Paper, and Live must call the same strategy implementation rather than maintaining mode-specific copies.

## Phase 3 grid fixture and real-data regression

Phase 3 adds one deliberately simple mode-neutral `StaticGridStrategy` as an engineering fixture. It is configured through the shared strategy interface with an anchor source, spacing in basis points, levels per side, quantity per order, and time-in-force. The initial grid is emitted only through `on_start`; normal candle callbacks do not contain a Backtest-specific execution path.

The repository contains a frozen full real-market XAGUSDT USD-M 1-minute SQLite regression fixture at `test-data/xagusdt_1m_2026.sqlite3`. It contains 369,480 Binance public-data candles from 2026-01-07 10:00 UTC through 2026-09-20 23:59 UTC, uses schema version 4, contains no authentication rows or production trading-run history, and is checksum-locked by `test-data/xagusdt_1m_2026.manifest.json`.

Core trading CI copies the frozen fixture to a temporary writable database and runs the production Backtest engine twice over 369,479 active candles after one pre-roll candle. The Phase 3 engineering configuration uses a previous-close anchor, 100 bps spacing, three levels per side, quantity 1, GTC limits, 4 bps simulated fees, touch fills, zero latency, and full fills. These are regression-fixture values, not a trading recommendation or optimized strategy parameters.

The locked semantic regression result contains 1 decision, 6 order intents, 6 orders, 6 fills, 6 position snapshots, and 369,479 equity snapshots. Its semantic result SHA-256 is `e7c2fff2605a4642df7ea1ab4f26ef3ca5896fbc49c63527b28aba7e6a1f6e93`. CI fails if the fixture checksum or the deterministic semantic baseline changes unexpectedly.

Backtest equity persistence batches consecutive equity snapshots without changing event order. Buffered equity is flushed before any later fill/strategy event and at run completion, preserving the canonical event sequence while making full-history regression practical.

Phase 3 remains backend-only. The strategy code is deployed on Render, but Phase 3 did not claim or perform a production-database XAGUSDT run because no safe backend invocation surface exists yet. The master Phase 3.5 UI checkpoint will expose authenticated user-triggered Backtest execution and inspection.

## Phase 3.5 Backtest Strategy UI

The authenticated Backtest page now exposes the backend historical replay engine. The dataset and replay interval selected in the page's top historical-data controls are authoritative for both charting and strategy replay; the lower strategy form does not ask for symbol, dataset, or timeframe again.

Backtest jobs are owned by the Render backend. Creating a run returns a lightweight job ID, and the browser polls job status every 2.5 seconds. The backend reports integer replay progress from 0% through 100%; closing the browser does not stop the running backend task. The current Render plan is intentionally limited to one concurrent heavy backtest.

Stored 1-minute OHLCV can be replayed as UTC 1-minute, 1-hour, or 1-day candles. Aggregation happens in Rust before strategy execution. Strategies can declare that they require a previous completed candle; when no earlier replay candle exists in the selected range, the first available replay candle is reserved as pre-roll and active replay begins on the next candle.

The current UI strategy is the Phase 3 `static-grid-fixture`. It exposes initial capital, start/end dates, previous-close or fixed anchor, spacing, levels per side, order quantity, fees, spread, slippage, latency, touch vs trade-through limit fills, and partial-fill ratio. GTC is fixed because the current simulator does not yet implement IOC/FOK/GTX semantics.

Completed results are reconstructed from canonical run persistence rather than browser state. The UI shows total return, final equity, max drawdown, fees, fill count, final position, realized PnL, effective replay range, pre-roll status, and a complete fill audit. The most recent run ID is kept in browser local storage only as a convenience pointer; the run itself remains in SQLite.

### Phase 3.5 verification

GitHub Actions verifies JavaScript syntax, the Rust suite, backend replay aggregation/pre-roll behavior, and the full frozen XAGUSDT regression. The Phase 3.5 code is deployed and live on Render. The authenticated production XAGUSDT run was launched from the hosted page, reached 100%, persisted as Run #1, and returned coherent fills/KPIs.

## Phase 3.6 Visual Backtest Analysis

Completed Backtest runs now expose a dedicated authenticated analysis endpoint at `/api/backtests/runs/{run_id}/analysis`. The endpoint returns the complete persisted strategy equity series, position-change events, and persisted priced order levels with activation/terminal times. It does not duplicate the historical market dataset in the response; the browser reuses the complete historical dataset already loaded for the Backtest page or fetches that same stored dataset when inspecting a run whose dataset is not currently selected.

The visual layer never changes strategy or execution behavior. Strategy equity is read directly from canonical persisted equity snapshots. Position quantity is reconstructed from persisted position snapshots. Grid/order lines are reconstructed from canonical orders and terminal order-state events.

Every completed run is compared with a constant **Buy & Hold** benchmark of the same underlying over exactly the effective active Backtest period. The benchmark starts with the same initial capital, buys at the first active replay candle open, and holds continuously. Benchmark equity is `initial_capital × candle_close / first_active_open`. It is a raw underlying-return benchmark and does not apply the strategy's fees, spread, slippage, latency, or partial-fill assumptions.

Because strategy equity remains the persisted canonical series, a zero-position/cash-only period can remain flat while Buy & Hold continues moving with the underlying. This is intentional and makes out-of-market periods visible.

The completed-run analysis surface contains:
- full replay OHLC with persisted buy/sell order-level lifetimes and buy/sell fill markers;
- Strategy Equity versus Buy & Hold, both in quote-currency equity;
- Strategy Drawdown versus Buy & Hold Drawdown, in percent;
- signed base-asset position quantity and gross market exposure as percent of strategy equity;
- an explicit strategy/run parameter summary;
- the exact fill-audit table retained below the charts.

Dense 1-minute runs keep all records available and use zoomable ECharts views rather than truncating the run. Pure visualization math for Buy & Hold, drawdown, and position/exposure is isolated in `web/backtest-analysis-math.js` and covered by Node tests.

### Phase 3.6 verification

Automated verification covers the analysis math, JavaScript syntax, Rust storage/read paths, the existing Rust suite, and the full frozen XAGUSDT regression. The analysis implementation is deployed live on Render, and the authenticated production visual output was reviewed and accepted as sufficient for Backtest analysis.

## Phase 4.1 Paper runtime contract

Phase 4.1 establishes the backend-owned Paper run contract without changing any strategy rules.

- `PaperManager` is created by the server and owns in-memory Paper runtime handles keyed by the canonical `run_id`.
- Each run has isolated runtime state. There is no single global strategy/portfolio object shared across runs.
- `PaperRunCore` reuses the existing mode-neutral `Strategy` trait, `PortfolioState`, `SimulatedExecution`, and canonical trading-run persistence rather than creating Paper-specific strategy or accounting implementations.
- Canonical Paper runs use the existing lifecycle/status model: `created`, `running`, and terminal `completed` / `stopped` / `failed`.
- Strategy implementations remain under `src/trading/strategies/`; Paper runtime orchestration is outside those strategy modules.
- Phase 4.1 does not by itself certify the real-time candle clock, recovery policy, control API/live stream, or Trading page. Those remain separate Phase 4 checkpoints.

Focused tests verify canonical Paper lifecycle persistence and isolation between separate Paper run cores.

## Phase 4.2 Real-time candle clock

Paper's strategy clock is backend-owned and deterministic.

- Active Paper replay consumes the existing server-side Binance public market service at a **1-minute base interval**; browser data is never an input to the strategy clock.
- A run created during an interval starts in `arming` state and uses the next clean UTC 1m / 1h / 1d boundary as its first active boundary.
- Before `on_start`, the backend obtains exactly the immediately previous completed replay candle for the configured interval. This gives previous-candle strategies the same information boundary used by Backtest.
- Completed 1-minute candles are aggregated into 1m / 1h / 1d replay candles using the shared `TradingInterval` UTC bucket definitions.
- Snapshot validation prevents duplicate, out-of-order, misaligned, or missing completed 1-minute candles from reaching the strategy clock. Older candles already processed are recognized as rolling-snapshot history and ignored.
- Deterministic tests compare Paper aggregation directly with the Backtest aggregation routine for 1m, 1h, and 1d and verify identical open/close timestamps and OHLCV values for the same completed minute sequence.

Phase 4.2 establishes chronology only. Order/fill processing semantics remain the separate Phase 4.3 checkpoint.

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
