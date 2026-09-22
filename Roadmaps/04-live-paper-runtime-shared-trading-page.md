# Phase 4 — Live Paper Runtime + Shared Trading Page

Status: IN PROGRESS — 4.1 COMPLETE

## Prerequisite

Phase 3.7 is complete. Strategy implementations now live under `src/trading/strategies/`, with the current fixture in `static_grid.rs` and future strategy concepts isolated into their own modules. Phase 4 must use the shared `Strategy` interface and must not fold Paper-specific behavior into any strategy module.

## Objective

Run the same strategy engine against **real-time Binance public market data** in Paper mode on Render, while sending **no real Binance account orders**.

Phase 4 also creates the permanent **Trading** page used by both Paper and future Live mode. In this phase, Paper is fully operational and Live is visible but locked.

The browser is an operator/monitor only. The Paper runtime belongs to the backend and continues when the browser is closed.

## Non-negotiable rules

- Paper uses the existing mode-neutral strategy interface and canonical Run / Decision / OrderIntent / Order / Fill / Position / Equity model.
- Paper uses the same simulated-execution rules already proven in Backtest.
- Strategy decisions may use only information available at that real-time moment.
- No Binance private-account connection or real-order submission is added in Phase 4.
- Live mode is rejected server-side in Phase 4; disabling a browser button is not the safety boundary.
- User stop and feed/runtime failure are persisted explicitly. Do not silently invent continuity across an invalid market-data gap.
- The new Trading page is shared infrastructure for Phase 4 Paper, Phase 7 live-account readiness, and Phase 8 Live execution.
- Do not change the grid strategy rules as part of Phase 4.

---

## 4.1 — Paper runtime contract

Build the runtime foundation before building the page.

- [x] Add a backend Paper-run manager keyed by canonical `run_id`.
- [x] Reuse the existing `Strategy`, portfolio state, simulated execution, and persistence contracts rather than creating Paper-specific strategy logic.
- [x] Define explicit Paper lifecycle states using the canonical run lifecycle: created → running → completed/stopped/failed.
- [x] Keep runtime state isolated per run so the architecture does not depend on one global bot, even if UI/resource policy initially limits active runs.

**Checkpoint:** A Paper run has a clear backend lifecycle and uses the same core trading contracts as Backtest.

---

## 4.2 — Real-time candle clock

Make the market clock deterministic before strategy execution is enabled.

- [ ] Feed Paper from the existing backend Binance public market-data path, never from browser-delivered data.
- [ ] Use completed 1-minute market candles as the base stream and aggregate them in UTC to the configured 1m / 1h / 1d Paper interval using the same bucket rules as Backtest.
- [ ] Start a new Paper run on a clean replay boundary. If started during an incomplete interval, enter an **arming** state and begin at the next full interval rather than treating a partial candle as a complete historical candle.
- [ ] Bootstrap the immediately previous completed replay candle before `on_start` so previous-close strategies have the same information contract as Backtest.
- [ ] Detect duplicate, out-of-order, stale, and missing candles before they reach the strategy.

**Checkpoint:** Given the same completed candle sequence, Paper and Backtest expose candles to the strategy in the same chronological order.

---

## 4.3 — Paper strategy + simulated execution loop

Connect the real-time clock to the already-proven engine semantics.

For every completed Paper replay candle:

1. [ ] Process orders that were already active before that candle using the shared simulated-execution rules.
2. [ ] Update portfolio, position, fees, mark, PnL, and equity.
3. [ ] Persist fills/position/equity through the canonical run model.
4. [ ] Only after the candle is complete, call the strategy's normal `on_candle`.
5. [ ] Persist its Decision / OrderIntent / Order events; those new orders cannot retroactively fill from the candle that produced them.

Also:

- [ ] Apply the configured fees, spread, slippage, latency, touch/trade-through, and partial-fill assumptions exactly as recorded on the run.
- [ ] Keep Paper order IDs and event ordering deterministic and auditable.

**Checkpoint:** The real-time Paper loop obeys the same no-lookahead and resting-order timing rules already established in Backtest.

---

## 4.4 — Runtime integrity, stop, and failure behavior

Make backend ownership safe before exposing start/stop controls.

- [ ] User **Stop** must be idempotent, persist the terminal state, and prevent further strategy decisions/fills.
- [ ] Browser disconnect/logout must not stop an active Paper run.
- [ ] Market-feed reconnect may resume only when the runtime can prove the required strategy candle is still complete and chronological.
- [ ] If a required completed interval cannot be reconstructed without violating real-time chronology, fail the Paper run explicitly rather than silently backfilling it later and calling it real-time Paper.
- [ ] On process/service restart, detect any run that had been `running` and terminate/recover it according to the same chronology-integrity rule; never silently pretend uninterrupted observation.
- [ ] Persist a clear failure/stop reason for later Phase 5/6 comparison.

**Checkpoint:** A Paper run never continues through an unknown market-data period without an explicit, auditable state transition.

---

## 4.5 — Trading control API + live stream

Expose the runtime through authenticated backend interfaces.

- [ ] Add authenticated endpoints to create/start, stop, inspect, and list Paper runs.
- [ ] Return a complete current snapshot: run status, strategy/config, market, position, cash/equity/PnL, fees, open orders, latest fills, and feed health.
- [ ] Add an authenticated run-specific live stream for price/candle updates, order-state changes, fills, position/equity changes, runtime status, and feed status.
- [ ] Reconnection flow is **snapshot first → then live events**, so refreshing the page cannot lose state.
- [ ] Server-side requests for `mode=live` must return a locked/not-enabled response throughout Phase 4.

**Checkpoint:** The Trading page can be rebuilt entirely from backend state after refresh without being the owner of the runtime.

---

## 4.6 — Shared Trading page

Create one operational page that survives into Phase 7 and Phase 8.

### Top controls

- [ ] Add a new authenticated **Trading** navigation/page.
- [ ] Add a prominent **PAPER | LIVE** mode selector.
- [ ] Paper is selectable; Live is visibly locked and explains that real execution is not enabled yet.
- [ ] Show run ID/status plus Binance feed/connection health at the top.

### Main live view

- [ ] Live candlestick chart updates from backend market data.
- [ ] Draw active buy/sell grid/order levels on the chart for their actual active lifetime.
- [ ] Add buy/sell fill markers as fills occur.
- [ ] Show current position/inventory, cash, equity, realized/unrealized PnL, fees, and exposure.
- [ ] Show open orders with price, side, quantity, fill state, and age.
- [ ] Show an event/fill audit stream with clear event names and timestamps.

### Run configuration and controls

- [ ] Let the user configure the current strategy and the same Paper execution assumptions used by Backtest.
- [ ] Add clear **Start Paper** and **Stop** controls.
- [ ] Once a run is active, lock configuration that would mutate the running strategy.
- [ ] Mode cannot change while a run is active.

**Checkpoint:** Closing/reopening the Trading page restores the same active Paper run and its current visual state.

---

## 4.7 — Paper/Live shared UI contract

Prevent Phase 7/8 from requiring a second page.

- [ ] Keep the monitoring model mode-neutral: market, orders, fills, position, equity/PnL, fees, status, and events use one frontend contract.
- [ ] Keep execution-specific behavior behind adapters: Phase 4 uses simulated execution; Phase 8 will use the real Binance execution adapter.
- [ ] Allow mode-specific setup/safety panels without duplicating the core chart/monitoring UI.
- [ ] Keep Live styling unmistakable so future real-money mode cannot be confused with Paper.

**Checkpoint:** Phase 7/8 can extend/unlock the existing Trading page instead of replacing it.

---

## 4.8 — Verification and production checkpoint

Verify chronology first, then runtime resilience, then UI.

- [ ] Add deterministic tests feeding the same synthetic completed-candle sequence through Backtest and Paper and compare strategy decisions/order intents/fill/accounting semantics.
- [ ] Test start-at-mid-interval arming and previous-candle bootstrap.
- [ ] Test duplicate/out-of-order/stale candle rejection and feed-gap failure behavior.
- [ ] Test idempotent start/stop and browser disconnect/reconnect via snapshot + live stream.
- [ ] Test that every Phase 4 Live-mode start path is rejected server-side.
- [ ] Run existing Backtest/grid regressions unchanged.
- [ ] Deploy to Render.
- [ ] Start a real Paper run from the hosted Trading page and verify live candles, grid/order lines, fills, positions, equity/PnL, logs, and browser-independent execution.
- [ ] Stop the run cleanly and verify the complete canonical Paper history remains persisted.
- [ ] Update `Documents/ARCHITECTURE.md` with the implemented Phase 4 architecture.

## Completion condition

Phase 4 is complete when the hosted application can start, monitor, reconnect to, and stop a real-time **Paper** run on Render using the same strategy and canonical trading contracts proven in Backtest.

The shared Trading page must show the live market, active grid/orders, fills, position, equity/PnL, exposure, fees, runtime/feed status, and audit events. Closing the browser must not stop the run.

**Live mode remains locked and no real Binance orders are possible.**

The next phase is **Phase 5 — Paper-period historical replay**.
