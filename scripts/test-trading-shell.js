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
  'id="trading-portfolio-position"',
  'id="trading-portfolio-cash"',
  'id="trading-portfolio-equity"',
  'id="trading-portfolio-exposure"',
  'id="trading-portfolio-realized"',
  'id="trading-portfolio-unrealized"',
  'id="trading-portfolio-fees"',
  'id="trading-orders-body"',
  'id="trading-audit-body"',
  'id="trading-audit-load-all"',
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
  'renderPortfolio(snapshot)',
  'renderOpenOrders(snapshot)',
  'syncAuditFromSnapshot(snapshot)',
  '"/api/trading/runs/" + runId + "/audit?limit=250"',
  '"/api/trading/runs/" + runId + "/audit?after_sequence="',
  'loadCompleteAudit(currentRunId)',
]) {
  if (!tradingJs.includes(required)) throw new Error("Trading control contract is missing: " + required);
}

const tradingApi = fs.readFileSync("src/api/trading.rs", "utf8");
if (!tradingApi.includes('.route("/api/trading/runs/{run_id}/chart", get(run_chart))')) {
  throw new Error("Trading API is missing the authenticated chart bootstrap route");
}
if (!tradingApi.includes('.route("/api/trading/runs/{run_id}/audit", get(run_audit))')) {
  throw new Error("Trading API is missing the authenticated canonical audit route");
}
const paper = fs.readFileSync("src/paper.rs", "utf8");
for (const required of [
  "trading_run_order_levels(run_id)",
  "trading_run_fill_audit(run_id)",
  'MarketKey::new(&snapshot.symbol, "1m", market_type)',
  'trading_run_audit_page(run_id, after_sequence, limit)',
  'pub mark_price: Option<f64>',
]) {
  if (!paper.includes(required)) throw new Error("Paper chart contract is missing: " + required);
}

const storageRead = fs.readFileSync("src/storage/runs/read.rs", "utf8");
for (const required of [
  "pub fn trading_run_audit_page",
  "WITH selected AS",
  "LIMIT ?3",
  "ORDER BY e.run_sequence",
]) {
  if (!storageRead.includes(required)) throw new Error("Canonical audit pagination contract is missing: " + required);
}

console.log("Trading control + chart + operations/audit contract OK");
