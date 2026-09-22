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
- Use UTC epoch milliseconds for event/run timestamps, consistent with the existing database.
- Preserve raw run records. Later comparison/calibration work must not rewrite historical Paper/Live observations.
- No UI, API, simulator, strategy, Paper runtime, Binance private-account connection, or real order submission belongs in this roadmap.

## Sequential phases

### Phase 1.1 — Freeze the canonical data contract

- [ ] Define the required identity and lifecycle fields for a run: `run_id`, mode, status, strategy identifier/version, strategy parameters, instrument, start/end timestamps, initial capital/configuration, data-source metadata, execution-assumption metadata, and optional comparison/experiment identifier.
- [ ] Define canonical event identities and ordering so decisions, intents, orders, fills, positions, and equity snapshots can be reconstructed in chronological order.
- [ ] Define which fields are common across all modes and which fields may remain nullable until a later mode supplies them, such as exchange order IDs in Live.
- [ ] Define lifecycle/status values for runs and orders before encoding them as SQLite constraints.
- [ ] Record the final contract inside the Phase 1 implementation change before creating the migration.

**Exit:** One explicit mode-neutral persistence contract exists and contains no separate Backtest/Paper/Live result schemas.

### Phase 1.2 — Add the canonical SQLite migration

- [ ] Add the next numbered migration after the current schema version.
- [ ] Create a canonical run table linked to the existing `market_instruments` table.
- [ ] Create canonical tables for decisions, order intents, orders, fills, position/inventory snapshots, and equity/PnL snapshots.
- [ ] Add the comparison/experiment linkage required to associate equivalent runs across modes.
- [ ] Add foreign keys, mode/status checks, uniqueness rules, and chronological/query indexes required for deterministic run reconstruction.
- [ ] Keep Live-only exchange identifiers nullable so the same order/fill structure remains usable by Backtest and Paper.
- [ ] Ensure deleting a run removes only its run-owned event/result records and cannot delete the shared instrument or historical market datasets.
- [ ] Register the migration in the existing atomic migration runner.

**Exit:** A fresh database and an existing migrated database both reach the new schema version with the complete canonical run model.

### Phase 1.3 — Add Rust domain/storage types

- [ ] Add strongly typed Rust enums/structures for run mode, run status, order side/type/status, decision/order/fill records, and position/equity snapshots.
- [ ] Keep the Rust types mode-neutral; do not create separate Backtest/Paper/Live record types for equivalent concepts.
- [ ] Add explicit serialization for strategy parameters, run configuration, data-source metadata, and execution assumptions where flexible metadata is required.
- [ ] Reject invalid enum/status values at the storage boundary rather than silently accepting malformed records.

**Exit:** The canonical schema has one corresponding Rust data contract that later engines can share.

### Phase 1.4 — Add write-side storage operations

- [ ] Add storage operations to create a run and assign/return its `run_id`.
- [ ] Add append/write operations for decisions, order intents, orders/order-state changes, fills, position snapshots, and equity/PnL snapshots.
- [ ] Preserve caller-supplied event timestamps; storage must not replace strategy/exchange event time with database-write time.
- [ ] Make multi-row state transitions transactional where partial persistence would create an impossible trading history.
- [ ] Add run lifecycle operations for start/running/completed/failed/stopped states without deleting prior events.
- [ ] Keep these writes backend/storage-only; do not expose them through public application APIs in this phase.

**Exit:** A synthetic run can be persisted from creation through completion using only canonical storage operations.

### Phase 1.5 — Add read-side reconstruction operations

- [ ] Add storage reads for one run and its metadata.
- [ ] Add chronological reads for decisions, intents, orders, fills, positions, and equity/PnL snapshots.
- [ ] Add lookup by comparison/experiment identifier so later equivalent Backtest/Paper/Live runs can be retrieved together.
- [ ] Make ordering deterministic using timestamp plus stable record/event identity when timestamps are equal.
- [ ] Return mode-neutral records suitable for the future Backtest, Paper, Live, and comparison layers.

**Exit:** Storage can reconstruct a complete run and retrieve a linked set of equivalent runs without mode-specific query code.

### Phase 1.6 — Verify schema invariants and persistence

- [ ] Extend migration tests to verify the new tables/indexes and new schema version.
- [ ] Verify migration rollback remains atomic if the new migration fails partway through.
- [ ] Add storage tests that create equivalent `backtest`, `paper`, and `live` run records through the same API.
- [ ] Verify invalid mode/status/foreign-key combinations are rejected.
- [ ] Verify order → fill → position/equity relationships survive close/reopen of the SQLite database.
- [ ] Verify comparison/experiment lookup returns the linked runs while preserving each run's independent raw records.
- [ ] Verify deleting a run cascades only through run-owned records and leaves shared market/historical data intact.

**Exit:** The canonical model is migration-safe, persistence-safe, mode-neutral, and proven through the same storage API for Backtest, Paper, and Live.

## Phase 1 completion condition

Phase 1 is complete when the repository contains a tested canonical SQLite + Rust storage contract capable of representing and reconstructing equivalent Backtest, Paper, and Live runs with the same core records.

No trading engine should be implemented during this phase. Phase 2 begins only after this storage foundation is complete.
