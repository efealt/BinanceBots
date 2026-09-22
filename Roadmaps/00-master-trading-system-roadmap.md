# Master Trading System Roadmap

## Objective

Build one backend trading/research system in which **Backtest, Paper, and Live use the same strategy interface and the same canonical run/order/fill/result data model**. The modes differ only by market-data clock and execution source. Detailed implementation roadmaps will be created separately for each phase when that phase begins.

This roadmap is intentionally about backend/runtime/data architecture, not strategy research. A simple grid strategy will be added only as a test vehicle once the shared engine is ready.

## Rules

- Complete phases strictly in order.
- Database/schema changes required by a phase are implemented before that phase writes runtime results.
- Backtest, Paper, and Live must not become separate strategy implementations.
- Every strategy run has a unique `run_id` and a mode such as `backtest`, `paper`, or `live`.
- The three modes persist the same core event/result concepts so runs can be compared directly.
- A historical replay comparison for a Paper/Live period happens only **after that real-time period has ended and the corresponding Binance historical data has become available and been downloaded/verified**.
- Strategy sophistication is out of scope for this roadmap. Backend correctness, timing, execution modeling, persistence, recovery, and comparability come first.
- Before implementing each phase, create a separate detailed roadmap for that phase.

## Shared run contract

The exact schema belongs to Phase 1, but all three modes use the same conceptual records:

```text
Run
Decision
OrderIntent
Order
Fill
Position / Inventory
PnL / Equity Snapshot
```

Each run records enough identity to reproduce and compare it: mode, strategy/version, parameters, symbol/market, timing, data source, and execution assumptions.

## Sequential phases

### Phase 1 — Unified run model and database foundation

Define the canonical Backtest/Paper/Live data contract first, then add the migrations and storage APIs required to persist it.

The model must distinguish mode through run metadata rather than by inventing different result structures for each environment.

**Exit:** The database can represent equivalent Backtest, Paper, and Live runs and their decisions/orders/fills/results before any new engine writes them.

### Phase 2 — Backtest engine

Build the historical-data clock, strategy interface, portfolio/order state, and simulated execution path on top of downloaded data.

The execution simulator must be designed to support configurable fill assumptions such as fees, spread, slippage, latency, limit-order fill rules, partial fills, and conservative/worst-case scenarios without changing strategy logic.

All backtest output is written through the Phase 1 canonical run model.

**Exit:** A deterministic historical run can consume stored Binance data and persist a complete comparable run without browser-owned state.

### Phase 3 — Minimal grid strategy test fixture

Implement one deliberately simple grid strategy through the shared strategy interface.

Its purpose is to prove signals, order intents, simulated fills, state transitions, persistence, restart behavior where applicable, and reproducibility. It is not the final trading strategy.

**Exit:** The simple grid can run end-to-end in Backtest using only the shared engine and canonical storage model.

### Phase 4 — Live Paper runtime

Add a backend-owned Paper mode that consumes real-time Binance market data but sends no account orders.

Paper uses the same strategy engine and canonical run model as Backtest. Only the clock/data source and simulated execution environment differ.

Paper execution assumptions remain explicit and configurable so later Live observations can calibrate them.

**Exit:** A Paper run can operate for hours/days independently of the browser and persist the same run/order/fill/result structure as Backtest.

### Phase 5 — Binance live-account connection and observation

Add authenticated Binance account connectivity **without enabling strategy-driven live trading yet**.

Read and persist the exchange state needed for later execution and comparison: account/balance state, positions, open orders, order updates, fills, fees, and relevant timestamps/IDs.

The account connection, private stream, reconnect behavior, and exchange reconciliation are proven before any strategy is allowed to submit real orders.

**Exit:** Binance account/execution data can be observed and recorded reliably while no automated strategy can place live orders.

### Phase 6 — Live execution

Add the real Binance executor behind the same strategy/order-intent interface already used by Backtest and Paper.

Live execution includes exchange filters/rounding, idempotent client order IDs, risk limits, stale-data protection, partial-fill/rejection handling, emergency stop behavior, reconnect/reconciliation, and persistent execution audit history.

The strategy itself remains unchanged when switching between Paper and Live.

**Exit:** The test strategy can run in Live mode with actual Binance fills recorded into the same canonical data model, under explicit safety controls.

### Phase 7 — Post-period historical replay

After a Paper/Live observation period has ended, download and verify the Binance historical data covering that same period.

Then rerun the **same strategy version and parameters** as a Backtest over that completed interval. The replay must use only information that would have been available at each historical decision time.

Example:

```text
Day 1:
  Paper and/or Live run in real time

Day 2:
  Download verified Day 1 Binance history
  Replay the same configuration as Backtest
```

**Exit:** The system contains temporally aligned Backtest, Paper, and where applicable Live runs covering the same completed market interval.

### Phase 8 — Three-way comparison and execution calibration

Build comparison tooling that aligns equivalent runs and trades across Backtest, Paper, and Live.

Compare signal timing, intended orders, actual/simulated fills, fill delay, slippage, fees, partial/missed fills, position path, PnL, drawdown, and other execution differences.

Use observed Paper-vs-Live and Backtest-vs-Live differences to calibrate realistic and conservative Backtest/Paper execution assumptions. Keep raw observed results separate from calibrated assumptions.

**Exit:** The system can quantify how closely Backtest and Paper represent Live execution and use measured evidence to improve simulation assumptions.

## Intended end state

```text
                    Shared Strategy Engine
                           |
          +----------------+----------------+
          |                |                |
      Backtest           Paper             Live
 historical clock     live clock        live clock
 simulated exec      simulated exec    Binance exec
          |                |                |
          +----------------+----------------+
                           |
                 Canonical Run Data Model
                           |
                 Three-Way Comparison
                           |
          Evidence-based simulation calibration
```

The result is one strategy/runtime architecture with three execution environments, not three separate trading systems.
