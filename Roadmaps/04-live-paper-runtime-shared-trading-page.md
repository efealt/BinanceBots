# Phase 4 — Live Paper Runtime + Shared Trading Page

Status: IN PROGRESS — 4.1–4.8A COMPLETE

## Prerequisite

Phase 3.7 is complete. Strategy implementations now live under `src/trading/strategies/`, with the current fixture in `static_grid.rs` and future strategy concepts isolated into their own modules. Phase 4 must use the shared `Strategy` interface and must not fold Paper-specific behavior into any strategy module.

## Objective

Run the same strategy engine against **real-time Binance public market data** in Paper mode on Render, while sending **no real Binance account orders**.

Phase 4 also creates the permanent **Trading** page used by both Paper and future Live mode. In this phase, Paper is fully operational and Live is visible but locked.

The browser is an operator/monitor only. The Live-Paper runtime belongs to the backend and continues when the browser is closed.

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

## 4.1 — Live-Paper runtime contract

Build the runtime foundation before building the page.

- [x] Add a backend Paper-run manager keyed by canonical `run_id`.
- [x] Reuse the existing `Strategy`, portfolio state, simulated execution, and persistence contracts rather than creating Paper-specific strategy logic.
- [x] Define explicit Paper lifecycle states using the canonical run lifecycle: created → running → completed/stopped/failed.
- [x] Keep runtime state isolated per run so the architecture does not depend on one global bot, even if UI/resource policy initially limits active runs.

**Checkpoint:** A Live-Paper run has a clear backend lifecycle and uses the same core trading contracts as Backtest.

---

## 4.2 — Real-time candle clock

Make the market clock deterministic before strategy execution is enabled.

- [x] Feed Paper from the existing backend Binance public market-data path, never from browser-delivered data.
- [x] Use completed 1-minute market candles as the base stream and aggregate them in UTC to the configured 1m / 1h / 1d Paper interval using the same bucket rules as Backtest.
- [x] Start a new Live-Paper run on a clean replay boundary. If started during an incomplete interval, enter an **arming** state and begin at the next full interval rather than treating a partial candle as a complete historical candle.
- [x] Bootstrap the immediately previous completed replay candle before `on_start` so previous-close strategies have the same information contract as Backtest.
- [x] Detect duplicate, out-of-order, stale, and missing candles before they reach the strategy.

**Checkpoint:** Given the same completed candle sequence, Paper and Backtest expose candles to the strategy in the same chronological order.

---

## 4.3 — Paper strategy + simulated execution loop

Connect the real-time clock to the already-proven engine semantics.

For every completed Paper replay candle:

1. [x] Process orders that were already active before that candle using the shared simulated-execution rules.
2. [x] Update portfolio, position, fees, mark, PnL, and equity.
3. [x] Persist fills/position/equity through the canonical run model.
4. [x] Only after the candle is complete, call the strategy's normal `on_candle`.
5. [x] Persist its Decision / OrderIntent / Order events; those new orders cannot retroactively fill from the candle that produced them.

Also:

- [x] Apply the configured fees, spread, slippage, latency, touch/trade-through, and partial-fill assumptions exactly as recorded on the run.
- [x] Keep Paper order IDs and event ordering deterministic and auditable.

**Checkpoint:** The real-time Paper loop obeys the same no-lookahead and resting-order timing rules already established in Backtest.

---

## Remaining execution rule

From 4.4 onward, each substep below is a separate implementation unit: **implement → focused tests → commit → one CI/deploy verification → stop**. Do not combine adjacent substeps unless the user explicitly asks.

---

## 4.4 — Runtime integrity, stop, and failure behavior

### 4.4A — Stop semantics + backend ownership

- [x] Make **Stop** idempotent.
- [x] Persist the terminal `stopped` state and stop reason.
- [x] Guarantee no further strategy decisions or fills after stop.
- [x] Confirm browser disconnect/logout does not stop a backend Live-Paper run.

**Checkpoint:** A Live-Paper run is backend-owned and stops exactly once when explicitly requested.

### 4.4B — Feed reconnect + gap integrity

- [x] Resume after feed reconnect only when candle continuity is provable.
- [x] Reject/fail on an unrecoverable missing completed interval rather than silently backfilling it as real-time Paper.
- [x] Persist the failure reason for later Paper-vs-Backtest analysis.

**Checkpoint:** A Live-Paper run never crosses an unknown market-data gap silently.

### 4.4C — Service restart handling

- [x] Detect Live-Paper runs that were `created` or `running` when the service restarts.
- [x] Recover only when chronology can be proven safe; otherwise terminate the interrupted run explicitly.
- [x] Persist the restart/interruption reason.

**Checkpoint:** A Render restart cannot masquerade as uninterrupted Paper observation.

---

## 4.5 — Trading control API + live stream

### 4.5A — Control endpoints + Live lock

- [x] Add authenticated endpoints to create/start, stop, inspect, and list Live-Paper runs.
- [x] Reject every `mode=live` start/control path server-side in Phase 4.

**Checkpoint:** Paper can be controlled through authenticated backend APIs and Live cannot be started.

### 4.5B — Complete snapshot contract

- [x] Return run status, strategy/config, market, position, cash/equity/PnL, fees, open orders, recent fills, and feed health.
- [x] Make the snapshot sufficient to rebuild the Trading page after refresh without relying on browser state.

**Checkpoint:** One authenticated snapshot contains the complete current Paper state required by the UI.

### 4.5C — Run live stream + reconnect contract

- [x] Add an authenticated run-specific live stream for candles, order changes, fills, position/equity changes, runtime status, and feed status.
- [x] Reconnect using **snapshot first → live events second**.
- [x] Prevent refresh/reconnect from losing or duplicating visible runtime state.

**Checkpoint:** The browser can disconnect and reconnect without becoming runtime owner.

---

## 4.6 — Shared Trading page

### 4.6A — Page shell + mode/status header

- [x] Add the authenticated **Trading** page and navigation entry.
- [x] Add a prominent **PAPER | LIVE** selector.
- [x] Keep Live visibly locked in Phase 4.
- [x] Show run ID/status and Binance feed/connection health.

**Checkpoint:** The permanent Paper/Live page shell exists with unambiguous mode/status state.

### 4.6B — Configuration + run controls

- [x] Expose the current strategy parameters and Live-Paper execution assumptions.
- [x] Add **Start Paper** and **Stop** controls.
- [x] Lock configuration while a run is active.
- [x] Prevent mode changes while a run is active.

**Checkpoint:** A Live-Paper run can be safely configured and controlled from the page.

### 4.6C — Live chart + order/fill overlays

- [x] Render the live candlestick chart.
- [x] Draw active buy/sell grid/order levels for their actual active lifetime.
- [x] Add buy/sell fill markers in real time.

**Checkpoint:** The chart visually explains what the Paper strategy is doing against the live market.

### 4.6D — Portfolio, orders, and audit panels

- [x] Show current position/inventory, cash, equity, realized/unrealized PnL, fees, and exposure.
- [x] Show open orders with side, price, quantity, fill state, and age.
- [x] Show the event/fill audit stream with clear names and timestamps.
- [x] Preserve access to the complete run record rather than silently truncating it.

**Checkpoint:** The page provides both operational monitoring and exact auditability.

---

## 4.7 — Paper/Live shared UI contract

- [x] Keep the frontend monitoring model mode-neutral: market, orders, fills, position, equity/PnL, fees, status, and events use one contract.
- [x] Keep execution-specific behavior behind adapters: Phase 4 simulated execution, Phase 8 real Binance execution.
- [x] Permit mode-specific setup/safety panels without duplicating the core page.
- [x] Keep future Live styling unmistakable from Paper.

**Checkpoint:** Phase 7/8 can extend/unlock this same Trading page instead of replacing it.

---

## 4.8 — Final Phase 4 verification

### 4.8A — Automated parity + safety verification

- [x] Feed the same synthetic completed-candle sequence through Backtest and Paper and compare decisions, intents, fills, accounting, and ordering.
- [x] Verify start-at-mid-interval arming and previous-candle bootstrap.
- [x] Verify duplicate/out-of-order/stale/gap handling.
- [x] Verify idempotent stop and snapshot/live-stream reconnect behavior.
- [x] Verify every Phase 4 Live-mode start path is rejected server-side.
- [x] Run the existing Backtest/grid regression unchanged.

**Checkpoint:** Automated tests prove the Phase 4 chronology and safety contract without using production UI behavior as evidence.

### 4.8B — Trading workspace + persistent bots

Detailed roadmap: `Roadmaps/04.8B-trading-workspace-production-revision.md`

Foundation already complete:
- [x] Active workspace no longer auto-loads terminal historical runs.
- [x] Registered Symbol/Market drives an always-live Trading chart before execution.
- [x] Trading uses the same native Lightweight Charts engine as Market.

Remaining:
- [ ] Add persistent Bot identity + saved configuration above the existing Run model.
- [x] Add the top Bot strip, bot selection, New Bot, Save Bot, and unsaved-change state.
- [x] Make Live-Paper execution bot-aware and support multiple different Live-Paper bots concurrently.
- [x] Add backend-derived **Show on graph** preview for the selected/new bot.
- [ ] Add Bot history, clear Run activity terminology, and compact the remaining workspace layout.
- [ ] Complete integrated multi-bot production acceptance.

**Checkpoint:** Trading is the persistent operational home for saved bots and their forward Live-Paper runs. Live-Real-Account execution remains locked until its later phase.

### 4.8C — Production smoke + visual acceptance

- [ ] Create and save an idle bot without starting a run.
- [ ] Start at least two different Live-Paper bots and verify concurrent backend ownership and state isolation.
- [ ] Verify live candles, preview/grid/order lines, fills, positions, equity/PnL, fees, exposure, run activity, and Bot history.
- [ ] Close/reopen the page and verify all running bots continue and restore correctly.
- [ ] Stop/restart a bot and verify separate immutable run IDs remain under the same Bot history.
- [ ] Update `Documents/ARCHITECTURE.md` with the final implemented Bot → Run architecture.
- [ ] Mark Phase 4 complete only after the hosted workspace is visually meaningful and persisted bot/run state is coherent.

**Checkpoint:** Phase 4 is proven end-to-end in production.

## Completion condition

Phase 4 is complete when the hosted application can start, monitor, reconnect to, and stop a real-time **Paper** run on Render using the same strategy and canonical trading contracts proven in Backtest.

The shared Trading page must show the live market, active grid/orders, fills, position, equity/PnL, exposure, fees, runtime/feed status, and audit events. Closing the browser must not stop the run.

**Live mode remains locked and no real Binance orders are possible.**

The next phase is **Phase 5 — Paper-period historical replay**.
