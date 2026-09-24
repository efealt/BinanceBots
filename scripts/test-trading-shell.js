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
  'id="trading-bot-strip"',
  'id="trading-new-bot-button"',
  'id="trading-bot-status"',
  'id="trading-run-id"',
  'id="trading-run-status"',
  'id="trading-feed-label"',
  'id="trading-connection-label"',
  'id="trading-live-chart"',
  'id="trading-preview-badge"',
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
  'data-mode-panel="paper"',
  'data-mode-panel="live"',
]) {
  if (!trading.includes(required)) {
    throw new Error(`Trading shell is missing ${required}`);
  }
}
if (!/id="trading-mode-live"[^>]*disabled/.test(trading)) {
  throw new Error("Live mode must remain visibly disabled in Phase 4");
}
if (!trading.includes("Live-Real-Account execution is locked server-side.")) {
  throw new Error("Trading shell must use explicit Live-Real-Account lock terminology");
}
if (!trading.includes(">LIVE-PAPER</button>") || !trading.includes(">LIVE-REAL-ACCOUNT <span>Locked</span></button>")) {
  throw new Error("Trading mode labels must use explicit Live-Paper / Live-Real-Account terminology");
}

console.log("Trading shell contract OK");

for (const required of [
  'id="trading-paper-form"',
  'id="trading-bot-name"',
  'id="trading-save-bot-button"',
  'id="trading-preview-button"',
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

if (!trading.includes("https://unpkg.com/lightweight-charts@5.2.0/dist/lightweight-charts.standalone.production.js")) {
  throw new Error("Trading chart must load the same pinned Lightweight Charts runtime as Market");
}
if (!trading.includes('/market-chart.js?v=5')) {
  throw new Error("Trading page must reuse the MarketChart wrapper");
}
if (!trading.includes('/trading-contract.js?v=2')) {
  throw new Error("Trading page must load the Phase 4.8B Bot-aware shared mode contract before page behavior");
}
if (!trading.includes('data-trading-mode="paper"')) {
  throw new Error("Trading page must expose its active mode for unmistakable Paper/Live styling");
}
if (!trading.includes('/trading.js?v=14')) {
  throw new Error("Trading page must load the Phase 4.8B preview-capable Bot workspace client");
}
for (const id of [
  "trading-chart-last-price",
  "trading-chart-open",
  "trading-chart-high",
  "trading-chart-low",
  "trading-chart-close",
  "trading-chart-time",
]) {
  if (!trading.includes('id="' + id + '"')) throw new Error("Trading market readout is missing #" + id);
}
if (trading.includes('value="BTCUSDT"')) {
  throw new Error("Trading symbol must come from the registered database catalog, not a hardcoded default");
}

const tradingJs = fs.readFileSync("web/trading.js", "utf8");
for (const required of [
  'TradingContract.normalizeSnapshot(snapshot)',
  'activeAdapter.urls.start()',
  'activeAdapter.urls.stop(currentRunId)',
  'activeAdapter.urls.chart(runId)',
  'activeAdapter.urls.auditTail(runId, 250)',
  'activeAdapter.urls.auditAfter(runId, afterSequence, 500)',
  'activeAdapter.urls.stream(runId)',
  'activeAdapter.buildStartPayload({',
  'bot_id: selectedBotId',
  'fetchJson("/api/trading/bots")',
  'async function selectBot(botId)',
  'function openNewBotDraft()',
  'async function persistCurrentBotFromForm()',
  'previewButton.addEventListener("click"',
  'requestJson("/api/trading/preview"',
  'function refreshPreviewStaleness()',
  'Preview · not active',
  'Update preview',
  'async function saveCurrentBot()',
  'Create & Start Live-Paper',
  'Save & Start Live-Paper',
  'Unsaved changes',
  'Running · Live-Paper',
  'setConfigLocked(Boolean(monitor.run.runtimeActive))',
  'applySnapshotToConfig(snapshot)',
  'syncChartFromMonitor(monitor)',
  'renderPortfolio(monitor)',
  'renderOpenOrders(monitor)',
  'syncAuditFromMonitor(monitor)',
  'new MarketChart(tradingChartElement, { showWeekends: false, indicators: false })',
  'LightweightCharts.createSeriesMarkers',
  'LightweightCharts.LineSeries',
  'LightweightCharts.LineStyle.Dashed',
  'order.active_to_ms == null ? null : numeric(order.active_to_ms)',
  'chart.series.createPriceLine',
  'subscribeCrosshairMove',
  'chart.updateCandle(toMarketChartCandle(normalized))',
  'loadCompleteAudit(currentRunId)',
  'renderNoRun();',
  'fetch("/api/data/downloads"',
  'registeredMarketsBySymbol',
  'populateMarketOptions(symbolInput.value',
  '"/api/market/stream?"',
  'syncMarketStreamToSelection(true)',
  'resetRunChartState("Live market remains available with no active Live-Paper run.")',
  'candle.open_time_ms ?? candle.open_time',
  'candle.close_time_ms ?? candle.close_time',
]) {
  if (!tradingJs.includes(required)) throw new Error("Trading control contract is missing: " + required);
}

const tradingCss = fs.readFileSync("web/styles.css", "utf8");
for (const required of [
  ".trading-page { display: grid; min-width: 0;",
  "grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr) minmax(0, 0.8fr);",
  ".trading-control-panel { min-width: 0;",
]) {
  if (!tradingCss.includes(required)) throw new Error("Trading responsive layout contract is missing: " + required);
}

const tradingContract = fs.readFileSync("web/trading-contract.js", "utf8");
for (const required of [
  'mode: "paper"',
  'executionKind: "simulated"',
  'mode: "live"',
  'executionKind: "binance_private"',
  'locked: true',
  'function normalizeSnapshot(snapshot)',
  'function adapterFor(mode)',
]) {
  if (!tradingContract.includes(required)) throw new Error("Shared Trading mode contract is missing: " + required);
}

const tradingApi = fs.readFileSync("src/api/trading.rs", "utf8");
if (!tradingApi.includes('.route("/api/trading/bots", post(create_bot).get(list_bots))')) {
  throw new Error("Trading API is missing the Bot create/list route");
}
if (!tradingApi.includes('.route("/api/trading/bots/{bot_id}", get(bot_snapshot).put(update_bot))')) {
  throw new Error("Trading API is missing the Bot read/update route");
}
if (!tradingApi.includes('.route("/api/trading/bots/{bot_id}/runs", get(bot_history))')) {
  throw new Error("Trading API is missing the Phase 5A Bot history route");
}
if (!tradingApi.includes('.route("/api/trading/runs/{run_id}/activity", get(run_activity))')) {
  throw new Error("Trading API is missing the Phase 5A Run activity route");
}
if (!tradingApi.includes('.route("/api/trading/preview", post(preview_run))')) {
  throw new Error("Trading API is missing the side-effect-free preview route");
}
if (!tradingApi.includes("bot_id: i64")) {
  throw new Error("Live-Paper start must require an explicit persisted bot_id");
}
if (!tradingApi.includes('.route("/api/trading/runs/{run_id}/chart", get(run_chart))')) {
  throw new Error("Trading API is missing the authenticated chart bootstrap route");
}
if (!tradingApi.includes('.route("/api/trading/runs/{run_id}/audit", get(run_audit))')) {
  throw new Error("Trading API is missing the authenticated canonical audit route");
}
const paper = fs.readFileSync("src/paper.rs", "utf8");
if (!paper.includes("BotAlreadyActive") || !paper.includes("active_runtime_admission")) {
  throw new Error("Live-Paper manager must enforce one active Run per Bot");
}
for (const required of [
  "pub async fn preview(&self, config: PaperPreviewConfig)",
  "build_preview_snapshot(",
  "preview_grid_matches_live_paper_initial_orders_for_same_boundary",
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
  "pub fn trading_bot_run_summaries",
  "pub fn trading_run_activity_page",
  "event_kind <> 'equity'",
  "ORDER BY e.run_sequence DESC",
  "pub fn trading_run_audit_page",
  "WITH selected AS",
  "LIMIT ?3",
  "ORDER BY e.run_sequence",
]) {
  if (!storageRead.includes(required)) throw new Error("Canonical audit pagination contract is missing: " + required);
}

console.log("Trading shared Live-Paper / Live-Real-Account UI contract OK");

const marketChartJs = fs.readFileSync("web/market-chart.js", "utf8");
for (const required of [
  'class MarketChart',
  'LightweightCharts.createChart',
  'LightweightCharts.CandlestickSeries',
  'CrosshairMode.Normal',
  'pressedMouseMove: true',
  'mouseWheel: true',
  'marketChartContainer',
  'indicators = true',
]) {
  if (!marketChartJs.includes(required)) throw new Error("Reusable MarketChart contract is missing: " + required);
}
