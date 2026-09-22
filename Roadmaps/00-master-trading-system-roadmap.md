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
- A strategy is eligible for Paper-vs-Backtest or Live-vs-Backtest comparison only when the historical dataset contains the inputs that strategy actually used. If a strategy depends on trades, depth, or other microstructure unavailable in the downloaded history, those inputs must be captured/persisted before that strategy can be fairly replayed.
- Raw observed Paper/Live results remain immutable. Later calibrated execution assumptions are stored separately from the observations used to derive them.
- Real Binance order submission remains disabled until the final phase.
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

Each run records at least its mode, strategy/version, parameters, symbol/market, capital/configuration, timing, data source, and execution assumptions.

## Sequential phases

### Phase 1 — Unified run model and database foundation

Define the canonical Backtest/Paper/Live data contract first, then add the migrations and storage APIs required to persist it.

The database distinguishes mode through run metadata rather than by creating unrelated result structures for each environment. It also supports linking equivalent runs for later comparison.

**Exit:** The database can represent equivalent Backtest, Paper, and Live runs and their decisions/orders/fills/results before any new engine writes them.

### Phase 2 — Backtest engine

Build the historical-data clock, shared strategy interface, portfolio/order state, deterministic event sequencing, and simulated execution path on top of downloaded data.

The simulator supports explicit execution assumptions such as fees, spread, slippage, latency, limit-order fill rules, partial fills, and conservative scenarios without changing strategy logic.

No future candle information may influence a decision or fill that occurs earlier in simulated time.

**Exit:** A deterministic historical run can consume stored Binance data and persist a complete comparable run through the canonical data model.

### Phase 3 — Minimal grid strategy test fixture

Implement one deliberately simple grid strategy through the shared strategy interface.

Its purpose is only to prove signals, order intents, simulated fills, state transitions, persistence, reproducibility, and the Backtest engine end to end. It is not the final trading strategy.

**Exit:** The simple grid runs end to end in Backtest using only the shared engine and canonical storage model.

### Phase 4 — Live Paper runtime

Add a backend-owned Paper mode that consumes real-time Binance market data but sends **no Binance account orders**.

Paper uses the same strategy engine and canonical run model as Backtest. Only the clock/data source and simulated execution environment differ.

Paper runs independently of the browser and persists all decisions, order intents, simulated orders/fills, positions, and PnL needed for later replay comparison.

**Exit:** The same test strategy can run for a real-time period in Paper mode and produce a complete persistent run without any real-money execution.

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

### Phase 6 — Paper vs Backtest validation and calibration

Build comparison tooling for the aligned Paper and Backtest runs before any live trading is attempted.

Compare signal timing, intended orders, simulated fills, position path, PnL, drawdown, fees, slippage assumptions, missed/partial fills, and other execution differences.

Use the observed differences to improve realistic and conservative Backtest/Paper execution assumptions while keeping the original raw runs unchanged.

**Exit:** We can quantify how closely Backtest reproduces Paper and have evidence-based simulation settings before connecting strategy execution to real money.

### Phase 7 — Live-account integration and execution readiness

Add authenticated Binance account connectivity and the real execution infrastructure, but keep **strategy-driven live order submission disabled**.

This phase establishes account/balance/position reads, open-order state, private order/fill updates, fees, exchange filters and rounding, idempotent client order IDs, reconnect/reconciliation, stale-data protection, risk limits, emergency-stop behavior, and persistent execution audit records.

The live executor sits behind the same order-intent interface used by Backtest and Paper, but it is not permitted to submit strategy orders yet.

**Exit:** The system can observe and reconcile the live Binance account and has a fully integrated execution/safety path, while automated real-money order submission remains off.

### Phase 8 — Live trading and final three-way validation

Only after Backtest and Paper have been compared and the live-account execution/safety path has been verified do we enable strategy-driven Live orders.

Run the same strategy version/configuration in Live and, where useful, Paper over the same real-time period. Persist actual Binance acknowledgements, fills, fees, timestamps, positions, and PnL in the canonical run model.

After that Live period has ended and the corresponding Binance historical data becomes available:

1. download and verify the same historical interval;
2. replay the same strategy/configuration as Backtest;
3. align Backtest, Paper, and Live runs;
4. compare signals, intended orders, fill prices/timing, slippage, fees, partial/missed fills, position paths, PnL, and drawdown;
5. calibrate realistic and conservative simulation assumptions from measured Live execution differences without altering the raw runs.

**Exit:** The system has proven the same strategy through Backtest → Paper → Live in chronological order and can quantify how realistically Backtest and Paper represent actual Live execution.

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
Phase 6   Paper ↔ Backtest comparison/calibration
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
