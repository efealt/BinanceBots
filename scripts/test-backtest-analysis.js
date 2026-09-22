const assert = require("node:assert/strict");
const math = require("../web/backtest-analysis-math.js");

const candles = [
  { open_time_ms: 0, close_time_ms: 59_999, open_price: 100, close_price: 100 },
  { open_time_ms: 60_000, close_time_ms: 119_999, open_price: 100, close_price: 110 },
  { open_time_ms: 120_000, close_time_ms: 179_999, open_price: 110, close_price: 90 },
];

const benchmark = math.buildBuyAndHold(candles, 1_000);
assert.equal(benchmark.entryPrice, 100);
assert.equal(benchmark.quantity, 10);
assert.deepEqual(benchmark.equity.map((point) => point[1]), [1_000, 1_100, 900]);
assert.ok(Math.abs(benchmark.totalReturnPercent - (-10)) < 1e-12);

const benchmarkDrawdown = math.drawdownSeries(benchmark.equity);
assert.equal(benchmarkDrawdown.points[0][1], 0);
assert.equal(benchmarkDrawdown.points[1][1], 0);
assert.ok(Math.abs(benchmarkDrawdown.maxDrawdownPercent - (-18.181818181818176)) < 1e-9);

const flatStrategy = [[59_999, 1_000], [119_999, 1_000], [179_999, 1_000]];
const flatDrawdown = math.drawdownSeries(flatStrategy);
assert.deepEqual(flatStrategy.map((point) => point[1]), [1_000, 1_000, 1_000]);
assert.equal(flatDrawdown.maxDrawdownPercent, 0);
assert.notDeepEqual(flatStrategy.map((point) => point[1]), benchmark.equity.map((point) => point[1]));

const zeroExposure = math.buildPositionExposure(candles, flatStrategy, []);
assert.deepEqual(zeroExposure.position.map((point) => point[1]), [0, 0, 0]);
assert.deepEqual(zeroExposure.exposure.map((point) => point[1]), [0, 0, 0]);

const withFill = math.buildPositionExposure(candles, flatStrategy, [
  { event_time_ms: 119_999, position_quantity: "2" },
]);
assert.deepEqual(withFill.position.map((point) => point[1]), [0, 2, 2]);
assert.ok(withFill.exposure[1][1] > 0);

console.log("Phase 3.6 analysis math tests passed");
