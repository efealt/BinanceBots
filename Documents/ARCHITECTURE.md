# BinanceGrid Architecture

Status: current system  
Last updated: 2026-09-24

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
- `PaperRunCore` reuses the existing mode-neutral `Strategy` trait, `PortfolioState`, and canonical trading-run persistence rather than creating Paper-specific strategy or accounting implementations. The original Phase 4.1 implementation also reused the candle-oriented simulator; Phase 4.8C supersedes that execution coupling with explicit historical and real-time adapters.
- Canonical Paper runs use the existing lifecycle/status model: `created`, `running`, and terminal `completed` / `stopped` / `failed`.
- Strategy implementations remain under `src/trading/strategies/`; Paper runtime orchestration is outside those strategy modules.
- Phase 4.1 does not by itself certify the real-time candle clock, recovery policy, control API/live stream, or Trading page. Those remain separate Phase 4 checkpoints.

Focused tests verify canonical Paper lifecycle persistence and isolation between separate Paper run cores.

## Phase 4.2 Real-time candle clock

Paper's strategy clock is backend-owned and deterministic.

- Active Paper replay consumes the existing server-side Binance public market service at a **1-minute base interval**; browser data is never an input to the strategy clock.
- **Start Live-Paper is immediate.** At click time the backend uses the open of the replay bucket already in progress as a reference boundary and obtains exactly the immediately previous completed 1m / 1h / 1d replay candle. The unfinished current replay candle is never used to initialize the strategy.
- The Run is persisted as `Running` and its initial strategy orders are created synchronously before the Start response is returned; there is no routine wait for the next replay boundary.
- To prevent retroactive fills, the simulator never evaluates a 1-minute OHLC range that began before the orders existed. Post-start market processing begins with the first full UTC 1-minute candle at or after Start. For 1h / 1d replay, the first replay bucket may therefore be a post-start partial bucket; it contains only full 1-minute candles after Start and never pre-start range data.
- Subsequent completed 1-minute candles are aggregated into 1m / 1h / 1d replay candles using the shared `TradingInterval` UTC bucket definitions.
- Snapshot validation prevents duplicate, out-of-order, misaligned, or missing completed 1-minute candles from reaching the strategy clock. Older candles already processed are recognized as rolling-snapshot history and ignored.
- Deterministic tests compare Paper aggregation directly with the Backtest aggregation routine for 1m, 1h, and 1d and verify identical open/close timestamps and OHLCV values for the same completed minute sequence.

Phase 4.2 establishes chronology only. Order/fill processing semantics remain the separate Phase 4.3 checkpoint.

## Phase 4.3 Paper strategy and simulated execution loop

Each completed Paper replay candle now runs through the same execution chronology established by Backtest:

1. resting simulated orders that were eligible before the candle are processed first;
2. fills are persisted and applied to the shared `PortfolioState`;
3. order state and position snapshots are persisted through the canonical run model;
4. the portfolio is marked to the completed candle close;
5. only then does the mode-neutral strategy receive `on_candle`;
6. decisions and new order intents are persisted and submitted after that completed-candle decision;
7. the candle-close equity snapshot is persisted last.

Because simulated execution for a candle is processed before the strategy callback, an order created from that candle's completed information cannot retroactively fill from the same candle's earlier range. It becomes eligible only on a later candle according to the shared simulator and configured latency.

The original Phase 4.3 implementation used the same candle-oriented simulator in Paper and Backtest. Phase 4.8C supersedes that architecture: Backtest keeps historical candle execution while Live-Paper moves to a real-time event adapter. The Run still records the exact execution assumptions: fees, spread, slippage, latency, limit fill policy, and partial-fill ratio.

Focused Phase 4.3 tests verify:
- a resting order fills before the strategy sees the completed candle and the strategy observes the post-fill position;
- an order emitted by that completed candle remains pending even when the just-finished candle crossed its price;
- canonical fill → position → decision → new intent → equity event ordering is append-ordered and auditable;
- configured fee/spread/slippage/latency/partial-fill behavior is applied through the Paper path;
- trade-through limit policy does not fill on a mere touch.

Phase 4.3 does not certify stop/restart/gap-recovery behavior; that remains Phase 4.4.

## Phase 4.4A Stop semantics and backend ownership

Paper Stop is now an idempotent backend operation.

- The in-memory Paper core tracks its canonical lifecycle and refuses any further candle processing once it is terminal.
- A successful user stop expires pending simulated orders once, persists exactly one `stopped` transition with the `user_stop` reason, and marks the runtime core terminal.
- Repeating Stop on the same in-memory run does not append duplicate stop events.
- Repeating Stop after the in-memory runtime handle is gone returns the persisted stopped run instead of attempting a second transition.
- The runtime loop gives a ready stop signal priority over market polling and re-checks the stop signal after asynchronous market/snapshot work before advancing the strategy.
- Browser live-update subscriptions are observers only. Dropping a browser subscription does not signal Stop or remove the backend runtime handle.

Focused tests cover stop idempotence, persisted reason, rejection of post-stop candle processing, repeated Stop without a runtime handle, and browser-subscription disconnect independence.

Phase 4.4A does not define feed-gap recovery or service-restart recovery; those remain 4.4B and 4.4C.

## Phase 4.4B Feed reconnect and gap integrity

Paper now treats Binance feed connectivity as part of the strategy clock's integrity contract.

- While the backend market feed is `loading` or `reconnecting`, the Paper strategy clock is paused; no candles, fills, or strategy decisions are advanced.
- When the feed reports `live` again, the runtime resumes only after the backend snapshot proves contiguous 1-minute chronology starting from the runtime's exact expected candle.
- A later candle appearing while the expected minute is absent proves an unrecoverable real-time gap, even when that later candle is still forming. The run fails rather than silently treating a later historical catch-up as uninterrupted Paper observation.
- Older candles already processed remain harmless rolling-snapshot history and are ignored.
- Successful continuity validation emits a runtime `feed_resumed` event; a disconnect emits `feed_paused`.
- Gap failures use the persisted reason prefix `market_data_gap:`, stored on the canonical failed run for later Paper-vs-Backtest comparison.

Focused tests verify contiguous reconnect catch-up, missing-minute detection when the snapshot has already advanced, and persistence of the exact gap-failure reason.

Phase 4.4B does not define service-restart recovery; that remains 4.4C.

## Phase 4.4C Service restart handling

Paper runtime continuity is deliberately **not** reconstructed across a Render/service process restart in Phase 4.

- On PaperManager startup, canonical Paper runs still in `created` or `running` state are detected from SQLite.
- The current runtime keeps strategy object state, simulated pending-order state, and precise real-time observation continuity in memory. After a process restart, that complete state cannot be proven equivalent to the interrupted runtime.
- Therefore interrupted Paper runs are terminated explicitly as `failed`; the system never pretends the observation remained continuous.
- The terminal reason is persisted as:
  `service_restart_interruption: paper runtime state and realtime chronology cannot be proven`
- Already terminal Paper runs are left unchanged.

Focused tests verify that both `created` and `running` Paper runs become failed with the persisted interruption reason, while an already stopped run receives no new events.

This is the Phase 4 safe-recovery rule: **recover only when continuity can be proven; because the current in-memory runtime state cannot be reconstructed exactly after process loss, restart recovery terminates rather than resumes.**

## Phase 4.5A Trading control endpoints and Live lock

The authenticated backend control surface for Paper trading is registered under the existing protected application router:

- `POST /api/trading/runs` — create/start a Paper run.
- `GET /api/trading/runs` — list persisted/current Paper runs.
- `GET /api/trading/runs/{run_id}` — inspect one Paper run.
- `POST /api/trading/runs/{run_id}/stop` — stop one Paper run.

These routes are merged only into the server's protected router, which is wrapped by the existing `require_auth` middleware. They are not added to the public router.

Phase 4 enforces the Paper/Live boundary server-side. The create/start request must declare a mode; `paper` is accepted and `live` is rejected with HTTP 423 Locked before market-data access or Paper runtime creation. The remaining control endpoints are backed exclusively by `PaperManager`; no Live manager, Live run creation route, or real-order executor is exposed in Phase 4.

Focused tests verify the mode gate, including case/whitespace handling, and prove that a Live start request is rejected before any market/runtime access.

Phase 4.5A does not certify the completeness of the snapshot payload or the WebSocket reconnect contract; those remain 4.5B and 4.5C.

## Phase 4.5B Complete Paper snapshot contract

`GET /api/trading/runs/{run_id}` now exposes one backend-owned snapshot sufficient to rebuild the current Paper monitoring state after a browser refresh.

The snapshot contains:

- canonical run identity/status plus `created_at_ms`, `started_at_ms`, `ended_at_ms`, and whether the runtime is currently active;
- strategy ID/version/parameters, canonical `run_config`, `data_source`, initial capital, and exact recorded execution assumptions;
- market identity and replay interval;
- backend feed status, best bid/ask, mid price, and the latest 1-minute base candle including whether it is closed;
- latest completed replay candle and last processed base-candle timestamp;
- current portfolio state: position quantity, average entry, cash, equity, realized/unrealized PnL, and fees;
- complete current in-memory open simulated orders for an active Paper runtime;
- recent fills and runtime events.

The browser does not contribute any state to this snapshot. During an active run, market fields are refreshed directly from the backend Binance market service on each Paper poll. For persisted terminal runs, canonical config, timestamps, financial state, and recent fills are reconstructed from SQLite; `runtime_active=false` makes clear that no live runtime/feed state is being claimed.

Persisted recent fills are returned in chronological order, matching the active-runtime convention.

Focused tests verify canonical/config/financial reconstruction without browser state and live market-field synchronization from a backend market snapshot.

Phase 4.5B does not define live-stream sequencing or reconnect de-duplication; that remains 4.5C.

## Phase 4.5C Run live stream and reconnect contract

The authenticated per-run stream at `GET /api/trading/runs/{run_id}/stream` now uses an explicit **snapshot first → live updates second** contract.

- Every active Paper snapshot has a monotonic in-memory `stream_revision`.
- The backend subscribes to the run broadcast channel **before** reading the bootstrap snapshot. This closes the previous race where an update could occur between separate snapshot and subscribe calls.
- The WebSocket sends one `snapshot` message first, then `update` messages containing the same complete Paper snapshot contract used by the REST endpoint.
- Updates at or below the last delivered revision are discarded, so a state already represented by the bootstrap snapshot is not replayed as a duplicate.
- Because each update is a complete state snapshot, candles, order changes, fills, portfolio/equity, runtime status, and feed status are reconstructed from one mode-neutral message shape rather than browser-owned incremental state.
- If the broadcast receiver reports lag, the server does not silently skip forward. It fetches a fresh authoritative Paper snapshot and sends it as a new `snapshot` resynchronization message before continuing.
- A terminal/persisted run has no live receiver: its snapshot is sent once and the WebSocket closes.
- The stream remains inside the authenticated protected router, and disconnecting the WebSocket does not affect the backend Paper runtime.

Focused tests verify monotonic snapshot revisions, subscribe-before-snapshot bootstrap behavior, and revision de-duplication.

This completes the Phase 4.5 backend control/snapshot/stream layer; the Trading page itself begins in Phase 4.6.

## Phase 4.6A Shared Trading page shell

The authenticated application now includes `/trading.html`, linked from the main navigation on every existing app page.

The Phase 4.6A shell establishes the permanent shared Paper/Live surface without adding run controls yet:

- a prominent **PAPER | LIVE** mode selector with Paper selected;
- Live is visibly disabled and labeled locked, with the page stating that server-side Live execution remains locked until Phase 8;
- a Paper-run status card showing the selected backend run ID and runtime/canonical status;
- a Binance-feed status card driven from the backend Paper snapshot;
- a backend-connection card showing REST/stream connectivity;
- runtime context for symbol, market type, replay interval, and strategy.

On load, the page queries the protected Trading API and restores an active `arming`/`running` Paper run when one exists. If no active Paper runtime exists, the operational workspace remains empty rather than auto-loading the newest terminal run. Terminal Paper runs remain persisted and queryable through the canonical Trading APIs, but they are historical state rather than the current bot. If an active run exists, the page attaches to the authenticated per-run WebSocket from Phase 4.5C so header/feed status stays current. The browser remains an observer; closing or refreshing the page does not affect the backend runtime.

The page is served by the existing protected static-file fallback, so it inherits the same server-enforced authentication boundary as the rest of the application.

Focused frontend checks verify the Trading navigation entry on all app pages, required status/header elements, JavaScript syntax, and the visibly disabled Live selector.

Phase 4.6A does not add strategy configuration, Start/Stop controls, charts, or portfolio/order audit panels; those remain 4.6B–4.6D.

## Phase 4.6B Trading configuration and run controls

The shared authenticated Trading page now operates the Phase 4 Paper control API.

- The page exposes market/symbol, replay interval, initial capital, current strategy, static-grid parameters, and the exact Paper execution assumptions used by the shared simulator.
- Existing Paper snapshots repopulate the form from backend/canonical state; configuration is not reconstructed from browser-local state.
- **Start Paper** sends the visible configuration to `POST /api/trading/runs`, renders the returned authoritative snapshot, then attaches to the run stream.
- **Stop Paper** calls `POST /api/trading/runs/{run_id}/stop`, then renders the returned persisted terminal snapshot.
- While a Paper runtime is active, strategy/configuration controls and the Paper mode selector are locked. The Stop control remains available.
- Live remains disabled independently of this browser locking; the Phase 4 server-side Live rejection from 4.5A remains the safety boundary.
- When a Paper run terminates or the user stops it, the operational workspace clears run-specific portfolio/orders/audit/chart overlays and the form becomes editable again while retaining the visible just-used configuration. The terminal run remains persisted but is not presented as the current bot.

The controls deliberately do not add chart/order/fill visualization; those are Phase 4.6C and 4.6D.

## Phase 4.6C Live chart and execution overlays

The Trading page uses the same Lightweight Charts 5.2 candlestick engine and reusable MarketChart wrapper as the Market page, backed only by server-owned state.

- `GET /api/trading/runs/{run_id}/chart` is authenticated with the rest of the Trading API.
- For an active Paper run, the chart bootstrap returns the backend Binance 1-minute market-feed window, currently bounded by the market service's 1,000-candle runtime capacity.
- Order overlays come from canonical persisted orders plus terminal order-state events through `trading_run_order_levels`, so each horizontal buy/sell level carries its actual active-from and active-to timestamps.
- Fill markers come from the complete canonical fill audit through `trading_run_fill_audit`.
- WebSocket snapshots continue updating the current 1-minute candle in place. New orders/fills trigger a chart-overlay refresh from canonical storage, so the browser does not invent execution history.
- The chart explicitly labels its base feed as Binance 1m even when the strategy replay interval is 1h or 1d; strategy timing semantics remain backend-owned and unchanged.
- The live candle window is runtime market context, not a replacement for the complete persisted run audit or Phase 5 historical replay. An inactive persisted run therefore does not claim retained live candles after its backend runtime is gone.

Phase 4.6C does not add portfolio, open-order table, exposure, or full event-log panels; those remain Phase 4.6D.

- The Trading chart's market candles are independent of Paper runtime state. After registered instrument discovery, the page connects to the existing authenticated `/api/market/stream` service, consumes its initial 1-minute snapshot, and keeps that selected market streaming before any Paper run exists.
- Symbol/Market changes while no run is active reconnect the chart immediately. Starting or restoring Paper keeps the same live market chart and layers canonical run order lifetimes and fills on top; stopping clears only run overlays and leaves the selected live market visible.
- This reuses the same backend `MarketService` as the Market page; no second market-data backend or browser-owned trading feed was introduced.
- The Trading chart is an operational execution view built on native Lightweight Charts interactions. The compact last-price/OHLC/UTC strip is driven by the series/crosshair; the library owns crosshair, price scale, current-price line/label, zoom, drag pan, touch interaction, and visible-range behavior. Trading adds canonical order lifetimes as native LineSeries and fills as native series markers, without rebuilding the chart on each market tick.

## Phase 4.6D Portfolio, open orders, and canonical audit

The Trading page now exposes the complete Paper operational state required for monitoring and exact run auditability.

- Portfolio cards render backend snapshot values for current position/inventory, cash, equity, realized PnL, unrealized PnL, fees paid, mark price, and gross exposure as `abs(position × mark) / equity`.
- Active Paper snapshots expose open orders with side, price, original quantity, filled quantity, remaining quantity, partial-fill state, and backend submission age. When no active runtime exists, the operational orders panel is empty; terminal persisted runs are not auto-loaded into it.
- `PaperSnapshot.mark_price` is synchronized from the backend Binance mid/last candle while active and reconstructed from the latest persisted position snapshot after the runtime ends, so terminal exposure is not estimated from browser-local data.
- `GET /api/trading/runs/{run_id}/audit` exposes the canonical append-ordered run event stream. The default response returns the latest 250 events for operational viewing.
- Audit pagination uses canonical `run_sequence` through `after_sequence` with a maximum page size of 500. The page's **Load complete audit** action walks every page from sequence zero until the server reports no later events, so long runs remain fully accessible rather than silently truncated.
- Audit rows preserve event time plus event-specific fields such as order side/type, price, quantity, filled quantity, fee, position quantity, equity, status, and persisted run notes where applicable.
- While a run remains active, the page refreshes the canonical tail periodically; when the complete audit is loaded, later canonical events append after the last known sequence.

The audit API reads SQLite canonical records directly. The browser's bounded recent-event buffer remains a live convenience only and is not treated as the historical source of truth.

## Phase 4.7 Shared Paper/Live UI contract

The Trading page now has an explicit mode-neutral frontend contract instead of scattering Paper-specific field names and routes through every renderer.

- `web/trading-contract.js` owns the shared monitoring model for run status, market state, strategy identity, portfolio/equity/PnL/fees/exposure, open orders, recent fills, and recent runtime events.
- Raw backend snapshots are normalized into that monitoring model before the shared status, chart, portfolio, orders, and audit surfaces render.
- Paper-specific control and transport behavior is isolated behind a Paper adapter: list/snapshot/chart/audit/stream/start/stop routes plus the Paper start payload builder.
- A separate Live adapter already exists as a locked contract with `executionKind = binance_private`. It exposes no usable execution endpoints in Phase 4 and throws if asked to build a Live start payload, preserving the server-side Phase 4 Live lock as the real safety boundary.
- The current Paper strategy/configuration form is explicitly a `data-mode-panel="paper"` setup panel. A separate hidden Live safety/setup panel occupies the same slot for future Phase 7/8 account/risk controls, so the core monitoring page is not duplicated.
- Mode styling is driven by `data-trading-mode`. Paper uses the normal application accent; future Live mode uses the sell/risk color family across the mode selector and shared monitoring surfaces so real execution is visually unmistakable.
- Dedicated frontend tests compare the Paper and Live normalized model shapes and verify that Live remains locked while Paper continues to use the existing authenticated Trading API.

This refactor does not add private Binance account access, user-data streams, signing, or order submission. Those remain later-phase responsibilities.

## Phase 4.8A Automated parity and safety verification

Phase 4 now has a dedicated automated verification suite that exercises the chronology and safety contract independently of the production UI.

- A synthetic completed-candle sequence is fed through both the historical Backtest engine and the Paper core with the same static-grid strategy, execution assumptions, pre-roll candle, and active candles. The test compares decisions, order intents, created orders, order-state transitions, fills, position snapshots, equity/accounting snapshots, non-status event ordering, and final portfolio values.
- Mid-interval starts are checked against the shared UTC interval boundary calculation, and bootstrap verification accepts only the immediately previous completed replay candle.
- Backend snapshot integrity checks explicitly cover duplicate, out-of-order, stale-history, and missing-candle/gap cases.
- Stop verification confirms a second stop produces no additional canonical event, while reconnect verification enforces snapshot-first bootstrap and monotonically newer stream revisions without making the browser runtime owner.
- The Phase 4 API Live gate is tested with an otherwise-invalid Live request, proving the request is rejected as Live-locked before market/configuration validation or Paper runtime creation. No Paper or Live run is created.
- CI runs the focused Phase 4 verification tests, the complete Rust suite, and then the pre-existing frozen XAGUSDT Backtest/grid regression unchanged.

These tests add verification only; they do not change strategy rules, Paper execution semantics, or enable any Binance private-account behavior.

## Phase 4.8B Phase 1 — Persistent Bot identity and saved configuration

Trading now has a persistent Bot parent layer above execution Runs.

- A **Bot** is durable identity plus saved configuration: stable `bot_id`, editable `bot_name`, validated configuration JSON, and created/updated timestamps.
- Saved configuration contains the complete current Trading setup required to restore the form: Symbol/Market parameters, replay interval, initial capital, strategy ID/grid parameters, and execution assumptions.
- Protected persistence endpoints are `POST /api/trading/bots`, `GET /api/trading/bots`, `GET /api/trading/bots/{bot_id}`, and `PUT /api/trading/bots/{bot_id}`.
- Migration 005 adds `trading_bots` and `trading_runs.bot_id`. Migration 006 resets Trading operational tables while leaving Data Downloader definitions and downloaded historical market datasets untouched.
- Newly created **Live-Paper** runs must reference a real persisted Bot. The storage layer enforces this invariant even when called outside the HTTP API. The same invariant is reserved for future **Live-Real-Account** runs; Backtest is the only run mode that may exist without a Bot.
- A Run remains an immutable execution snapshot. Editing the parent Bot does not rewrite prior run strategy parameters, run configuration, data source, initial capital, or execution assumptions.
- Paper runtime snapshots and run summaries expose the linked `bot_id` when present so the next workspace phase can group operational state by Bot.
- New Live-Paper starts require an explicit persisted `bot_id`. Phase 2 adds the Bot strip and explicit New Bot / Save Bot UI that supplies this ID.
- **Live-Paper** means real-time Binance market data with simulated orders/fills. **Live-Real-Account** means real Binance account order submission and remains locked.

## Phase 4.8B Phase 2 — Bot strip and selected workspace

Trading is now Bot-first rather than run-first.

- On page load the client reads saved Bots from `GET /api/trading/bots` and Live-Paper run summaries from `GET /api/trading/runs`.
- The top Bot strip sorts Bots with an active Live-Paper run first. Each card shows Bot name, Bot ID, `Idle` or `Running · Live-Paper`, and active Run ID when present.
- Selecting a saved Bot restores the Bot's persisted Symbol/Market, replay interval, capital, strategy/grid parameters, and execution assumptions.
- If that Bot owns an active Live-Paper Run, the client loads its current canonical snapshot and attaches to that Run's backend WebSocket stream. If the Bot is idle, execution panels clear while the selected market's real-time Binance chart remains active.
- **New Bot** creates only an unsaved browser draft; it does not stop or mutate any backend-owned run.
- **Save Bot** uses the protected Bot create/update endpoints and does not create a Run.
- A saved Bot's form fingerprint is tracked in the browser. Any edit is shown as **Unsaved changes**; Live-Paper Start remains disabled until the Bot is saved and the submitted configuration therefore matches the persisted Bot record.
- Starting Live-Paper always sends the selected persisted `bot_id`.
- Reloading Trading reconstructs the Bot strip from backend state and selects a running Bot first when one exists, so browser lifetime is not runtime ownership.
- Phase 2 does not change runtime concurrency. The existing single-active-Live-Paper limit remains until Phase 3.

## Phase 4.8B Phase 3 — Bot-aware concurrent Live-Paper lifecycle

Forward execution is now keyed by **Bot → Run**, not by one global current runtime.

- A Bot may have at most one active Live-Paper Run in `arming` or `running` state. Backend admission enforces this under the runtime registry lock so simultaneous Start requests cannot create duplicate active Runs for one Bot.
- Different Bots may run Live-Paper concurrently, up to the production runtime capacity. Each runtime remains keyed by its immutable `run_id`.
- Runtime snapshots, WebSocket updates, portfolio state, open orders, fill/audit data, and chart overlays are always loaded by the selected Bot's active `run_id`. Switching Bot cards closes only the browser subscription to the prior Run; it never signals that backend runtime to stop.
- Stop targets only the selected Run. The Bot remains persisted and returns to `Idle`; a later Start creates a new Run ID under the same Bot.
- New unsaved configurations use an explicit **Create & Start Live-Paper** flow: Bot persistence succeeds first, then execution starts with that `bot_id`.
- Edited saved Bots use **Save & Start Live-Paper**: the update is persisted before the new Run is created, preventing the UI from executing an unsaved variant under a saved Bot label.
- Browser close/reload is not runtime ownership. Reload reconstructs all running Bot cards from backend run summaries and reconnects only the currently selected Run stream.
- Concurrency tests exercise two simultaneously running Bot cores and prove orders, fills, portfolios, run events, and canonical audit pages remain separated by `run_id`.
- **Live-Real-Account** execution remains server-side locked.

## Phase 4.8B Phase 4 — Side-effect-free pre-trade preview

Trading now supports a backend-derived **Show on graph** preview before Live-Paper execution.

- `POST /api/trading/preview` accepts the visible Live-Paper configuration without requiring a saved Bot. It validates the same configuration contract used by Start and calls the same static-grid strategy initialization semantics entirely in memory.
- Preview is deliberately non-canonical and side-effect free: it does not create or update Bots, Runs, order intents, orders, fills, positions, equity snapshots, or audit events.
- Previous-close preview uses the latest fully-known replay boundary so it never relies on an unfinished candle. Live-Paper Start now uses the same current reference-boundary rule and starts immediately from the latest completed replay candle. Preview and Start therefore match within the same replay bucket; they can differ only if the market crosses into a new replay bucket between the two actions.
- The existing TradingView Lightweight Charts surface draws dotted `PREVIEW BUY/SELL` price lines and an explicit **Preview · not active** badge. Preview state never masquerades as canonical execution state.
- Strategy-defining edits (symbol, market, replay interval, strategy, anchor, fixed anchor, spacing, levels, quantity) mark the preview stale. Stale levels use warning styling and the action becomes **Update preview**.
- Selecting another Bot or opening a New Bot clears the prior preview. When a Live-Paper Run becomes active, preview overlays are removed and replaced by the Run's canonical order/fill overlays.
- A Rust parity test proves that preview levels and persisted Live-Paper initial orders match exactly for the same strategy configuration, previous candle, and market boundary.
- **Live-Real-Account** remains server-side locked.


## Phase 4.8C Phase 1 — Explicit execution adapters

The shared strategy contract remains mode-neutral. Strategies still emit the same `StrategyOrderIntent` objects regardless of whether they are used in Backtest or Live-Paper.

Execution interpretation is now explicit:

- `HistoricalExecution` owns the existing deterministic historical-candle semantics. Its resting-order OHLC Touch / Trade Through behavior is unchanged, preserving Backtest outputs and the frozen XAGUSDT regression.
- `LivePaperExecution` is a distinct real-time execution adapter. It owns resting simulated orders and accepts `LiveExecutionEvent` inputs; the first supported event is a Binance-style trade carrying trade ID, exchange event time, price, and quantity.
- Both adapters emit the same shared `ExecutionFill` contract, so portfolio accounting and canonical persistence do not need mode-specific fill representations.
- Pending-order validation, latency-based eligibility timestamps, partial-fill quantity calculation, fee calculation, and cumulative fill accounting remain centralized in the execution module instead of being duplicated.
- No strategy wrapper or Live-Paper copy of `StaticGridStrategy` exists. A focused test submits one static-grid `on_start` output unchanged into both adapters and proves the resting intents are identical.
- Phase 1 introduced the adapter split. Phase 3 now wires the running Live-Paper core to `LivePaperExecution`; Backtest remains on `HistoricalExecution`.
## Phase 4.8C Phase 2 — Backend realtime execution feed

The Market service now exposes a backend-only bounded event path for execution chronology. It is separate from the browser's market-stream UI polling.

- Each realtime event carries a monotonic backend sequence, a connection epoch, backend receive time, and either feed status, Binance trade, or candle data.
- Trade events preserve Binance trade ID, exchange trade time, price, and quantity. Candle events remain separate so later phases can route trades to execution immediately while completed candles drive strategy timing.
- Each running Live-Paper Run owns a `RunMarketSubscription`. Its per-Run cursor tracks the last backend sequence and last Binance trade ID. Exact duplicate trade IDs are suppressed; backwards trade IDs are treated as continuity failures.
- Broadcast lag, sequence gaps, channel closure, and connection-epoch changes are explicit errors. A feed reconnect currently fails the development Run and persists a stable reason because missed trade chronology cannot yet be reconstructed safely.
- The subscription is attached to backend Live-Paper runtimes and is independent of browser presence. Phase 3 now consumes its trade events for resting-order fills; candle events remain separate from execution.
- The bounded channel capacity is 16,384 execution events. A slow consumer fails explicitly rather than silently dropping execution information.

### Development trading-state reset

Migration 007 intentionally clears disposable Bot/Run operational state before the realtime execution path proceeds. It deletes `trading_runs` and `trading_bots` and relies on foreign-key cascades for their canonical child records. Download definitions, historical datasets/OHLC/trades, market instruments, and live-capture tables are preserved. No compatibility bridge is kept for the discarded development trading records.


## Phase 4.8C Phase 3 — Live-Paper resting-order execution

Live-Paper resting orders now execute from Binance trade events rather than completed candle OHLC ranges.

- `PaperRunCore` owns `LivePaperExecution`; it no longer owns or calls the historical candle executor.
- Each qualifying `MarketRealtimeEventKind::Trade` is converted to a `LiveTradeEvent` with Binance trade ID, exchange trade time, price, and quantity and is processed immediately in the backend Run loop.
- Touch fills use BUY `trade <= limit` and SELL `trade >= limit`; Trade Through uses strict BUY `trade < limit` and SELL `trade > limit`. Simulated limit fills remain priced at the resting limit.
- `latency_ms` gates eligibility by exchange timestamp only. `partial_fill_ratio` and configured fees remain shared execution assumptions.
- Canonical Fill rows persist the qualifying Binance trade ID in `exchange_trade_id`; event timing preserves exchange time and backend receive time. Order-state and position effects follow the same shared accounting path as before.
- Completed strategy candles call `strategy.on_candle` and persist strategy/equity effects only; they cannot retroactively fill resting Live-Paper orders.
- Duplicate trade IDs are suppressed per Run and defensively by `LivePaperExecution`; backwards trade IDs and feed continuity failures fail explicitly rather than guessing.
- A stopped or failed Run expires pending simulated orders and rejects later trade processing.

Backtest execution is unchanged and continues to use `HistoricalExecution`.


## Phase 4.8B Phase 5A — Bot history and Run activity backend contracts

The lower Trading workspace now has dedicated read-only backend contracts for history and presentation without changing execution semantics.

- `GET /api/trading/bots/{bot_id}/runs` returns only Runs owned by that Bot, newest first. Each entry includes Run/mode identity, canonical and runtime status, timestamps, market/strategy context, fill count, latest position, fees paid, realized PnL, and latest equity when persisted.
- Runtime status is overlaid from the in-memory Live-Paper runtime registry when a Run is currently `arming` or `running`; historical Runs fall back to their canonical persisted status.
- `GET /api/trading/runs/{run_id}/activity` is a presentation feed derived from canonical persisted events. It is newest-first and excludes repetitive `equity` heartbeat events from the default activity list. The response reports both canonical and activity totals plus the number of suppressed Equity rows, and supports older-page retrieval with `before_sequence`.
- The canonical `GET /api/trading/runs/{run_id}/audit` contract is unchanged. No canonical events are deleted, reordered in storage, rewritten, or summarized away.
- Phase 5A is read-only with respect to execution: no changes were made to strategy logic, order/fill chronology, portfolio accounting, Bot state, Run lifecycle, or Live-Paper concurrency.
- Tests prove Bot-history isolation and ordering, per-Run summary ownership, newest-first activity filtering, Equity-noise suppression, and canonical audit non-mutation.


## Phase 4.8B Phase 5B — Lower Trading workspace

The accepted upper Trading workflow remains unchanged: Bot strip, Live-Paper configuration/controls, and Shared market monitor continue to own Bot selection, execution control, and the live chart.

The lower workspace is now presentation-oriented while staying faithful to canonical backend state:

- **Portfolio monitor** is compact: signed inventory and equity are primary, with realized/unrealized PnL, fees, exposure, cash, and mark price grouped in smaller metrics.
- **Open orders** are split into BUY-left / SELL-right execution columns on desktop and stack vertically on narrow screens. Every backend order remains distinct. Untouched orders use the operationally simpler **Qty · Open** display; partially filled orders expose **Filled / original · Remaining** only when the distinction becomes useful, alongside Order ID, price, state, type, and age.
- **Run activity** is the default execution record view. It consumes the Phase 5A newest-first meaningful-activity endpoint, so fills/orders/positions/lifecycle events are immediately visible while repetitive Equity heartbeat events are counted but suppressed from the default list.
- The complete canonical record remains available through a separate on-demand **Technical audit** modal, including a complete-audit loader. The raw diagnostic record is not part of the daily Trading layout; opening or closing the modal has no execution side effects.
- **Bot history** shows Runs newest first with active/current vs terminal status and compact persisted result summaries. Historical selection is read-only. While inspecting history, the compact **Exit history** utility returns to the selected Bot's current state; visible info affordances explain this and the other Run Activity utilities.
- Historical inspection uses separate browser detail state from the active runtime state. If the Bot is currently running, its WebSocket and backend ownership continue unaffected while the lower panels and chart overlays inspect an older Run. Returning to the current Bot restores the active Run details.
- The market chart remains live during historical inspection and follows the inspected Run's symbol/market when that differs from the Bot's current saved configuration; the saved configuration itself is never mutated by inspection.
- Bots and Portfolio panels override the generic panel minimum height so they fit their actual content instead of leaving large empty desktop gaps. The redundant Runtime context panel was removed because its symbol/market/interval/strategy values duplicate the accepted Bot/config/chart surfaces.
- Lower panels use responsive `minmax(0,...)` grids and stack on narrow screens; the redesign introduces no page-level horizontal scrolling.
- Console remains untouched in Phase 5B; its lightweight real-Bot monitoring conversion is Phase 6.


## UI

- **Console** — current bot-console UI shell.
- **Market** — live market chart, quote, depth, trades, and indicators.
- **Data Downloader** — historical dataset creation, catalog, date editing, and missing-data import.
- **Backtest** — stored OHLCV charting and research diagnostics.
- **Trading** — one shared authenticated Paper/Live monitoring workspace with mode adapters, Paper-specific setup, future Live safety/setup isolation, live market/order/fill visualization, portfolio/exposure state, open orders, and complete canonical audit access; Live remains locked.
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
