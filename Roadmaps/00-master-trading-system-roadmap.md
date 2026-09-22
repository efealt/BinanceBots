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

### Phase 2 — Backtest engine

Build the historical clock, shared strategy interface, portfolio/order state, and reusable simulated-execution component on top of downloaded data and the Phase 1 canonical run model.

The historical clock is authoritative: strategy decisions, order creation, fills, and state transitions occur only from information available at that simulated time. Same-timestamp ordering must be deterministic and explicit.

Execution assumptions are run configuration, not hidden strategy behavior. The simulator is designed to support fees, spread/slippage, latency, limit-order fill rules, partial fills, and conservative scenarios without changing strategy logic. Paper will reuse this same simulated-execution component later.

**Exit:** A deterministic backend Backtest run can consume stored Binance data, drive a strategy through the shared interface, reconstruct portfolio/order state, and persist a complete run through the Phase 1 canonical model.

### Phase 3 — Minimal grid strategy test fixture

Implement one deliberately simple grid strategy through the Phase 2 shared strategy interface.

Its purpose is only to prove signals, order intents, simulated fills, state transitions, persistence, reproducibility, and the Backtest engine end to end. It must not contain Backtest-specific shortcuts that prevent the same strategy code from being used later in Paper and Live.

**Exit:** The simple grid runs end to end in Backtest using the shared engine and canonical storage model, with no mode-specific strategy implementation.

### Phase 4 — Live Paper runtime

Add a backend-owned Paper mode that consumes real-time Binance market data but sends **no Binance account orders**.

Paper uses the same strategy interface, canonical run model, portfolio/order state model, and simulated-execution component already proven in Backtest. The main change is the clock/data source: historical replay becomes real-time market input.

Paper must run independently of the browser and persist the same decisions, order intents, simulated orders/fills, positions, and equity/PnL records required for later replay comparison.

**Exit:** The same strategy code can run for a real-time period in Paper mode, survive normal browser disconnects, and produce a complete persistent run without real-money execution.

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

The real executor must consume the same order-intent contract used by Backtest/Paper and write actual exchange acknowledgements/fills into the same Phase 1 canonical run model. Readiness is proven without permitting automated strategy orders.

**Exit:** The system can observe/reconcile the live Binance account and has a verified execution/safety path, while automated real-money submission remains disabled.

### Phase 8 — Live trading and final three-way validation

Only after Backtest/Paper validation is complete and Phase 7 execution/safety readiness is proven do we enable strategy-driven Live orders. This is the first phase in which the strategy may submit real-money orders.

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
Phase 3   Simple grid test fixture
   ↓
Phase 4   Paper in real time
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
