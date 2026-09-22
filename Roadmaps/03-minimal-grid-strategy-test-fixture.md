# Phase 3 — Minimal Grid Strategy Test Fixture Roadmap

Status: COMPLETE

## Objective

Implement one deliberately simple, mode-neutral grid strategy through the Phase 2 strategy interface and prove it end to end against both controlled synthetic cases and a **committed full real XAGUSDT 1-minute SQLite regression dataset**.

Phase 3 remains backend-only. It does not add the Backtest UI, Paper runtime, Binance account access, strategy optimization, or real-money execution. After Phase 3 is complete, work stops for Phase 3.5 so the backend can be exposed through a user-facing Backtest Strategy UI before Paper trading begins.

## Rules

- Use the existing Phase 1 canonical run/order/fill/position/equity model.
- Use the existing Phase 2 historical clock, `on_start` hook, simulator, and timing semantics.
- The grid strategy must be mode-neutral; no Backtest-only strategy path is allowed.
- The grid is a **test fixture**, not a profitability exercise. Do not optimize parameters against XAGUSDT.
- Grid parameters must be explicit configuration, not hidden constants. User-owned trading parameters are approved before the full real-data benchmark is finalized.
- Initial grid orders are created through `on_start` using only information available before the first active test candle.
- A resting order that existed before a candle is processed may fill from that candle's OHLC range.
- An order created from a completed candle may not retroactively use that candle's earlier open/high/low.
- If a fill causes a replacement/replenishment order to be created only at candle close, that new order becomes eligible on later market data; Phase 3 does not invent an intrabar path that OHLC cannot prove.
- No sampling or truncation of the committed XAGUSDT regression dataset is allowed.
- Routine GitHub Actions tests must **not re-download XAGUSDT**. The real-market fixture is downloaded/built once, committed, then reused.
- The committed fixture must contain no credentials, authentication data, sessions, user information, or production trading records.
- GitHub Actions must never mutate the committed fixture in place. Each test copies it to a temporary writable database and runs there.
- No frontend/UI/API work belongs in Phase 3. That is Phase 3.5.

## Sequential implementation

### Phase 3.1 — Freeze the real XAGUSDT regression dataset

- [x] Acquire the full XAGUSDT USD-M 1-minute public Binance history corresponding to the current production coverage target: **2026-01-07 10:00 UTC through 2026-09-20 23:59 UTC**, currently expected to contain **369,480 candles**.
- [x] Build a clean SQLite fixture using the same migration-defined schema as the application.
- [x] Include only the data required for deterministic historical backtesting: the XAGUSDT instrument metadata, its historical dataset metadata, the full 1-minute OHLCV series, and empty canonical trading-run tables created by migrations.
- [x] Exclude authentication audit rows, sessions, credentials, unrelated instruments/datasets, and any production run history.
- [x] Verify exact row count, first/last timestamps, unique candle open times, chronological order, OHLC invariants, and dataset/instrument linkage.
- [x] Record a small manifest beside the database containing source/provenance, coverage, row count, schema version, file size, and SHA-256 checksum.
- [x] Commit the frozen fixture under `test-data/` (or a compressed full-fidelity form only if GitHub's single-file limit requires it). Do not reduce the candle count to make the file fit.
- [x] Add a reproducible one-off fixture-build command/script, but do not call it during ordinary CI.

**Exit:** GitHub contains one immutable, checksum-verified, full real XAGUSDT 1-minute SQLite regression fixture. Normal test runs require no Binance download.

### Phase 3.2 — Define the minimal grid strategy contract

- [x] Implement a deliberately simple static grid through the shared `Strategy` interface.
- [x] Expose explicit configuration for the grid anchor/reference, spacing, number of levels on each side, order quantity/sizing input, and relevant order type/time-in-force.
- [x] Initialize the first resting grid through `on_start`.
- [x] Use the immediately preceding completed candle as the initialization reference when that configuration requires a historical price anchor.
- [x] Keep the first fixture candle available as pre-roll when needed so the first **active** candle begins with already-resting grid orders and no future information.
- [x] Emit auditable decisions and order intents through the existing canonical persistence path.
- [x] Do not add strategy-specific logic to the simulator or storage layer.
- [x] Do not add optimization, adaptive parameters, trend filters, leverage logic, or risk overlays in this phase.

**Exit:** The grid strategy can create a deterministic set of resting orders before the first active candle without any mode-specific code.

### Phase 3.3 — Verify grid timing and fill semantics with controlled candles

- [x] Prove an initial resting grid order may fill inside the first active candle when that candle touches/trades through the level according to the configured Phase 2 fill policy.
- [x] Prove an order created from candle N's completed information cannot use candle N's earlier high/low/open to fill retroactively.
- [x] Prove multiple grid levels that were already resting before the candle may each be evaluated against that candle.
- [x] Prove replacement/replenishment orders created only after candle-close information are not allowed to fill earlier within that same candle.
- [x] Verify fees, spread/slippage, latency, partial-fill behavior, positions, cash, PnL/equity, and order lifecycle remain those of the shared Phase 2 engine.
- [x] Verify the grid strategy itself never reads future candles or bypasses the simulator.

**Exit:** Controlled tests demonstrate the exact grid timing model agreed for OHLC backtesting, including the distinction between resting intrabar orders and retroactive same-candle fills.

### Phase 3.4 — Run the actual full XAGUSDT regression backtest in GitHub Actions

- [x] Update the core trading workflow so the committed XAGUSDT fixture is copied to a temporary writable SQLite database before execution.
- [x] Run the **real Phase 2 Backtest engine** and Phase 3 grid strategy across the full committed XAGUSDT historical fixture, not a mocked or sampled series.
- [x] Use one pre-roll candle for initialization when required; report active-candle count separately so the full fixture coverage remains transparent.
- [x] Persist the run through the same Phase 1 canonical tables used by normal Backtest runs.
- [x] Verify the run reaches `completed`, produces order intents/orders/fills/position/equity records, and leaves the original committed fixture byte-for-byte unchanged.
- [x] Run the same configuration a second time and compare semantic outputs deterministically (orders/fills/positions/equity and event ordering), excluding intentionally unique/runtime fields such as `run_id` and persistence timestamps.
- [x] Save a compact regression baseline/summary containing the fixture checksum, strategy/version/parameters, execution assumptions, candle count, order/fill counts, fees, final position, final equity/PnL, and a deterministic semantic-result checksum.
- [x] Fail CI if the fixture checksum changes unexpectedly or the deterministic regression output changes without an intentional reviewed update.

**Exit:** A GitHub-hosted worker has genuinely executed the production Backtest engine and minimal grid across the full committed real XAGUSDT dataset, with repeatable results and no network data download.

### Phase 3.5 (internal Phase 3 step) — Production-runtime proof without frontend work

- [x] Ensure the same grid strategy and Backtest engine build and deploy on the existing Render service.
- [x] Do not claim a production-XAGUSDT execution merely from successful deployment.
- [x] Confirmed that Phase 3 has no safe backend-only production run trigger/command, so no Render-resident XAGUSDT execution is claimed.
- [x] Recorded that limitation explicitly and left authenticated production run invocation/inspection to the master Phase 3.5 UI checkpoint.
- [x] Verify no Paper or real-order path is enabled.

**Exit:** The Phase 3 code is production-deployable, and the report clearly distinguishes the proven full real-data GitHub run from any Render production-data run that was or was not actually executed.

## Phase 3 completion condition

Phase 3 is complete only when:

1. the full real XAGUSDT 1-minute regression SQLite fixture is committed and checksum-verified;
2. ordinary CI reuses that committed database without downloading it again;
3. the simple mode-neutral grid runs through the real Backtest engine against that full fixture;
4. controlled timing tests prove resting-order same-candle fills are allowed while retroactive same-candle fills are prohibited;
5. the full real-data regression is deterministic and persisted through the canonical Phase 1 model; and
6. the repository remains backend-only for this work.

After this point, stop before Paper trading. **Phase 3.5 is the user-facing Backtest Strategy UI checkpoint** where the user can configure, launch, inspect, and manually validate these backend runs before Phase 4 Live Paper begins.

## Completion evidence

- Frozen fixture: `test-data/xagusdt_1m_2026.sqlite3`, 48,140,288 bytes, 369,480 candles.
- Fixture SHA-256: `37afda792f5f06cee9e63eca010f6c5b850d2282fe5ecc5a686b213f832c4a7e`.
- Full regression active candles: 369,479 after one pre-roll candle.
- Locked semantic SHA-256: `e7c2fff2605a4642df7ea1ab4f26ef3ca5896fbc49c63527b28aba7e6a1f6e93`.
- Baseline counts: 1 decision, 6 intents, 6 orders, 6 fills, 6 position snapshots, 369,479 equity snapshots.
- The full regression is executed twice and compared semantically; unique run IDs and persistence timestamps are intentionally excluded from the deterministic checksum.
- The committed fixture is verified byte-for-byte unchanged after CI execution.
- Render successfully builds/deploys the Phase 3 backend code, but Phase 3 does not claim a production SQLite strategy run because no run-trigger surface exists before the master Phase 3.5 UI checkpoint.
