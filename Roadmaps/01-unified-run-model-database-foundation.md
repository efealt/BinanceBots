# Phase 1 — Unified Run Model and Database Foundation Roadmap

## Objective

Create the canonical persistence model that Backtest, Paper, and Live will all use later. This phase defines and implements the database/storage contract only; it does **not** build the backtest engine, paper runtime, live account connection, execution logic, or strategy logic.

## Rules

- Complete phases strictly in order.
- Reuse the existing SQLite migration system and `market_instruments` identity.
- Backtest, Paper, and Live write to the same canonical tables; mode is metadata on the run, not a separate schema.
- Every run has a unique `run_id` and mode: `backtest`, `paper`, or `live`.
- Equivalent runs can share a comparison/experiment identifier so later replay results can be aligned.
- Persist the same core concepts for all modes: run, decision, order intent, order, fill, position/inventory, and equity/PnL snapshot.
- Keep strategy-specific information in version/parameter/metadata fields rather than adding strategy-specific columns to the canonical tables.
- Use UTC epoch milliseconds consistently, but preserve distinct time semantics where they exist: strategy/event time, exchange time when supplied, system receive time, and persistence/write time. Later latency/comparison work must not infer one from another.
- Prices, quantities, fees, balances, PnL, and other monetary/trading values must use an exact deterministic representation suitable for Binance execution records; do not rely on floating-point `REAL` as the canonical persisted trading value.
- Preserve raw run records. Later comparison/calibration work must not rewrite historical Paper/Live observations.
- Order lifecycle history is append/audit oriented: later status changes must not erase the earlier order-state sequence.
- Paper/Live run history has no normal runtime delete path. Database cascade behavior may exist for controlled maintenance/tests, but ordinary application behavior preserves completed/raw observations.
- No UI, API, simulator, strategy, Paper runtime, Binance private-account connection, or real order submission belongs in this roadmap.

## Sequential phases

### Phase 1.1 — Freeze the canonical data contract

- [x] Define the required identity and lifecycle fields for a run: `run_id`, mode, status, strategy identifier/version, strategy parameters, instrument, start/end timestamps, initial capital/configuration, data-source metadata, execution-assumption metadata, and optional comparison/experiment identifier.
- [x] Define canonical event identities and ordering so decisions, intents, orders, fills, positions, and equity snapshots can be reconstructed in chronological order.
- [x] Define timestamp semantics explicitly for each record: decision/event time, exchange time when applicable, system receive time, and persisted/write time. Keep unavailable timestamps nullable rather than substituting another clock.
- [x] Define which fields are common across all modes and which fields may remain nullable until a later mode supplies them, such as exchange order IDs in Live.
- [x] Define lifecycle/status values for runs and orders before encoding them as SQLite constraints.
- [x] Define the exact persisted numeric representation for price, quantity, fee, capital, balance, PnL, and related trading values so round-tripping is deterministic and does not depend on binary floating-point.
- [x] Define order-state persistence as an append-only lifecycle/event sequence rather than destructive status replacement.
- [x] Record the final contract inside the Phase 1 implementation change before creating the migration.

**Exit:** One explicit mode-neutral persistence contract exists and contains no separate Backtest/Paper/Live result schemas.

### Phase 1.2 — Add the canonical SQLite migration

- [x] Add the next numbered migration after the current schema version.
- [x] Create a canonical run table linked to the existing `market_instruments` table.
- [x] Create canonical tables for decisions, order intents, orders, fills, position/inventory snapshots, and equity/PnL snapshots.
- [x] Add the comparison/experiment linkage required to associate equivalent runs across modes.
- [x] Add foreign keys, mode/status checks, uniqueness rules, and chronological/query indexes required for deterministic run reconstruction.
- [x] Encode trading numerics using the exact representation selected in Phase 1.1; canonical run/order/fill values must not be persisted as lossy floating-point trading state.
- [x] Store order lifecycle/state transitions in append-oriented records so the full sequence from intent/submission through partial fill/fill/cancel/reject remains reconstructable.
- [x] Keep Live-only exchange identifiers nullable so the same order/fill structure remains usable by Backtest and Paper.
- [x] Define foreign-key cascade behavior for controlled maintenance/tests so removing a run cannot delete the shared instrument or historical market datasets, while exposing no normal runtime delete path for Paper/Live run history.
- [x] Register the migration in the existing atomic migration runner.

**Exit:** A fresh database and an existing migrated database both reach the new schema version with the complete canonical run model.

### Phase 1.3 — Add Rust domain/storage types

- [x] Add strongly typed Rust enums/structures for run mode, run status, order side/type/status, decision/order/fill records, and position/equity snapshots.
- [x] Keep the Rust types mode-neutral; do not create separate Backtest/Paper/Live record types for equivalent concepts.
- [x] Add explicit serialization for strategy parameters, run configuration, data-source metadata, and execution assumptions where flexible metadata is required.
- [x] Reject invalid enum/status values at the storage boundary rather than silently accepting malformed records.

**Exit:** The canonical schema has one corresponding Rust data contract that later engines can share.

### Phase 1.4 — Add write-side storage operations

- [x] Add storage operations to create a run and assign/return its `run_id`.
- [x] Add append/write operations for decisions, order intents, orders/order-state changes, fills, position snapshots, and equity/PnL snapshots.
- [x] Preserve caller-supplied strategy/event and exchange timestamps and record system receive/write timestamps separately; storage must never replace one clock with another.
- [x] Make multi-row state transitions transactional where partial persistence would create an impossible trading history.
- [x] Add run lifecycle operations for start/running/completed/failed/stopped states without deleting prior events.
- [x] Add order lifecycle writes as append-only state transitions/events; never mutate away the historical path of an order.
- [x] Keep these writes backend/storage-only; do not expose them through public application APIs in this phase.

**Exit:** A synthetic run can be persisted from creation through completion using only canonical storage operations.

### Phase 1.5 — Add read-side reconstruction operations

- [x] Add storage reads for one run and its metadata.
- [x] Add chronological reads for decisions, intents, orders, fills, positions, and equity/PnL snapshots.
- [x] Add lookup by comparison/experiment identifier so later equivalent Backtest/Paper/Live runs can be retrieved together.
- [x] Make ordering deterministic using timestamp plus stable record/event identity when timestamps are equal.
- [x] Return mode-neutral records suitable for the future Backtest, Paper, Live, and comparison layers.

**Exit:** Storage can reconstruct a complete run and retrieve a linked set of equivalent runs without mode-specific query code.

### Phase 1.6 — Verify schema invariants and persistence

- [x] Extend migration tests to verify the new tables/indexes and new schema version.
- [x] Verify migration rollback remains atomic if the new migration fails partway through.
- [x] Add storage tests that create equivalent `backtest`, `paper`, and `live` run records through the same API.
- [x] Verify invalid mode/status/foreign-key combinations are rejected.
- [x] Verify exact trading numerics round-trip through SQLite without precision drift.
- [x] Verify event/exchange/receive/write timestamps retain their distinct values through persistence and reconstruction.
- [x] Verify full append-only order lifecycle history survives close/reopen of the SQLite database together with order → fill → position/equity relationships.
- [x] Verify comparison/experiment lookup returns the linked runs while preserving each run's independent raw records.
- [x] Verify controlled run cleanup cascades only through run-owned records and leaves shared market/historical data intact, while the normal Paper/Live storage API exposes no run-deletion operation.

**Exit:** The canonical model is migration-safe, persistence-safe, mode-neutral, and proven through the same storage API for Backtest, Paper, and Live.

## Phase 1 completion condition

**Completed 2026-09-21.** Migration 004 and the shared Rust storage layer now implement the canonical mode-neutral run model for Backtest, Paper, and Live. Trading numerics use canonical decimal text; event, exchange, receive, and persistence clocks remain distinct; run/order histories are append-oriented; comparison-linked runs can be reconstructed through the same storage API; and no normal run-deletion API exists.

Verification: `cargo test --all-targets` passed all 10 repository tests in GitHub Actions, including migration rollback, exact decimal round-trip, schema constraints, persistence/reopen reconstruction, linked Backtest/Paper/Live runs, append-only order history, and controlled cascade behavior. The same code built successfully on Render, migration 004 applied to the existing persistent production SQLite database during startup, and the service returned to `live`.

**Exit achieved:** the repository contains the tested canonical SQLite + Rust storage contract required before Phase 2. No trading engine was added in Phase 1.
