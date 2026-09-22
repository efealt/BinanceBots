# Phase 2 — Backtest Engine Roadmap

## Objective

Build the deterministic backend Backtest engine on top of Phase 1 persistence and downloaded historical OHLCV. This phase creates reusable runtime components that Paper will later reuse. It does not add the grid strategy, Paper runtime, Live account integration, or real order submission.

## Rules

- Use the Phase 1 canonical run/event/order/fill/position/equity storage model.
- Historical time is authoritative. A strategy may only see a candle after that candle is complete.
- An order created from a completed candle cannot use that candle's earlier high/low/open to obtain a fill.
- Same-timestamp ordering is deterministic and represented by the existing per-run event sequence.
- Strategy code is mode-neutral. It receives market/portfolio context and emits decisions/order intents; it does not know whether execution is Backtest, Paper, or Live.
- Simulated execution is a reusable component, not embedded inside strategy code.
- Execution assumptions are explicit run configuration.
- No strategy-specific rules belong in the engine.
- No UI/API work is required in this phase.

## Sequential implementation

### Phase 2.1 — Historical dataset/clock boundary

- [ ] Add storage reads for dataset identity plus bounded chronological OHLCV.
- [ ] Define the historical clock so a candle becomes visible at its close time.
- [ ] Reject empty or invalid time ranges.
- [ ] Guarantee strictly chronological candle processing.

**Exit:** The engine can consume a selected stored dataset/range without exposing future candles.

### Phase 2.2 — Shared strategy contract

- [ ] Define a mode-neutral Strategy trait.
- [ ] Define strategy context containing only current completed market data, current simulated time, and portfolio state.
- [ ] Define strategy outputs for auditable decisions and order intents.
- [ ] Keep strategy identifier/version/parameters available for canonical run metadata.

**Exit:** A test-only strategy can run without Backtest-specific APIs.

### Phase 2.3 — Portfolio and order state

- [ ] Add deterministic in-memory portfolio state for cash, signed position quantity, average entry, realized/unrealized PnL, fees, and equity.
- [ ] Add in-memory simulated order state linked to canonical persisted order IDs.
- [ ] Make state transitions explicit and deterministic.

**Exit:** The engine can reconstruct its current portfolio/order state solely from processed events.

### Phase 2.4 — Reusable simulated execution

- [ ] Implement market and limit order simulation.
- [ ] Prevent same-candle look-ahead by making newly submitted orders eligible only on later market data.
- [ ] Support explicit fee, slippage, latency, limit-touch policy, and deterministic partial-fill assumptions.
- [ ] Persist created/accepted/partial/filled order lifecycle events through Phase 1 storage.
- [ ] Persist fills, positions, and equity using the canonical model.

**Exit:** A strategy intent can travel through the same order/fill contract later reusable by Paper.

### Phase 2.5 — Backtest runner

- [ ] Create a Backtest run with dataset/range/execution assumptions captured in run metadata.
- [ ] Transition run lifecycle created → running → completed/failed.
- [ ] Process each completed candle in deterministic order: execute previously eligible orders, mark portfolio, call strategy, persist decisions/intents/new orders, then snapshot equity.
- [ ] Return a compact run result containing run ID and final portfolio state.
- [ ] Mark failed runs without deleting prior events.

**Exit:** A backend Backtest consumes stored history and persists a complete canonical run.

### Phase 2.6 — Verification

- [ ] Test that a strategy cannot fill from the candle that generated its signal.
- [ ] Test deterministic replay produces equivalent event/fill/equity results.
- [ ] Test market slippage/fees.
- [ ] Test limit touch vs trade-through behavior.
- [ ] Test latency delays eligibility.
- [ ] Test deterministic partial fills.
- [ ] Test portfolio/equity accounting and canonical persistence.
- [ ] Test failure lifecycle preserves prior events.
- [ ] Run the full repository test suite.
- [ ] Verify Render builds, applies the current schema, and returns live.

**Exit:** Phase 2 is complete and Phase 3 can add the simple grid only through this shared engine.
