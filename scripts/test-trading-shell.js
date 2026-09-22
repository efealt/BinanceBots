const fs = require("fs");

const pages = [
  "web/index.html",
  "web/market.html",
  "web/data-downloader.html",
  "web/backtests.html",
  "web/security.html",
  "web/trading.html",
];

for (const path of pages) {
  const html = fs.readFileSync(path, "utf8");
  if (!html.includes('href="/trading.html"')) {
    throw new Error(`${path} is missing the Trading navigation entry`);
  }
}

const trading = fs.readFileSync("web/trading.html", "utf8");
for (const required of [
  'id="trading-mode-paper"',
  'id="trading-mode-live"',
  'id="trading-run-id"',
  'id="trading-run-status"',
  'id="trading-feed-label"',
  'id="trading-connection-label"',
  'id="trading-live-chart"',
  'id="trading-chart-status"',
]) {
  if (!trading.includes(required)) {
    throw new Error(`Trading shell is missing ${required}`);
  }
}
if (!/id="trading-mode-live"[^>]*disabled/.test(trading)) {
  throw new Error("Live mode must remain visibly disabled in Phase 4");
}
if (!trading.includes("Live execution is locked server-side until Phase 8.")) {
  throw new Error("Trading shell must explain the Phase 4 Live lock");
}

console.log("Trading shell contract OK");

for (const required of [
  'id="trading-paper-form"',
  'id="trading-config-symbol"',
  'id="trading-config-market-type"',
  'id="trading-config-interval"',
  'id="trading-config-capital"',
  'id="trading-config-strategy"',
  'id="trading-config-anchor"',
  'id="trading-config-spacing"',
  'id="trading-config-levels"',
  'id="trading-config-quantity"',
  'id="trading-config-fee"',
  'id="trading-config-spread"',
  'id="trading-config-slippage"',
  'id="trading-config-latency"',
  'id="trading-config-limit-policy"',
  'id="trading-config-partial-fill"',
  'id="trading-start-button"',
  'id="trading-stop-button"',
]) {
  if (!trading.includes(required)) throw new Error("Trading controls are missing " + required);
}

if (!trading.includes("https://cdn.jsdelivr.net/npm/echarts@6.0.0/dist/echarts.min.js")) {
  throw new Error("Trading chart must load the pinned ECharts runtime");
}

const tradingJs = fs.readFileSync("web/trading.js", "utf8");
for (const required of [
  'requestJson("/api/trading/runs"',
  '"/api/trading/runs/" + currentRunId + "/stop"',
  'setConfigLocked(Boolean(snapshot.runtime_active))',
  'paperModeButton.disabled = locked',
  'applySnapshotToConfig(snapshot)',
  '"/api/trading/runs/" + runId + "/chart"',
  'type: "candlestick"',
  'type: "custom"',
  'name: "Buy fills"',
  'name: "Sell fills"',
]) {
  if (!tradingJs.includes(required)) throw new Error("Trading control contract is missing: " + required);
}

const tradingApi = fs.readFileSync("src/api/trading.rs", "utf8");
if (!tradingApi.includes('.route("/api/trading/runs/{run_id}/chart", get(run_chart))')) {
  throw new Error("Trading API is missing the authenticated chart bootstrap route");
}
const paper = fs.readFileSync("src/paper.rs", "utf8");
for (const required of [
  "trading_run_order_levels(run_id)",
  "trading_run_fill_audit(run_id)",
  'MarketKey::new(&snapshot.symbol, "1m", market_type)',
]) {
  if (!paper.includes(required)) throw new Error("Paper chart contract is missing: " + required);
}

console.log("Trading control + live chart contract OK");
