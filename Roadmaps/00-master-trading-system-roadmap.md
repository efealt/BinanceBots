# Master Trading System Roadmap

## Objective

Build one backend trading/research system in which **Backtest, Paper, and Live use the same strategy interface and the same canonical run/order/fill/result data model**. Development progresses in that order: prove Backtest first, validate Paper against later historical replay second, and only then enable real-money Live trading.

Detailed implementation roadmaps are created separately for each phase when that phase begins. Strategy research is not the focus here; one simple grid strategy is used only as a test vehicle for the shared backend.

## Rules

- Complete phases strictly in order.
- Database/schema changes required by a phase are implemented before that phase writes runtime results.
- Backtest, Paper, and Live must use one strategy interface rather than separate strategy implementations.
- Every run has a unique `run_id`, a mode (`backtest`, `paper`, or `live`), and enough metadata to reproduce it.
- Equivalent runs must be linkable through a comparison/experiment identifier so the same strategy configuration can be aligned across modes.
- The three modes persist the same core event/result concepts so their outputs can be compared directly.
- Paper simulation may use only information available at that real-time moment. Backtest replay may use only information available at each historical decision time.
- A historical replay happens only **after the real-time Paper or Live period has ended and the corresponding Binance historical data has become available and been downloaded/verified**.
- Raw observed Paper/Live results remain immutable. Later calibrated execution assumptions are stored separately from the observations used to derive them.
- Real Binance order submission remains disabled until the final phase.
- Before implementing each phase, create a separate detailed roadmap for that phase.

## Shared run contract

Phase 1 has implemented the canonical persistence contract. All later phases must consume that shared model rather than create parallel Backtest/Paper/Live result schemas:

```text
Run
Decision
OrderIntent
Order
Fill
Position / Inventory
PnL / Equity Snapshot
```

Each run records at least its mode, strategy/version, parameters, symbol/market, capital/configuration, timing, data source, and execution assumptions.

## Sequential phases

### Phase 1 — Unified run model and database foundation — COMPLETE

Implemented the canonical Backtest/Paper/Live persistence contract, exact trading-value representation, append-ordered event history, comparison linkage, and shared Rust storage APIs.

**Exit achieved:** The database can represent and reconstruct equivalent Backtest, Paper, and Live runs before any trading engine writes them.

### Phase 2 — Backtest engine — COMPLETE

Implemented the deterministic historical clock, mode-neutral strategy interface, pre-first-candle `on_start` hook, portfolio/order state, reusable simulated-execution component, bounded historical dataset reads, canonical run persistence, and failure lifecycle on top of Phase 1.

The historical clock is authoritative. Resting orders that existed before a candle is processed may fill from that candle's OHLC range. Normal candle-close strategy logic sees the candle only after it is complete, and any new order created from that completed candle cannot retroactively fill from the candle's earlier open/high/low. A pre-first-candle `on_start` hook allows initial resting orders, such as a grid, to be placed before the first active candle is processed using only information already available at that boundary. Same-timestamp ordering remains deterministic and explicit.

Execution assumptions are run configuration, not hidden strategy behavior. The simulator is designed to support fees, spread/slippage, latency, limit-order fill rules, partial fills, and conservative scenarios without changing strategy logic. Paper will reuse this same simulated-execution component later.

**Exit achieved:** A deterministic backend Backtest run consumes stored Binance data, drives a strategy through the shared interface, reconstructs portfolio/order state, and persists a complete run through the Phase 1 canonical model. Automated verification covers resting-order intrabar fills, prevention of retroactive same-candle fills, deterministic replay, fees/slippage, limit policies, latency, partial fills, accounting, and failed-run preservation.

### Phase 3 — Minimal grid strategy test fixture — COMPLETE

Implemented a deliberately simple mode-neutral static grid through the Phase 2 strategy interface, plus a frozen full real XAGUSDT 1-minute SQLite regression fixture committed to GitHub.

The fixture contains 369,480 Binance public-data candles from 2026-01-07 10:00 UTC through 2026-09-20 23:59 UTC. Ordinary CI does not re-download it. GitHub Actions copies the committed fixture to a temporary writable database and runs the actual Backtest engine twice over 369,479 active candles after one pre-roll candle. Controlled tests cover resting-order same-candle fills, multiple resting levels, and prevention of retroactive fills for orders created after a candle closes.

The locked real-data regression produces 1 decision, 6 order intents, 6 orders, 6 fills, 6 position snapshots, and 369,479 equity snapshots with semantic checksum `e7c2fff2605a4642df7ea1ab4f26ef3ca5896fbc49c63527b28aba7e6a1f6e93`. The fixture and semantic baseline are checksum-guarded in CI.

Phase 3 did not expose a frontend or Paper runtime. The same strategy code is deployed on Render, but no production-database XAGUSDT run is claimed because Phase 3 intentionally has no authenticated run-trigger surface yet.

**Exit achieved:** The simple grid runs end to end through the shared Backtest engine and canonical storage model on the full committed real XAGUSDT regression dataset, with deterministic repeatability and no mode-specific strategy implementation.

### Phase 3.5 — Backtest Strategy UI checkpoint — COMPLETE

Implemented the authenticated Backtest job API, backend UTC 1m/1h/1d replay aggregation, strategy/execution configuration UI, 0–100% progress reporting with 2.5-second polling, persisted-run result reconstruction, KPI output, and complete fill audit.

The top historical dataset and interval selectors are the only market/timeframe selectors. Backtests execute on Render in backend-owned blocking tasks and continue independently of the browser. The current small Render instance accepts only one heavy Backtest job at a time.

Automated verification is green and the implementation is deployed live. The authenticated production XAGUSDT run was launched from the hosted page, reached 100%, persisted as Run #1, and returned coherent fills/KPIs.

**Exit achieved:** The user can independently launch and inspect a historical strategy run through the hosted application while the backend remains the sole owner of strategy execution and persistence.

### Phase 3.6 — Visual Backtest Analysis — COMPLETE

Implemented complete run visualization before Paper trading: underlying replay price with persisted order/grid lifetimes and fill markers, strategy equity against a constant Buy & Hold benchmark of the same underlying, drawdown comparison, signed position/exposure, and explicit strategy/run parameter summary.

Buy & Hold always starts with the same initial capital at the first active replay candle open and holds the underlying continuously through the effective Backtest period. Strategy equity remains the persisted canonical series, so flat/no-exposure periods remain flat while Buy & Hold continues to move.

Automated verification is green, the code is deployed live, and the authenticated production visual output was reviewed and accepted as sufficient for Backtest analysis.

**Exit achieved:** A completed Backtest run can be visually audited over its full effective period without changing strategy logic.

### Phase 3.7 — Strategy Module Structure — COMPLETE

Detailed roadmap: `Roadmaps/03.7-strategy-module-structure.md`

Before Paper runtime work begins, reorganize strategy implementations into a dedicated `src/trading/strategies/` namespace with one strategy per module. Move the existing static-grid fixture out of the generic `grid.rs` file and reserve separate modules for dynamic grid, volatility grid, mean reversion, and breakout without inventing their rules yet.

The shared `Strategy` interface, portfolio/accounting, execution engine, Backtest/Paper/Live contracts, and current static-grid behavior remain unchanged.

**Exit achieved:** Strategy implementations are structurally independent modules, the old shared `grid.rs` was removed, and the existing frozen XAGUSDT regression remains unchanged.

### Phase 4 — Live Paper runtime + shared Trading page — IN PROGRESS (4.1–4.4 + 4.5A–4.5B COMPLETE)

Detailed roadmap: `Roadmaps/04-live-paper-runtime-shared-trading-page.md`

Add a backend-owned Paper mode that consumes real-time Binance market data but sends **no Binance account orders**.

Paper uses the same strategy interface, canonical run model, portfolio/order state model, and simulated-execution component already proven in Backtest. The main change is the clock/data source: historical replay becomes real-time market input.

Phase 4 also creates one new authenticated **Trading** page that is designed from the beginning for both Paper and future Live execution. The page is shared rather than duplicated because Paper and Live need the same operational view: live price/candlestick data, active grid/order levels on the chart, fills, open orders, current position/inventory, cash/equity/PnL, exposure, fees, strategy parameters, run status/run ID, connection/feed status, event/order/fill logs, and start/stop controls.

The Trading page has a prominent **Paper / Live** mode selector. In Phase 4, Paper is fully functional and Live is visible but locked/disabled. A running mode cannot be switched by a casual toggle; the active run must be stopped before any future mode change. The browser remains a client only: Paper execution and state continue on Render if the page is closed.

This same Trading page is retained for later phases. Phase 7 adds authenticated live-account observations and execution-readiness infrastructure behind the existing page while strategy-driven real orders remain disabled. Phase 8 unlocks the Live mode and routes the same strategy/order-intent flow through the real Binance execution adapter. No separate Live-trading page is planned.

Paper must run independently of the browser and persist the same decisions, order intents, simulated orders/fills, positions, and equity/PnL records required for later replay comparison.

**Exit:** The same strategy code can run for a real-time period in Paper mode, survive normal browser disconnects, produce a complete persistent run without real-money execution, and be operated/observed through the shared Trading page with Live visibly locked.

### Phase 5 — Paper-period historical replay

After a Paper observation period has ended, wait until the corresponding Binance historical data is available, then download and verify that completed period.

Replay the **same strategy version, parameters, market, and relevant configuration** over the same time interval using Backtest.

Example:

```text
Day 1:
  Run Paper in real time

After Day 1 history becomes available:
  Download and verify Day 1 Binance data
  Replay the same configuration as Backtest
```

**Exit:** The system contains a temporally aligned Paper run and Backtest replay covering the same completed market interval.

### Phase 6 — Paper vs Backtest validation

Build comparison tooling for the aligned Paper and Backtest runs before any live trading is attempted.

Compare decision timing, intended orders, simulated fills, event ordering, position path, PnL, drawdown, fees, and any divergence between the real-time Paper path and the later historical replay.

The purpose here is to validate chronology, reproducibility, persistence, and consistency between real-time and historical execution of the same system. Paper is still simulated execution, so this phase does **not** claim to measure real exchange fill quality. Raw runs remain unchanged.

**Exit:** We can explain and quantify any Backtest-vs-Paper divergence before introducing real-money execution.

### Phase 7 — Live-account integration and execution readiness

Add authenticated Binance account connectivity and the real execution infrastructure, but keep **strategy-driven live order submission disabled** throughout this phase.

Establish account/balance/position reads, open-order state, private order/fill updates, fees, exchange filters and rounding, idempotent client order IDs, reconnect/reconciliation, stale-data protection, risk limits, emergency-stop behavior, and persistent execution audit records.

Phase 7 extends the **same Trading page built in Phase 4** with live-account observability and readiness state. The Live mode remains locked for strategy-driven execution, but the page can show real account balances, positions, open orders, connection/reconciliation status, exchange constraints, and safety state where appropriate. No parallel Live UI is created.

The real executor must consume the same order-intent contract used by Backtest/Paper and write actual exchange acknowledgements/fills into the same Phase 1 canonical run model. Readiness is proven without permitting automated strategy orders.

**Exit:** The system can observe/reconcile the live Binance account through the shared Trading page and has a verified execution/safety path, while automated real-money submission remains disabled.

### Phase 8 — Live trading and final three-way validation

Only after Backtest/Paper validation is complete and Phase 7 execution/safety readiness is proven do we enable strategy-driven Live orders. This is the first phase in which the strategy may submit real-money orders.

Phase 8 unlocks the **Live** mode on the shared Trading page created in Phase 4. Paper and Live use the same visual/operational surface; the execution adapter underneath changes from simulated execution to the real Binance executor. Live status must be unmistakable and real-money mode cannot be entered accidentally from an active Paper run.

Run the same strategy version/configuration in Live and, where useful, Paper over the same real-time period. Persist actual Binance acknowledgements, fills, fees, timestamps, positions, and PnL through the canonical run model.

After that Live period has ended and the corresponding Binance historical data becomes available:

1. download and verify the same historical interval;
2. replay the same strategy/configuration as Backtest;
3. align Backtest, Paper, and Live runs through their comparison identifier;
4. compare decisions, intended orders, fill prices/timing, slippage, fees, partial/missed fills, position paths, PnL, and drawdown;
5. use measured Live execution differences to calibrate realistic and conservative Backtest/Paper execution assumptions without altering any raw run.

**Exit:** The system has proven one strategy implementation through Backtest → Paper → Live and can quantify how realistically the simulated modes represent actual exchange execution.

## Intended flow

```text
Phase 1   Shared DB / canonical run model
   ↓
Phase 2   Backtest engine
   ↓
Phase 3   Simple grid test fixture + full committed XAGUSDT regression DB
   ↓
Phase 3.5 Backtest Strategy UI / user validation checkpoint
   ↓
Phase 3.6 Visual Backtest Analysis + Buy & Hold benchmark
   ↓
Phase 3.7 Strategy module structure
   ↓
Phase 4   Paper in real time + shared Paper/Live Trading page (Live locked)
   ↓
Phase 5   Later historical replay of that Paper period
   ↓
Phase 6   Paper ↔ Backtest validation
   ↓
Phase 7   Live account + execution/safety infrastructure
          REAL ORDERS STILL OFF
   ↓
Phase 8   Live trading
          ↓
          Later replay of the same completed interval
          ↓
          Backtest ↔ Paper ↔ Live comparison
```

The result is one strategy/runtime architecture with staged evidence: **Backtest first, Paper validation second, Live trading last**.
