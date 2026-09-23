const assert = require("assert");
const contract = require("../web/trading-contract.js");

const paper = contract.adapterFor("paper");
assert.equal(paper.locked, false);
assert.equal(paper.executionKind, "simulated");
assert.equal(paper.urls.runs(25), "/api/trading/runs?limit=25");
assert.equal(paper.urls.snapshot(7), "/api/trading/runs/7");
assert.equal(paper.urls.chart(7), "/api/trading/runs/7/chart");
assert.equal(paper.urls.auditTail(7, 250), "/api/trading/runs/7/audit?limit=250");
assert.equal(paper.urls.auditAfter(7, 500, 100), "/api/trading/runs/7/audit?after_sequence=500&limit=100");
assert.equal(paper.urls.stream(7), "/api/trading/runs/7/stream");
assert.equal(paper.urls.start(), "/api/trading/runs");
assert.equal(paper.urls.stop(7), "/api/trading/runs/7/stop");
assert.deepEqual(paper.buildStartPayload({ symbol: "BTCUSDT" }), { mode: "paper", symbol: "BTCUSDT" });

const live = contract.adapterFor("live");
assert.equal(live.locked, true);
assert.equal(live.executionKind, "binance_private");
assert.equal(live.urls.start(), null);
assert.throws(() => live.buildStartPayload({}), /locked server-side until Phase 8/);

const rawPaper = {
  run_id: 10,
  mode: "paper",
  runtime_status: "running",
  canonical_status: "running",
  runtime_active: true,
  symbol: "BTCUSDT",
  market_type: "spot",
  replay_interval: "1m",
  strategy_id: "static-grid-fixture",
  strategy_version: "1",
  strategy_params: { spacing_bps: 100 },
  execution_assumptions: { fee_bps: 4 },
  mark_price: 100,
  portfolio: {
    cash: 900,
    position_quantity: 2,
    average_entry_price: 95,
    realized_pnl: 5,
    unrealized_pnl: 10,
    fees_paid: 1,
    equity: 1000,
  },
  open_orders: [{
    order_id: 1,
    side: "buy",
    order_type: "limit",
    price: 90,
    original_quantity: 3,
    filled_quantity: 1,
    remaining_quantity: 2,
    status: "partially_filled",
    submitted_at_ms: 1234,
  }],
  recent_fills: [{
    event_time_ms: 1250,
    order_id: 1,
    side: "buy",
    order_type: "limit",
    price: 90,
    quantity: 1,
    fee: 0.1,
    status: "filled",
  }],
  recent_events: [{ kind: "fill" }],
};

const paperModel = contract.normalizeSnapshot(rawPaper);
assert.equal(paperModel.run.id, 10);
assert.equal(paperModel.run.mode, "paper");
assert.equal(paperModel.market.symbol, "BTCUSDT");
assert.equal(paperModel.strategy.id, "static-grid-fixture");
assert.equal(paperModel.execution.kind, "simulated");
assert.equal(paperModel.portfolio.grossExposurePercent, 20);
assert.equal(paperModel.orders[0].remainingQuantity, 2);
assert.equal(paperModel.fills[0].orderId, 1);
assert.equal(paperModel.events.length, 1);

const rawLive = {
  ...rawPaper,
  run_id: 11,
  mode: "live",
  execution_assumptions: {},
};
const liveModel = contract.normalizeSnapshot(rawLive);
assert.equal(liveModel.run.mode, "live");
assert.equal(liveModel.execution.kind, "binance_private");
assert.equal(liveModel.adapter.locked, true);
assert.deepEqual(Object.keys(liveModel).sort(), Object.keys(paperModel).sort());
assert.deepEqual(Object.keys(liveModel.run).sort(), Object.keys(paperModel.run).sort());
assert.deepEqual(Object.keys(liveModel.market).sort(), Object.keys(paperModel.market).sort());
assert.deepEqual(Object.keys(liveModel.portfolio).sort(), Object.keys(paperModel.portfolio).sort());

assert.equal(
  contract.selectRun([
    { run_id: 1, runtime_status: "stopped" },
    { run_id: 2, runtime_status: "running" },
  ]).run_id,
  2
);

console.log("Trading shared mode contract OK");
