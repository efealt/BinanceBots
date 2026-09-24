const TradingContract = window.BinanceGridTradingContract;
if (!TradingContract) throw new Error("Shared Trading contract failed to load");
const themeToggle = document.querySelector("#theme-toggle");
const runIdElement = document.querySelector("#trading-run-id");
const runStatusElement = document.querySelector("#trading-run-status");
const runBadgeElement = document.querySelector("#trading-run-badge");
const feedDot = document.querySelector("#trading-feed-dot");
const feedLabel = document.querySelector("#trading-feed-label");
const feedDetail = document.querySelector("#trading-feed-detail");
const connectionDot = document.querySelector("#trading-connection-dot");
const connectionLabel = document.querySelector("#trading-connection-label");
const connectionDetail = document.querySelector("#trading-connection-detail");
const topDot = document.querySelector("#trading-top-dot");
const topLabel = document.querySelector("#trading-top-label");
const symbolElement = document.querySelector("#trading-symbol");
const marketTypeElement = document.querySelector("#trading-market-type");
const intervalElement = document.querySelector("#trading-interval");
const strategyElement = document.querySelector("#trading-strategy");
const paperModeButton = document.querySelector("#trading-mode-paper");
const liveModeButton = document.querySelector("#trading-mode-live");
const modePanels = Array.from(document.querySelectorAll("[data-mode-panel]"));
const tradingForm = document.querySelector("#trading-paper-form");
const configLockBadge = document.querySelector("#trading-config-lock-badge");
const startButton = document.querySelector("#trading-start-button");
const stopButton = document.querySelector("#trading-stop-button");
const controlError = document.querySelector("#trading-control-error");
const controlStatus = document.querySelector("#trading-control-status");
const botStrip = document.querySelector("#trading-bot-strip");
const botStatus = document.querySelector("#trading-bot-status");
const newBotButton = document.querySelector("#trading-new-bot-button");
const botNameInput = document.querySelector("#trading-bot-name");
const saveBotButton = document.querySelector("#trading-save-bot-button");
const symbolInput = document.querySelector("#trading-config-symbol");
const marketTypeInput = document.querySelector("#trading-config-market-type");
const replayIntervalInput = document.querySelector("#trading-config-interval");
const capitalInput = document.querySelector("#trading-config-capital");
const strategyInput = document.querySelector("#trading-config-strategy");
const anchorInput = document.querySelector("#trading-config-anchor");
const fixedAnchorInput = document.querySelector("#trading-config-fixed-anchor");
const spacingInput = document.querySelector("#trading-config-spacing");
const levelsInput = document.querySelector("#trading-config-levels");
const quantityInput = document.querySelector("#trading-config-quantity");
const feeInput = document.querySelector("#trading-config-fee");
const spreadInput = document.querySelector("#trading-config-spread");
const slippageInput = document.querySelector("#trading-config-slippage");
const latencyInput = document.querySelector("#trading-config-latency");
const limitPolicyInput = document.querySelector("#trading-config-limit-policy");
const partialFillInput = document.querySelector("#trading-config-partial-fill");
const tradingChartElement = document.querySelector("#trading-live-chart");
const tradingChartStatus = document.querySelector("#trading-chart-status");
const tradingChartBadge = document.querySelector("#trading-chart-badge");
const chartLastPrice = document.querySelector("#trading-chart-last-price");
const chartOpen = document.querySelector("#trading-chart-open");
const chartHigh = document.querySelector("#trading-chart-high");
const chartLow = document.querySelector("#trading-chart-low");
const chartClose = document.querySelector("#trading-chart-close");
const chartTime = document.querySelector("#trading-chart-time");
const portfolioBadge = document.querySelector("#trading-portfolio-badge");
const portfolioPosition = document.querySelector("#trading-portfolio-position");
const portfolioCash = document.querySelector("#trading-portfolio-cash");
const portfolioEquity = document.querySelector("#trading-portfolio-equity");
const portfolioExposure = document.querySelector("#trading-portfolio-exposure");
const portfolioRealized = document.querySelector("#trading-portfolio-realized");
const portfolioUnrealized = document.querySelector("#trading-portfolio-unrealized");
const portfolioFees = document.querySelector("#trading-portfolio-fees");
const portfolioMark = document.querySelector("#trading-portfolio-mark");
const ordersBadge = document.querySelector("#trading-orders-badge");
const ordersBody = document.querySelector("#trading-orders-body");
const ordersStatus = document.querySelector("#trading-orders-status");
const auditBody = document.querySelector("#trading-audit-body");
const auditStatus = document.querySelector("#trading-audit-status");
const auditLoadAllButton = document.querySelector("#trading-audit-load-all");

let stream = null;
let reconnectTimer = null;
let marketStream = null;
let marketReconnectTimer = null;
let marketConnectionToken = 0;
let marketStreamKey = null;
let chartFillMarkersPlugin = null;
let chartOrderSeries = new Map();
let chartOrderPriceLines = new Map();
let tradingChartCrosshairBound = false;
let currentRunId = null;
let currentSnapshot = null;
let currentMonitor = null;
let activeMode = "paper";
let activeAdapter = TradingContract.adapterFor(activeMode);
let tradingChart = null;
let chartRunId = null;
let chartCandles = [];
let chartOrderLevels = new Map();
let chartFillMarkers = new Map();
let chartBootstrapToken = 0;
let chartRefreshInFlight = false;
let auditRunId = null;
let auditEvents = [];
let auditTotalEvents = 0;
let auditHasEarlier = false;
let auditHasMore = false;
let auditFullMode = false;
let auditRequestToken = 0;
let auditRefreshInFlight = false;
let auditLastRefreshAt = 0;
let registeredMarketsBySymbol = new Map();
let instrumentCatalogReady = false;
let instrumentCatalogError = null;
let tradingBots = [];
let tradingRunSummaries = [];
let selectedBotId = null;
let selectedBot = null;
let selectedBotBaseline = null;
let botDraftMode = true;
let botSelectionToken = 0;

function marketTypeLabel(marketType) {
  return marketType === "spot" ? "Spot" : marketType === "usd_m_perpetual" ? "USD-M perpetual" : humanize(marketType);
}

function hasRegisteredInstrument(symbol, marketType) {
  return registeredMarketsBySymbol.get(symbol)?.has(marketType) ?? false;
}

function populateMarketOptions(symbol, preferredMarket = null) {
  const markets = Array.from(registeredMarketsBySymbol.get(symbol) ?? []);
  markets.sort((a, b) => {
    const rank = { spot: 0, usd_m_perpetual: 1 };
    return (rank[a] ?? 99) - (rank[b] ?? 99) || a.localeCompare(b);
  });
  marketTypeInput.replaceChildren();
  for (const marketType of markets) {
    const option = document.createElement("option");
    option.value = marketType;
    option.textContent = marketTypeLabel(marketType);
    marketTypeInput.appendChild(option);
  }
  if (preferredMarket && markets.includes(preferredMarket)) {
    marketTypeInput.value = preferredMarket;
  } else if (markets.length) {
    marketTypeInput.value = markets[0];
  }
}

function selectRegisteredInstrument(symbol, marketType = null) {
  if (!registeredMarketsBySymbol.has(symbol)) return false;
  symbolInput.value = symbol;
  populateMarketOptions(symbol, marketType);
  return marketType ? hasRegisteredInstrument(symbol, marketType) : true;
}

async function loadRegisteredInstruments() {
  instrumentCatalogReady = false;
  instrumentCatalogError = null;
  symbolInput.replaceChildren(new Option("Loading registered symbols…", ""));
  marketTypeInput.replaceChildren(new Option("Loading registered markets…", ""));
  symbolInput.disabled = true;
  marketTypeInput.disabled = true;
  startButton.disabled = true;

  try {
    const response = await fetch("/api/data/downloads", {
      credentials: "same-origin",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) throw new Error(response.status + " " + response.statusText);
    const payload = await response.json();
    const next = new Map();

    for (const entry of payload.downloads ?? []) {
      if (String(entry.provider ?? "").toLowerCase() !== "binance") continue;
      const symbol = String(entry.symbol ?? "").trim().toUpperCase();
      const marketType = String(entry.market_type ?? "").trim().toLowerCase();
      if (!symbol || !["spot", "usd_m_perpetual"].includes(marketType)) continue;
      if (!next.has(symbol)) next.set(symbol, new Set());
      next.get(symbol).add(marketType);
    }

    registeredMarketsBySymbol = new Map(
      Array.from(next.entries()).sort(([left], [right]) => left.localeCompare(right))
    );
    instrumentCatalogReady = true;
    symbolInput.replaceChildren();

    for (const symbol of registeredMarketsBySymbol.keys()) {
      const option = document.createElement("option");
      option.value = symbol;
      option.textContent = symbol;
      symbolInput.appendChild(option);
    }

    if (registeredMarketsBySymbol.size === 0) {
      symbolInput.appendChild(new Option("No registered Binance symbols", ""));
      marketTypeInput.replaceChildren(new Option("No registered markets", ""));
      instrumentCatalogError = "No registered Binance symbols are available. Add one in Data Downloader first.";
      showControlError(instrumentCatalogError);
    } else {
      populateMarketOptions(symbolInput.value);
    }
  } catch (error) {
    instrumentCatalogReady = true;
    registeredMarketsBySymbol = new Map();
    instrumentCatalogError = "Could not load registered symbols from the database · " + error.message;
    symbolInput.replaceChildren(new Option("Registered symbols unavailable", ""));
    marketTypeInput.replaceChildren(new Option("Registered markets unavailable", ""));
    showControlError(instrumentCatalogError);
  }

  setConfigLocked(Boolean(currentMonitor?.run.runtimeActive));
}

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("theme", theme);
}

function initializeTheme() {
  const savedTheme = localStorage.getItem("theme");
  const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  setTheme(savedTheme ?? systemTheme);
}

function setDot(element, state) {
  element.classList.remove("live", "loading", "reconnecting", "pending");
  element.classList.add(state);
}

function humanize(value) {
  return String(value ?? "")
    .replaceAll("_", " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

function runSummaryIsActive(run) {
  return ["arming", "running"].includes(String(run?.runtime_status ?? "").toLowerCase());
}

function activeRunForBot(botId) {
  return tradingRunSummaries.find((run) => Number(run.bot_id) === Number(botId) && runSummaryIsActive(run)) ?? null;
}

function anyActiveLivePaperRun() {
  return tradingRunSummaries.some(runSummaryIsActive);
}

function botFormFingerprint() {
  return JSON.stringify({
    bot_name: botNameInput.value.trim(),
    symbol: symbolInput.value,
    market_type: marketTypeInput.value,
    replay_interval: replayIntervalInput.value,
    initial_capital: capitalInput.value.trim(),
    strategy_id: strategyInput.value,
    anchor: anchorInput.value,
    fixed_anchor_price: fixedAnchorInput.value,
    spacing_bps: spacingInput.value,
    levels_per_side: levelsInput.value,
    quantity_per_order: quantityInput.value,
    fee_bps: feeInput.value,
    spread_bps: spreadInput.value,
    slippage_bps: slippageInput.value,
    latency_ms: latencyInput.value,
    limit_fill_policy: limitPolicyInput.value,
    partial_fill_ratio: partialFillInput.value,
  });
}

function hasUnsavedBotChanges() {
  if (botDraftMode || !selectedBot) return true;
  return selectedBotBaseline !== botFormFingerprint();
}

function sortedBotsForStrip() {
  return [...tradingBots].sort((left, right) => {
    const leftActive = activeRunForBot(left.bot_id) ? 1 : 0;
    const rightActive = activeRunForBot(right.bot_id) ? 1 : 0;
    if (leftActive !== rightActive) return rightActive - leftActive;
    return Number(right.updated_at_ms ?? 0) - Number(left.updated_at_ms ?? 0)
      || Number(right.bot_id) - Number(left.bot_id);
  });
}

function renderBotStrip() {
  botStrip.replaceChildren();
  const bots = sortedBotsForStrip();
  if (!bots.length) {
    const empty = document.createElement("div");
    empty.id = "trading-bot-empty";
    empty.className = "trading-bot-empty";
    empty.textContent = "No bots created";
    botStrip.appendChild(empty);
    return;
  }

  for (const bot of bots) {
    const activeRun = activeRunForBot(bot.bot_id);
    const card = document.createElement("button");
    card.type = "button";
    card.className = "trading-bot-card";
    card.dataset.botId = String(bot.bot_id);
    card.classList.toggle("is-selected", Number(bot.bot_id) === Number(selectedBotId));
    card.classList.toggle("is-running", Boolean(activeRun));
    card.setAttribute("aria-pressed", String(Number(bot.bot_id) === Number(selectedBotId)));

    const top = document.createElement("span");
    top.className = "trading-bot-card-top";
    const name = document.createElement("strong");
    name.textContent = bot.bot_name;
    const state = document.createElement("span");
    state.className = "trading-bot-card-state";
    state.textContent = activeRun ? "Running · Live-Paper" : "Idle";
    top.append(name, state);

    const meta = document.createElement("span");
    meta.className = "trading-bot-card-meta";
    meta.textContent = "Bot #" + bot.bot_id + (activeRun ? " · Run #" + activeRun.run_id : "");

    card.append(top, meta);
    card.addEventListener("click", () => void selectBot(bot.bot_id));
    botStrip.appendChild(card);
  }
}

function setBotStatus(message) {
  botStatus.textContent = message;
}

function renderSelectedBotContext() {
  symbolElement.textContent = symbolInput.value || "—";
  marketTypeElement.textContent = marketTypeInput.value ? marketTypeLabel(marketTypeInput.value) : "—";
  intervalElement.textContent = replayIntervalInput.value || "—";
  strategyElement.textContent = strategyInput.value || "—";
}

function resetBotFormToDefaults() {
  botNameInput.value = "";
  const firstSymbol = registeredMarketsBySymbol.keys().next().value ?? "";
  if (firstSymbol) {
    selectRegisteredInstrument(firstSymbol);
  }
  replayIntervalInput.value = "1m";
  capitalInput.value = "100000";
  strategyInput.value = "static-grid-fixture";
  anchorInput.value = "previous_close";
  fixedAnchorInput.value = "";
  spacingInput.value = "100";
  levelsInput.value = "3";
  quantityInput.value = "1";
  feeInput.value = "4";
  spreadInput.value = "0";
  slippageInput.value = "0";
  latencyInput.value = "0";
  limitPolicyInput.value = "touch";
  partialFillInput.value = "1";
  syncFixedAnchorState();
}

function applyBotConfiguration(bot) {
  const config = bot?.config ?? {};
  botNameInput.value = bot?.bot_name ?? "";
  if (config.symbol && hasRegisteredInstrument(config.symbol, config.market_type)) {
    selectRegisteredInstrument(config.symbol, config.market_type);
  }
  replayIntervalInput.value = config.replay_interval || replayIntervalInput.value;
  capitalInput.value = config.initial_capital ?? capitalInput.value;
  strategyInput.value = config.strategy_id || strategyInput.value;
  const grid = config.grid ?? {};
  anchorInput.value = grid.anchor || anchorInput.value;
  fixedAnchorInput.value = grid.fixed_anchor_price ?? "";
  spacingInput.value = grid.spacing_bps ?? spacingInput.value;
  levelsInput.value = grid.levels_per_side ?? levelsInput.value;
  quantityInput.value = grid.quantity_per_order ?? quantityInput.value;
  const execution = config.execution ?? {};
  feeInput.value = execution.fee_bps ?? feeInput.value;
  spreadInput.value = execution.spread_bps ?? spreadInput.value;
  slippageInput.value = execution.slippage_bps ?? slippageInput.value;
  latencyInput.value = execution.latency_ms ?? latencyInput.value;
  limitPolicyInput.value = execution.limit_fill_policy || limitPolicyInput.value;
  partialFillInput.value = execution.partial_fill_ratio ?? partialFillInput.value;
  syncFixedAnchorState();
}

function setTradingMode(mode) {
  const adapter = TradingContract.adapterFor(mode);
  activeMode = adapter.mode;
  activeAdapter = adapter;
  document.body.dataset.tradingMode = adapter.theme;
  paperModeButton.classList.toggle("is-active", adapter.mode === "paper");
  paperModeButton.setAttribute("aria-pressed", String(adapter.mode === "paper"));
  liveModeButton.classList.toggle("is-active", adapter.mode === "live");
  liveModeButton.setAttribute("aria-pressed", String(adapter.mode === "live"));
  liveModeButton.disabled = TradingContract.adapterFor("live").locked || Boolean(currentMonitor?.run.runtimeActive);
  for (const panel of modePanels) {
    panel.hidden = panel.dataset.modePanel !== adapter.mode;
  }
}

function setConnection(state, detail) {
  const map = {
    connected: ["live", "Connected"],
    streaming: ["live", "Streaming"],
    connecting: ["loading", "Connecting"],
    reconnecting: ["reconnecting", "Reconnecting"],
    error: ["reconnecting", "Unavailable"],
  };
  const [dotState, label] = map[state] ?? ["pending", "Unknown"];
  setDot(connectionDot, dotState);
  connectionLabel.textContent = label;
  connectionDetail.textContent = detail;
}


function chartPalette() {
  const styles = getComputedStyle(document.documentElement);
  return {
    bid: styles.getPropertyValue("--bid").trim(),
    ask: styles.getPropertyValue("--ask").trim(),
  };
}

function numeric(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function formatChartPrice(value) {
  const number = numeric(value);
  if (number === null) return "—";
  const abs = Math.abs(number);
  const digits = abs >= 1000 ? 2 : abs >= 100 ? 3 : abs >= 1 ? 4 : 8;
  return number.toLocaleString(undefined, { maximumFractionDigits: digits });
}

function formatChartTime(timestamp) {
  const value = numeric(timestamp);
  if (value === null) return "—";
  return new Intl.DateTimeFormat("en", {
    timeZone: "UTC",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(value));
}

function renderChartReadout(candle = null) {
  const target = candle ?? chartCandles[chartCandles.length - 1] ?? null;
  if (!target) {
    for (const element of [chartLastPrice, chartOpen, chartHigh, chartLow, chartClose]) element.textContent = "—";
    chartTime.textContent = "—";
    return;
  }

  chartLastPrice.textContent = formatChartPrice(target.close);
  chartOpen.textContent = formatChartPrice(target.open);
  chartHigh.textContent = formatChartPrice(target.high);
  chartLow.textContent = formatChartPrice(target.low);
  chartClose.textContent = formatChartPrice(target.close);
  chartTime.textContent = formatChartTime(target.open_time_ms);
}

function setChartStatus(message) {
  tradingChartStatus.textContent = message;
}

function toMarketChartCandle(candle) {
  return {
    open_time: candle.open_time_ms,
    close_time: candle.close_time_ms,
    open: candle.open,
    high: candle.high,
    low: candle.low,
    close: candle.close,
    volume: candle.volume,
    is_closed: candle.is_closed,
  };
}

function bindTradingChartCrosshair() {
  if (!tradingChart?.chart || tradingChartCrosshairBound) return;
  tradingChartCrosshairBound = true;
  tradingChart.chart.subscribeCrosshairMove((param) => {
    const value = param?.seriesData?.get(tradingChart.series);
    if (!param?.time || !value || value.open === undefined) {
      renderChartReadout();
      return;
    }
    renderChartReadout({
      open_time_ms: Number(param.time) * 1000,
      open: value.open,
      high: value.high,
      low: value.low,
      close: value.close,
    });
  });
}

function ensureTradingChart() {
  if (!tradingChartElement || !window.LightweightCharts || typeof MarketChart !== "function") {
    setChartStatus("Lightweight Charts is unavailable.");
    return null;
  }
  if (!tradingChart) {
    tradingChart = new MarketChart(tradingChartElement, { showWeekends: false, indicators: false });
    tradingChart.initialize();
    chartFillMarkersPlugin = LightweightCharts.createSeriesMarkers(tradingChart.series, [], { autoScale: false });
    bindTradingChartCrosshair();
  }
  return tradingChart;
}

function resetRunOverlays() {
  const chart = ensureTradingChart();
  if (!chart) return;
  for (const series of chartOrderSeries.values()) {
    chart.chart.removeSeries(series);
  }
  for (const line of chartOrderPriceLines.values()) {
    chart.series.removePriceLine(line);
  }
  chartOrderSeries = new Map();
  chartOrderPriceLines = new Map();
  chartFillMarkersPlugin?.setMarkers([]);
}

function resetRunChartState(message = null) {
  chartBootstrapToken += 1;
  chartRunId = null;
  chartOrderLevels = new Map();
  chartFillMarkers = new Map();
  resetRunOverlays();
  if (message) setChartStatus(message);
  renderChartReadout();
}

function resetTradingChart(message = "Loading live market…") {
  chartBootstrapToken += 1;
  chartRunId = null;
  chartCandles = [];
  chartOrderLevels = new Map();
  chartFillMarkers = new Map();
  const chart = ensureTradingChart();
  resetRunOverlays();
  chart?.reset();
  tradingChartBadge.textContent = "Binance 1m";
  renderChartReadout(null);
  setChartStatus(message);
}

function selectedMarketStreamKey() {
  const symbol = String(symbolInput.value || "").trim().toUpperCase();
  const marketType = String(marketTypeInput.value || "").trim().toLowerCase();
  if (!symbol || !hasRegisteredInstrument(symbol, marketType)) return null;
  return { symbol, marketType, interval: "1m" };
}

function marketStreamKeyString(key) {
  return key ? [key.symbol, key.marketType, key.interval].join("|") : null;
}

function setMarketFeedState(state, detail = "") {
  const states = {
    live: ["live", "Live"],
    loading: ["loading", "Connecting"],
    reconnecting: ["reconnecting", "Reconnecting"],
    error: ["reconnecting", "Unavailable"],
    pending: ["pending", "Inactive"],
  };
  const [dotState, label] = states[state] ?? states.pending;
  setDot(feedDot, dotState);
  feedLabel.textContent = label;
  feedDetail.textContent = detail;
}

function marketStreamUrl(key) {
  const params = new URLSearchParams({
    symbol: key.symbol,
    interval: key.interval,
    market_type: key.marketType,
  });
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return protocol + "//" + window.location.host + "/api/market/stream?" + params;
}

function closeMarketStream() {
  marketConnectionToken += 1;
  if (marketReconnectTimer !== null) {
    window.clearTimeout(marketReconnectTimer);
    marketReconnectTimer = null;
  }
  const socket = marketStream;
  marketStream = null;
  marketStreamKey = null;
  if (socket) socket.close();
}

function scheduleMarketReconnect(key, token) {
  if (marketReconnectTimer !== null) return;
  marketReconnectTimer = window.setTimeout(() => {
    marketReconnectTimer = null;
    if (token === marketConnectionToken && marketStreamKeyString(key) === marketStreamKey) {
      openMarketStream(key, false);
    }
  }, 1000);
}

function normalizeOrderLevel(order) {
  const price = numeric(order.price);
  const quantity = numeric(order.quantity ?? order.original_quantity ?? order.originalQuantity);
  const activeFrom = numeric(order.active_from_ms ?? order.submitted_at_ms ?? order.submittedAtMs);
  const activeTo = order.active_to_ms == null ? null : numeric(order.active_to_ms);
  if (price === null || activeFrom === null) return null;
  return {
    order_id: Number(order.order_id ?? order.id),
    side: String(order.side || "").toLowerCase(),
    price,
    quantity: quantity ?? 0,
    active_from_ms: activeFrom,
    active_to_ms: activeTo,
    final_status: order.final_status ?? null,
  };
}

function normalizeFill(fill) {
  const price = numeric(fill.price);
  const quantity = numeric(fill.quantity);
  const eventTime = numeric(fill.event_time_ms ?? fill.eventTimeMs);
  if (price === null || eventTime === null) return null;
  return {
    order_id: Number(fill.order_id ?? fill.orderId),
    side: String(fill.side || "").toLowerCase(),
    price,
    quantity: quantity ?? 0,
    event_time_ms: eventTime,
  };
}

function fillKey(fill) {
  return [fill.order_id, fill.event_time_ms, fill.price, fill.quantity].join(":");
}

function upsertChartCandle(candle) {
  if (!candle) return null;
  const normalized = {
    open_time_ms: numeric(candle.open_time_ms ?? candle.open_time),
    close_time_ms: numeric(candle.close_time_ms ?? candle.close_time),
    open: numeric(candle.open),
    high: numeric(candle.high),
    low: numeric(candle.low),
    close: numeric(candle.close),
    volume: numeric(candle.volume) ?? 0,
    is_closed: Boolean(candle.is_closed),
  };
  if ([normalized.open_time_ms, normalized.close_time_ms, normalized.open, normalized.high, normalized.low, normalized.close].some((value) => value === null)) return null;
  const existing = chartCandles.findIndex((item) => item.open_time_ms === normalized.open_time_ms);
  if (existing >= 0) {
    chartCandles[existing] = normalized;
  } else {
    chartCandles.push(normalized);
    chartCandles.sort((a, b) => a.open_time_ms - b.open_time_ms);
    while (chartCandles.length > 1000) chartCandles.shift();
  }
  return normalized;
}

function candleTimeForEvent(eventTimeMs) {
  if (!chartCandles.length) return null;
  let candidate = chartCandles[0];
  for (const candle of chartCandles) {
    if (candle.open_time_ms <= eventTimeMs) candidate = candle;
    else break;
  }
  return Math.floor(candidate.open_time_ms / 1000);
}

function renderRunOverlays() {
  const chart = ensureTradingChart();
  if (!chart || !chartCandles.length) return;
  const palette = chartPalette();
  const firstTimeMs = chartCandles[0].open_time_ms;
  const lastTimeMs = chartCandles[chartCandles.length - 1].open_time_ms;
  const requiredClosedOrderIds = new Set();
  const requiredActiveOrderIds = new Set();

  for (const order of chartOrderLevels.values()) {
    if (!Number.isFinite(order.price)) continue;
    const color = order.side === "buy" ? palette.bid : palette.ask;

    if (order.active_to_ms === null) {
      requiredActiveOrderIds.add(order.order_id);
      let priceLine = chartOrderPriceLines.get(order.order_id);
      const options = {
        price: order.price,
        color,
        lineWidth: 2,
        lineStyle: LightweightCharts.LineStyle.Dashed,
        axisLabelVisible: true,
        title: (order.side === "buy" ? "BUY" : "SELL") + " #" + order.order_id,
      };
      if (!priceLine) {
        priceLine = chart.series.createPriceLine(options);
        chartOrderPriceLines.set(order.order_id, priceLine);
      } else {
        priceLine.applyOptions(options);
      }
      continue;
    }

    const startMs = Math.max(order.active_from_ms, firstTimeMs);
    const endMs = Math.min(order.active_to_ms, lastTimeMs);
    if (endMs < firstTimeMs || startMs > lastTimeMs) continue;
    requiredClosedOrderIds.add(order.order_id);

    let series = chartOrderSeries.get(order.order_id);
    if (!series) {
      series = chart.chart.addSeries(LightweightCharts.LineSeries, {
        color,
        lineWidth: 2,
        lineStyle: LightweightCharts.LineStyle.Dashed,
        crosshairMarkerVisible: false,
        lastValueVisible: false,
        priceLineVisible: false,
      });
      chartOrderSeries.set(order.order_id, series);
    } else {
      series.applyOptions({ color });
    }

    const start = candleTimeForEvent(startMs);
    const end = candleTimeForEvent(endMs);
    if (start === null || end === null) continue;
    const points = start === end
      ? [{ time: start, value: order.price }]
      : [{ time: start, value: order.price }, { time: end, value: order.price }];
    series.setData(points);
  }

  for (const [orderId, series] of chartOrderSeries.entries()) {
    if (requiredClosedOrderIds.has(orderId)) continue;
    chart.chart.removeSeries(series);
    chartOrderSeries.delete(orderId);
  }
  for (const [orderId, line] of chartOrderPriceLines.entries()) {
    if (requiredActiveOrderIds.has(orderId)) continue;
    chart.series.removePriceLine(line);
    chartOrderPriceLines.delete(orderId);
  }

  const markers = Array.from(chartFillMarkers.values())
    .map((fill) => {
      const time = candleTimeForEvent(fill.event_time_ms);
      if (time === null) return null;
      const buy = fill.side === "buy";
      return {
        id: fillKey(fill),
        time,
        price: fill.price,
        position: "atPriceMiddle",
        shape: buy ? "arrowUp" : "arrowDown",
        color: buy ? palette.bid : palette.ask,
        text: "#" + fill.order_id,
        size: 1,
      };
    })
    .filter(Boolean)
    .sort((left, right) => Number(left.time) - Number(right.time));

  chartFillMarkersPlugin?.setMarkers(markers);
}

function renderTradingChart({ fitContent = false } = {}) {
  const chart = ensureTradingChart();
  if (!chart) return;
  if (!chartCandles.length) {
    chart.reset();
    renderChartReadout(null);
    setChartStatus(
      marketStreamKey
        ? "Waiting for live Binance 1-minute candles."
        : "Choose a registered Symbol and Market to open the live chart."
    );
    return;
  }

  chart.setCandles(chartCandles.map(toMarketChartCandle), fitContent);
  renderRunOverlays();
  renderChartReadout();

  const selectedKey = selectedMarketStreamKey();
  const chartSymbol = currentMonitor?.market.symbol || selectedKey?.symbol || "market";
  tradingChartBadge.textContent = "Binance 1m · " + chartSymbol;
  setChartStatus(
    chartCandles.length + " live candles · " + chartOrderLevels.size + " active/run order levels · " + chartFillMarkers.size + " run fills · UTC"
  );
}

function applyMarketStreamSnapshot(snapshot, key) {
  const chart = ensureTradingChart();
  const fitContent = !chart?.candleCount;
  chartCandles = [];
  for (const candle of snapshot?.candles ?? []) upsertChartCandle(candle);
  renderTradingChart({ fitContent });
  setMarketFeedState(snapshot?.status ?? "live", key.symbol + " · " + marketTypeLabel(key.marketType) + " · Binance public 1m");
}

function applyMarketStreamUpdate(update, key) {
  const normalized = update?.candle ? upsertChartCandle(update.candle) : null;
  const chart = ensureTradingChart();
  if (normalized && chart) {
    chart.updateCandle(toMarketChartCandle(normalized));
    renderRunOverlays();
    renderChartReadout();
  }
  setMarketFeedState(update?.status ?? "live", key.symbol + " · " + marketTypeLabel(key.marketType) + " · Binance public 1m");
}

function openMarketStream(key = selectedMarketStreamKey(), clearCandles = true) {
  if (!key) {
    closeMarketStream();
    if (!currentMonitor?.run.runtimeActive) resetTradingChart("Choose a registered Symbol and Market.");
    setMarketFeedState("pending", "Choose a registered Binance market");
    return;
  }

  const keyString = marketStreamKeyString(key);
  if (marketStream && marketStreamKey === keyString) return;

  closeMarketStream();
  const token = marketConnectionToken;
  marketStreamKey = keyString;
  if (clearCandles) {
    chartCandles = [];
    if (!currentMonitor?.run.runtimeActive) {
      chartOrderLevels = new Map();
      chartFillMarkers = new Map();
      chartRunId = null;
    }
    const chart = ensureTradingChart();
    resetRunOverlays();
    chart?.reset();
    renderChartReadout(null);
  }

  setMarketFeedState("loading", key.symbol + " · " + marketTypeLabel(key.marketType) + " · loading Binance 1m");
  setChartStatus("Loading live Binance 1-minute candles for " + key.symbol + "…");
  const socket = new WebSocket(marketStreamUrl(key));
  marketStream = socket;

  socket.addEventListener("message", (event) => {
    if (token !== marketConnectionToken || socket !== marketStream || marketStreamKey !== keyString) return;
    try {
      const message = JSON.parse(event.data);
      if (message.type === "snapshot") applyMarketStreamSnapshot(message.data, key);
      if (message.type === "update") applyMarketStreamUpdate(message.data, key);
    } catch {
      setMarketFeedState("reconnecting", key.symbol + " · invalid market update");
    }
  });

  socket.addEventListener("error", () => {
    if (token === marketConnectionToken && socket === marketStream) socket.close();
  });

  socket.addEventListener("close", () => {
    if (token !== marketConnectionToken || socket !== marketStream) return;
    marketStream = null;
    setMarketFeedState("reconnecting", key.symbol + " · reconnecting Binance public 1m");
    scheduleMarketReconnect(key, token);
  });
}

function syncMarketStreamToSelection(clearCandles = true) {
  const key = selectedMarketStreamKey();
  const keyString = marketStreamKeyString(key);
  if (keyString && keyString === marketStreamKey && marketStream) return;
  openMarketStream(key, clearCandles);
}

async function loadChartBootstrap(runId, quiet = false) {
  const token = ++chartBootstrapToken;
  if (!quiet) setChartStatus("Loading run overlays for Run #" + runId + "…");
  chartRefreshInFlight = true;
  try {
    const chartUrl = activeAdapter.urls.chart(runId);
    if (!chartUrl) throw new Error(activeAdapter.label + " chart adapter is unavailable.");
    const payload = await fetchJson(chartUrl);
    if (token !== chartBootstrapToken || currentRunId !== runId) return;
    chartRunId = runId;
    if (!chartCandles.length) {
      for (const candle of payload.candles ?? []) upsertChartCandle(candle);
    }
    chartOrderLevels = new Map();
    for (const raw of payload.order_levels ?? []) {
      const order = normalizeOrderLevel(raw);
      if (order) chartOrderLevels.set(order.order_id, order);
    }
    chartFillMarkers = new Map();
    for (const raw of payload.fills ?? []) {
      const fill = normalizeFill(raw);
      if (fill) chartFillMarkers.set(fillKey(fill), fill);
    }
    renderTradingChart();
  } catch (error) {
    if (token === chartBootstrapToken && currentRunId === runId) {
      setChartStatus("Could not load run overlays · " + error.message);
    }
  } finally {
    if (token === chartBootstrapToken) chartRefreshInFlight = false;
  }
}

function syncChartFromMonitor(monitor) {
  if (!monitor?.run.id) return;
  if (chartRunId !== monitor.run.id) {
    void loadChartBootstrap(monitor.run.id);
    return;
  }

  const normalizedCandle = monitor.market.latestBaseCandle
    ? upsertChartCandle(monitor.market.latestBaseCandle)
    : null;
  if (normalizedCandle) ensureTradingChart()?.updateCandle(toMarketChartCandle(normalizedCandle));

  let overlaysChanged = false;
  const activeOrderIds = new Set();
  for (const raw of monitor.orders) {
    activeOrderIds.add(Number(raw.id));
    if (!chartOrderLevels.has(Number(raw.id))) {
      const order = normalizeOrderLevel(raw);
      if (order) {
        chartOrderLevels.set(order.order_id, order);
        overlaysChanged = true;
      }
    }
  }

  for (const raw of monitor.fills) {
    const fill = normalizeFill(raw);
    if (!fill) continue;
    const key = fillKey(fill);
    if (!chartFillMarkers.has(key)) {
      chartFillMarkers.set(key, fill);
      overlaysChanged = true;
    }
  }

  for (const order of chartOrderLevels.values()) {
    if (order.active_to_ms !== null || activeOrderIds.has(order.order_id)) continue;
    const orderFills = Array.from(chartFillMarkers.values()).filter((fill) => fill.order_id === order.order_id);
    if (orderFills.length) {
      order.active_to_ms = Math.max(...orderFills.map((fill) => fill.event_time_ms));
      overlaysChanged = true;
    } else if (!monitor.run.runtimeActive && monitor.run.endedAtMs) {
      order.active_to_ms = monitor.run.endedAtMs;
      overlaysChanged = true;
    }
  }

  renderRunOverlays();
  renderChartReadout();
  if (overlaysChanged && !chartRefreshInFlight) {
    void loadChartBootstrap(monitor.run.id, true);
  }
}


function formatOperationalNumber(value, maximumFractionDigits = 8) {
  const number = numeric(value);
  if (number === null) return "—";
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits,
    minimumFractionDigits: 0,
  }).format(number);
}

function formatPercent(value) {
  const number = numeric(value);
  return number === null ? "—" : number.toFixed(2) + "%";
}

function formatAge(timestamp) {
  const eventTime = numeric(timestamp);
  if (eventTime === null) return "—";
  const seconds = Math.max(0, Math.floor((Date.now() - eventTime) / 1000));
  if (seconds < 60) return seconds + "s";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return minutes + "m " + (seconds % 60) + "s";
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return hours + "h " + (minutes % 60) + "m";
  const days = Math.floor(hours / 24);
  return days + "d " + (hours % 24) + "h";
}

function formatUtcTimestamp(timestamp) {
  const value = numeric(timestamp);
  if (value === null) return "—";
  return new Date(value).toISOString().replace("T", " ").replace("Z", "");
}

function renderPortfolio(monitor) {
  const portfolio = monitor.portfolio;
  portfolioPosition.textContent = formatOperationalNumber(portfolio.positionQuantity);
  portfolioCash.textContent = formatOperationalNumber(portfolio.cash);
  portfolioEquity.textContent = formatOperationalNumber(portfolio.equity);
  portfolioExposure.textContent = formatPercent(portfolio.grossExposurePercent);
  portfolioRealized.textContent = formatOperationalNumber(portfolio.realizedPnl);
  portfolioUnrealized.textContent = formatOperationalNumber(portfolio.unrealizedPnl);
  portfolioFees.textContent = formatOperationalNumber(portfolio.feesPaid);
  portfolioMark.textContent = formatOperationalNumber(monitor.market.markPrice);
  portfolioBadge.textContent = monitor.run.runtimeActive ? "Live snapshot" : "Persisted final";
}

function emptyTableRow(columnCount, message) {
  const row = document.createElement("tr");
  const cell = document.createElement("td");
  cell.className = "empty-row";
  cell.colSpan = columnCount;
  cell.textContent = message;
  row.appendChild(cell);
  return row;
}

function tableCell(value, className = "") {
  const cell = document.createElement("td");
  cell.textContent = value;
  if (className) cell.className = className;
  return cell;
}

function renderOpenOrders(monitor) {
  const orders = [...monitor.orders].sort(
    (left, right) => (left.submittedAtMs ?? 0) - (right.submittedAtMs ?? 0)
  );
  ordersBody.replaceChildren();
  ordersBadge.textContent = orders.length + " open";

  if (!orders.length) {
    ordersBody.appendChild(emptyTableRow(8, monitor.run.runtimeActive ? "No active orders." : "No open orders in this persisted terminal state."));
    ordersStatus.textContent = monitor.run.runtimeActive
      ? "Backend snapshot currently has no resting orders."
      : "Terminal snapshot · no live orders.";
    return;
  }

  for (const order of orders) {
    const row = document.createElement("tr");
    const side = order.side;
    const original = order.originalQuantity;
    const filled = order.filledQuantity;
    const remaining = order.remainingQuantity;
    const fillPercent = original > 0 ? Math.min(100, Math.max(0, (filled / original) * 100)) : 0;
    const fillState = filled > 0 ? "Partial · " + fillPercent.toFixed(1) + "%" : "Resting";

    row.appendChild(tableCell("#" + order.id));
    row.appendChild(tableCell(humanize(side), "trading-side trading-side--" + side));
    row.appendChild(tableCell(formatOperationalNumber(order.price)));
    row.appendChild(tableCell(formatOperationalNumber(original)));
    row.appendChild(tableCell(formatOperationalNumber(filled)));
    row.appendChild(tableCell(formatOperationalNumber(remaining)));
    row.appendChild(tableCell(fillState));
    row.appendChild(tableCell(formatAge(order.submittedAtMs)));
    ordersBody.appendChild(row);
  }

  ordersStatus.textContent = orders.length + " backend-owned open order" + (orders.length === 1 ? "" : "s") + " · updated " + formatUtcTimestamp(monitor.run.updatedAtMs) + " UTC";
}

function auditEventDetails(item) {
  const parts = [];
  if (item.order_id != null) parts.push("Order #" + item.order_id);
  if (item.side) parts.push(humanize(item.side));
  if (item.order_type) parts.push(humanize(item.order_type));
  if (item.quantity != null) parts.push("Qty " + String(item.quantity));
  if (item.price != null) parts.push("@ " + String(item.price));
  if (item.filled_quantity != null) parts.push("Filled " + String(item.filled_quantity));
  if (item.fee != null) parts.push("Fee " + String(item.fee));
  if (item.position_quantity != null) parts.push("Position " + String(item.position_quantity));
  if (item.equity != null) parts.push("Equity " + String(item.equity));
  return parts.length ? parts.join(" · ") : "—";
}

function renderAuditEvents() {
  auditBody.replaceChildren();
  if (!auditEvents.length) {
    auditBody.appendChild(emptyTableRow(4, "No canonical events are persisted for this run yet."));
  } else {
    for (const item of auditEvents) {
      const row = document.createElement("tr");
      const event = item.event ?? {};
      const eventCell = tableCell(humanize(item.label || event.event_kind || "event"), "trading-event-kind");
      const detailCell = tableCell(auditEventDetails(item));
      if (item.note) {
        const note = document.createElement("span");
        note.className = "trading-event-note";
        note.textContent = item.note;
        detailCell.appendChild(note);
      }
      row.appendChild(tableCell(String(event.run_sequence ?? "—")));
      row.appendChild(tableCell(formatUtcTimestamp(event.event_time_ms)));
      row.appendChild(eventCell);
      row.appendChild(detailCell);
      auditBody.appendChild(row);
    }
  }

  const shown = auditEvents.length;
  const mode = auditFullMode ? "complete audit" : "latest canonical events";
  auditStatus.textContent = "Showing " + shown + " of " + auditTotalEvents + " persisted events · " + mode + ".";
  auditLoadAllButton.disabled = auditFullMode && !auditHasMore;
  auditLoadAllButton.textContent = auditFullMode && !auditHasMore ? "Complete audit loaded" : "Load complete audit";
}

function resetAudit(message = "No Live-Paper run selected.") {
  auditRequestToken += 1;
  auditRunId = null;
  auditEvents = [];
  auditTotalEvents = 0;
  auditHasEarlier = false;
  auditHasMore = false;
  auditFullMode = false;
  auditRefreshInFlight = false;
  auditLastRefreshAt = 0;
  auditBody.replaceChildren(emptyTableRow(4, message));
  auditStatus.textContent = message;
  auditLoadAllButton.disabled = true;
  auditLoadAllButton.textContent = "Load complete audit";
}

async function loadAuditTail(runId, quiet = false) {
  const token = ++auditRequestToken;
  auditRefreshInFlight = true;
  if (!quiet) auditStatus.textContent = "Loading latest canonical events for Run #" + runId + "…";
  try {
    const auditUrl = activeAdapter.urls.auditTail(runId, 250);
    if (!auditUrl) throw new Error(activeAdapter.label + " audit adapter is unavailable.");
    const page = await fetchJson(auditUrl);
    if (token !== auditRequestToken || currentRunId !== runId) return;
    auditRunId = runId;
    auditEvents = page.events ?? [];
    auditTotalEvents = Number(page.total_events ?? auditEvents.length);
    auditHasEarlier = Boolean(page.has_earlier);
    auditHasMore = Boolean(page.has_more);
    auditFullMode = !auditHasEarlier;
    auditLastRefreshAt = Date.now();
    renderAuditEvents();
  } catch (error) {
    if (token === auditRequestToken && currentRunId === runId) {
      auditStatus.textContent = "Could not load canonical audit · " + error.message;
    }
  } finally {
    if (token === auditRequestToken) auditRefreshInFlight = false;
  }
}

async function appendNewAuditEvents(runId) {
  if (auditRefreshInFlight || auditRunId !== runId) return;
  auditRefreshInFlight = true;
  try {
    const lastSequence = auditEvents.length ? auditEvents[auditEvents.length - 1].event.run_sequence : 0;
    const auditUrl = activeAdapter.urls.auditAfter(runId, lastSequence, 500);
    if (!auditUrl) throw new Error(activeAdapter.label + " audit adapter is unavailable.");
    const page = await fetchJson(auditUrl);
    if (currentRunId !== runId || auditRunId !== runId) return;
    if (page.events?.length) {
      const bySequence = new Map(auditEvents.map((item) => [item.event.run_sequence, item]));
      for (const item of page.events) bySequence.set(item.event.run_sequence, item);
      auditEvents = Array.from(bySequence.values()).sort((a, b) => a.event.run_sequence - b.event.run_sequence);
    }
    auditTotalEvents = Number(page.total_events ?? auditTotalEvents);
    auditHasMore = Boolean(page.has_more);
    auditLastRefreshAt = Date.now();
    renderAuditEvents();
  } catch (error) {
    auditStatus.textContent = "Canonical audit refresh failed · " + error.message;
  } finally {
    auditRefreshInFlight = false;
  }
}

async function loadCompleteAudit(runId) {
  if (!runId || auditRefreshInFlight) return;
  const token = ++auditRequestToken;
  auditRefreshInFlight = true;
  auditLoadAllButton.disabled = true;
  auditLoadAllButton.textContent = "Loading complete audit…";
  auditStatus.textContent = "Retrieving every persisted event for Run #" + runId + "…";
  try {
    const all = [];
    let afterSequence = 0;
    let total = 0;
    while (true) {
      const auditUrl = activeAdapter.urls.auditAfter(runId, afterSequence, 500);
      if (!auditUrl) throw new Error(activeAdapter.label + " audit adapter is unavailable.");
      const page = await fetchJson(auditUrl);
      if (token !== auditRequestToken || currentRunId !== runId) return;
      all.push(...(page.events ?? []));
      total = Number(page.total_events ?? total);
      auditStatus.textContent = "Loading complete audit · " + all.length + " of " + total + " events…";
      if (!page.has_more || page.last_sequence == null) break;
      const nextSequence = Number(page.last_sequence);
      if (!Number.isFinite(nextSequence) || nextSequence <= afterSequence) {
        throw new Error("Audit pagination did not advance.");
      }
      afterSequence = nextSequence;
    }
    auditRunId = runId;
    auditEvents = all;
    auditTotalEvents = total;
    auditHasEarlier = false;
    auditHasMore = false;
    auditFullMode = true;
    auditLastRefreshAt = Date.now();
    renderAuditEvents();
  } catch (error) {
    if (token === auditRequestToken && currentRunId === runId) {
      auditStatus.textContent = "Could not load complete audit · " + error.message;
      auditLoadAllButton.disabled = false;
      auditLoadAllButton.textContent = "Load complete audit";
    }
  } finally {
    if (token === auditRequestToken) auditRefreshInFlight = false;
  }
}

function syncAuditFromMonitor(monitor) {
  const runId = monitor?.run.id;
  if (!runId) return;
  if (auditRunId !== runId) {
    void loadAuditTail(runId);
    return;
  }
  if (Date.now() - auditLastRefreshAt < 2000 || auditRefreshInFlight) return;
  if (auditFullMode) {
    void appendNewAuditEvents(runId);
  } else {
    void loadAuditTail(runId, true);
  }
}

function showControlError(message) {
  controlError.textContent = message;
  controlError.hidden = !message;
}

function syncFixedAnchorState() {
  fixedAnchorInput.disabled = Boolean(currentMonitor?.run.runtimeActive) || activeMode !== "paper" || anchorInput.value !== "fixed";
}

function setConfigLocked(locked) {
  document.querySelectorAll("[data-trading-config]").forEach((control) => {
    control.disabled = locked;
  });
  const instrumentsAvailable = instrumentCatalogReady && !instrumentCatalogError && registeredMarketsBySymbol.size > 0;
  const persistedBotSelected = Number.isInteger(Number(selectedBotId)) && Number(selectedBotId) > 0 && !botDraftMode;
  const dirty = hasUnsavedBotChanges();
  const selectedRun = persistedBotSelected ? activeRunForBot(selectedBotId) : null;
  const anotherRunActive = anyActiveLivePaperRun() && !selectedRun;

  symbolInput.disabled = locked || !instrumentsAvailable;
  marketTypeInput.disabled = locked || !instrumentsAvailable;
  botNameInput.disabled = locked;
  saveBotButton.disabled = locked
    || !instrumentsAvailable
    || !botNameInput.value.trim()
    || (!botDraftMode && !dirty);
  startButton.disabled = locked
    || !instrumentsAvailable
    || !persistedBotSelected
    || dirty
    || anotherRunActive;
  stopButton.disabled = !locked;
  paperModeButton.disabled = locked || activeMode !== "paper";
  liveModeButton.disabled = TradingContract.adapterFor("live").locked || locked;

  if (locked) {
    configLockBadge.textContent = "Locked · Live-Paper running";
  } else if (botDraftMode) {
    configLockBadge.textContent = "New Bot · unsaved";
  } else if (dirty) {
    configLockBadge.textContent = "Unsaved changes";
  } else if (persistedBotSelected) {
    configLockBadge.textContent = "Saved · Bot #" + selectedBotId;
  } else {
    configLockBadge.textContent = "Editable";
  }

  syncFixedAnchorState();
}

function setControlStatus(message) {
  controlStatus.textContent = message;
}

function numberValue(input, label) {
  const value = Number(input.value);
  if (!Number.isFinite(value)) {
    throw new Error(label + " must be a finite number.");
  }
  return value;
}

function applySnapshotToConfig(snapshot) {
  if (snapshot.symbol && hasRegisteredInstrument(snapshot.symbol, snapshot.market_type)) {
    selectRegisteredInstrument(snapshot.symbol, snapshot.market_type);
  }
  replayIntervalInput.value = snapshot.replay_interval || replayIntervalInput.value;
  capitalInput.value = snapshot.initial_capital || capitalInput.value;
  strategyInput.value = snapshot.strategy_id || strategyInput.value;
  const grid = snapshot.strategy_params || {};
  anchorInput.value = grid.anchor || anchorInput.value;
  fixedAnchorInput.value = grid.fixed_anchor_price ?? "";
  spacingInput.value = grid.spacing_bps ?? spacingInput.value;
  levelsInput.value = grid.levels_per_side ?? levelsInput.value;
  quantityInput.value = grid.quantity_per_order ?? quantityInput.value;
  const execution = snapshot.execution_assumptions || {};
  feeInput.value = execution.fee_bps ?? feeInput.value;
  spreadInput.value = execution.spread_bps ?? spreadInput.value;
  slippageInput.value = execution.slippage_bps ?? slippageInput.value;
  latencyInput.value = execution.latency_ms ?? latencyInput.value;
  limitPolicyInput.value = execution.limit_fill_policy || limitPolicyInput.value;
  partialFillInput.value = execution.partial_fill_ratio ?? partialFillInput.value;
  syncFixedAnchorState();
}

function buildStartConfiguration() {
  const symbol = symbolInput.value.trim().toUpperCase();
  if (!/^[A-Z0-9]+$/.test(symbol)) {
    throw new Error("Symbol must contain only letters and numbers.");
  }
  const marketType = marketTypeInput.value;
  if (!hasRegisteredInstrument(symbol, marketType)) {
    throw new Error("Choose a registered Binance symbol and market.");
  }
  const anchor = anchorInput.value;
  const fixedAnchorPrice = anchor === "fixed" ? numberValue(fixedAnchorInput, "Fixed anchor price") : null;
  return {
    symbol,
    market_type: marketType,
    replay_interval: replayIntervalInput.value,
    initial_capital: capitalInput.value.trim(),
    strategy_id: strategyInput.value,
    grid: {
      anchor,
      fixed_anchor_price: fixedAnchorPrice,
      spacing_bps: numberValue(spacingInput, "Grid spacing"),
      levels_per_side: numberValue(levelsInput, "Levels per side"),
      quantity_per_order: numberValue(quantityInput, "Quantity per order"),
    },
    execution: {
      fee_bps: numberValue(feeInput, "Fee"),
      spread_bps: numberValue(spreadInput, "Spread"),
      slippage_bps: numberValue(slippageInput, "Slippage"),
      latency_ms: numberValue(latencyInput, "Latency"),
      limit_fill_policy: limitPolicyInput.value,
      partial_fill_ratio: numberValue(partialFillInput, "Partial fill ratio"),
    },
  };
}

async function requestJson(url, options = {}) {
  const response = await fetch(url, {
    credentials: "same-origin",
    headers: {
      Accept: "application/json",
      ...(options.body ? { "Content-Type": "application/json" } : {}),
    },
    ...options,
  });
  const responseText = await response.text();
  if (!response.ok) {
    throw new Error(responseText || (response.status + " " + response.statusText));
  }
  return responseText ? JSON.parse(responseText) : null;
}

function updateRunSummaryFromSnapshot(snapshot) {
  const runId = Number(snapshot?.run_id);
  if (!Number.isInteger(runId) || runId <= 0) return;
  const botId = Number(snapshot?.bot_id);
  const summary = {
    run_id: runId,
    bot_id: Number.isInteger(botId) && botId > 0 ? botId : null,
    canonical_status: String(snapshot?.canonical_status ?? ""),
    runtime_status: String(snapshot?.runtime_status ?? ""),
    symbol: String(snapshot?.symbol ?? ""),
    market_type: String(snapshot?.market_type ?? ""),
    replay_interval: String(snapshot?.replay_interval ?? ""),
    strategy_id: String(snapshot?.strategy_id ?? ""),
    created_at_ms: Number(snapshot?.created_at_ms ?? Date.now()),
    started_at_ms: snapshot?.started_at_ms ?? null,
    ended_at_ms: snapshot?.ended_at_ms ?? null,
  };
  const index = tradingRunSummaries.findIndex((run) => Number(run.run_id) === runId);
  if (index >= 0) tradingRunSummaries[index] = summary;
  else tradingRunSummaries.unshift(summary);
}

async function loadBotCatalog() {
  const runsUrl = activeAdapter.urls.runs(500);
  if (!runsUrl) throw new Error(activeAdapter.label + " runtime adapter is unavailable.");
  const [botsPayload, runsPayload] = await Promise.all([
    fetchJson("/api/trading/bots"),
    fetchJson(runsUrl),
  ]);
  tradingBots = Array.isArray(botsPayload?.bots) ? botsPayload.bots : [];
  tradingRunSummaries = Array.isArray(runsPayload?.runs) ? runsPayload.runs : [];
  if (selectedBotId != null) {
    selectedBot = tradingBots.find((bot) => Number(bot.bot_id) === Number(selectedBotId)) ?? null;
  }
  renderBotStrip();
}

function renderIdleBotWorkspace(message = null) {
  renderNoRun();
  renderSelectedBotContext();
  const label = selectedBot ? ("Bot #" + selectedBot.bot_id) : "New Bot";
  runStatusElement.textContent = selectedBot
    ? "Saved Bot · no active Live-Paper run."
    : "No persisted Bot selected.";
  topLabel.textContent = selectedBot ? "Live-Paper · idle" : "Live-Paper · new Bot";
  setControlStatus(
    message
      ?? (selectedBot
        ? "Bot #" + selectedBot.bot_id + " is idle. Edit and Save Bot, or start Live-Paper when configuration is saved."
        : "Configure the new Bot and Save Bot before starting Live-Paper.")
  );
  setBotStatus(
    selectedBot
      ? (hasUnsavedBotChanges()
        ? "Bot #" + selectedBot.bot_id + " · Unsaved changes"
        : "Bot #" + selectedBot.bot_id + " · Idle")
      : "New Bot · not saved"
  );
  setConfigLocked(false);
}

async function selectBot(botId) {
  const bot = tradingBots.find((candidate) => Number(candidate.bot_id) === Number(botId));
  if (!bot) return;
  const token = ++botSelectionToken;
  const activeRun = activeRunForBot(bot.bot_id);

  closeStream();
  selectedBotId = Number(bot.bot_id);
  selectedBot = bot;
  botDraftMode = false;
  applyBotConfiguration(bot);
  selectedBotBaseline = botFormFingerprint();
  showControlError("");
  renderBotStrip();

  if (!activeRun) {
    renderIdleBotWorkspace();
    syncMarketStreamToSelection(true);
    setConnection("connected", "Bot #" + bot.bot_id + " selected · no active Live-Paper run");
    return;
  }

  renderNoRun();
  syncMarketStreamToSelection(true);
  setConfigLocked(true);
  stopButton.disabled = true;
  runIdElement.textContent = "Run #" + activeRun.run_id;
  runStatusElement.textContent = "Loading active Live-Paper runtime…";
  runBadgeElement.textContent = "Loading";
  setBotStatus("Bot #" + bot.bot_id + " · Running · Live-Paper · Run #" + activeRun.run_id);
  setControlStatus("Loading Run #" + activeRun.run_id + " for Bot #" + bot.bot_id + "…");

  try {
    const snapshotUrl = activeAdapter.urls.snapshot(activeRun.run_id);
    if (!snapshotUrl) throw new Error(activeAdapter.label + " snapshot adapter is unavailable.");
    const snapshot = await fetchJson(snapshotUrl);
    if (token !== botSelectionToken || Number(selectedBotId) !== Number(bot.bot_id)) return;
    renderSnapshot(snapshot);
    setBotStatus("Bot #" + bot.bot_id + " · Running · Live-Paper · Run #" + activeRun.run_id);
    setConnection("connected", "Loaded Bot #" + bot.bot_id + " · Run #" + activeRun.run_id);
    if (snapshot.runtime_active) connectStream(snapshot.run_id);
  } catch (error) {
    if (token !== botSelectionToken) return;
    setConfigLocked(true);
    stopButton.disabled = true;
    runIdElement.textContent = "Run #" + activeRun.run_id;
    runStatusElement.textContent = "Active Live-Paper runtime state unavailable.";
    runBadgeElement.textContent = "Running";
    setBotStatus("Bot #" + bot.bot_id + " · Running · Live-Paper · Run #" + activeRun.run_id);
    setControlStatus("Run #" + activeRun.run_id + " is still treated as active. Reload or reselect the Bot to retry state loading.");
    setConnection("error", "Could not load Bot #" + bot.bot_id + " runtime · " + error.message);
  }
}

function openNewBotDraft() {
  botSelectionToken += 1;
  closeStream();
  selectedBotId = null;
  selectedBot = null;
  selectedBotBaseline = null;
  botDraftMode = true;
  showControlError("");
  resetBotFormToDefaults();
  renderBotStrip();
  renderIdleBotWorkspace();
  syncMarketStreamToSelection(true);
  setConnection("connected", "New Bot draft · no execution started");
}

async function saveCurrentBot() {
  if (currentMonitor?.run.runtimeActive) return;
  showControlError("");
  saveBotButton.disabled = true;
  try {
    const botName = botNameInput.value.trim();
    if (!botName) throw new Error("Bot name is required.");
    const configuration = buildStartConfiguration();
    const creating = botDraftMode || !selectedBotId;
    const url = creating ? "/api/trading/bots" : ("/api/trading/bots/" + selectedBotId);
    const bot = await requestJson(url, {
      method: creating ? "POST" : "PUT",
      body: JSON.stringify({
        bot_name: botName,
        configuration,
      }),
    });

    const index = tradingBots.findIndex((candidate) => Number(candidate.bot_id) === Number(bot.bot_id));
    if (index >= 0) tradingBots[index] = bot;
    else tradingBots.unshift(bot);
    selectedBotId = Number(bot.bot_id);
    selectedBot = bot;
    botDraftMode = false;
    applyBotConfiguration(bot);
    selectedBotBaseline = botFormFingerprint();
    renderBotStrip();
    renderSelectedBotContext();
    setConfigLocked(false);
    setBotStatus("Bot #" + bot.bot_id + " · Saved · Idle");
    setControlStatus("Bot #" + bot.bot_id + " saved. Live-Paper has not started.");
  } catch (error) {
    showControlError(error.message);
    setConfigLocked(false);
    setControlStatus("Bot was not saved.");
  }
}

function handleBotDraftChange() {
  if (currentMonitor?.run.runtimeActive) return;
  renderSelectedBotContext();
  setConfigLocked(false);
  if (botDraftMode) {
    setBotStatus("New Bot · not saved");
    setControlStatus("New Bot configuration is not saved.");
  } else if (hasUnsavedBotChanges()) {
    setBotStatus("Bot #" + selectedBotId + " · Unsaved changes");
    setControlStatus("Unsaved changes · Save Bot before starting Live-Paper.");
  } else {
    setBotStatus("Bot #" + selectedBotId + " · Idle");
    setControlStatus("Bot #" + selectedBotId + " is saved and ready for Live-Paper.");
  }
}

async function initializeBotWorkspace() {
  setConnection("connecting", "Reading saved Bots and Live-Paper state");
  try {
    await loadBotCatalog();
    const firstRunning = sortedBotsForStrip().find((bot) => activeRunForBot(bot.bot_id));
    const target = firstRunning ?? sortedBotsForStrip()[0] ?? null;
    if (target) {
      await selectBot(target.bot_id);
    } else {
      openNewBotDraft();
      setConnection("connected", "Protected Trading API available · no bots created");
    }
  } catch (error) {
    openNewBotDraft();
    setConnection("error", "Could not read Bot workspace · " + error.message);
  }
}

function renderNoRun() {
  closeStream();
  currentSnapshot = null;
  currentMonitor = null;
  currentRunId = null;
  runIdElement.textContent = "No active run";
  runStatusElement.textContent = "No active " + activeAdapter.label + " runtime.";
  runBadgeElement.textContent = "Idle";
  setDot(topDot, "pending");
  topLabel.textContent = activeAdapter.label + " · inactive";
  renderSelectedBotContext();
  setConfigLocked(false);
  setControlStatus(activeAdapter.locked ? activeAdapter.lockReason : "No active Live-Paper run.");
  resetRunChartState("Live market remains available with no active Live-Paper run.");
  portfolioBadge.textContent = "No active run";
  for (const element of [portfolioPosition, portfolioCash, portfolioEquity, portfolioExposure, portfolioRealized, portfolioUnrealized, portfolioFees, portfolioMark]) {
    element.textContent = "—";
  }
  ordersBadge.textContent = "0 open";
  ordersBody.replaceChildren(emptyTableRow(8, "No active Live-Paper run."));
  ordersStatus.textContent = "No active Live-Paper run.";
  resetAudit("No active Live-Paper run for the selected Bot.");
}

function renderSnapshot(snapshot) {
  const monitor = TradingContract.normalizeSnapshot(snapshot);
  if (selectedBotId && monitor.run.botId && Number(monitor.run.botId) !== Number(selectedBotId)) return;
  updateRunSummaryFromSnapshot(snapshot);
  renderBotStrip();
  if (!monitor.run.runtimeActive) {
    const terminalStatus = humanize(monitor.run.runtimeStatus || monitor.run.canonicalStatus || "ended");
    const terminalRunId = monitor.run.id;
    renderIdleBotWorkspace("Run #" + terminalRunId + " is " + terminalStatus + ". The Bot remains saved and idle.");
    setConnection("connected", "Bot #" + (selectedBotId ?? "—") + " idle · Run #" + terminalRunId + " " + terminalStatus);
    renderBotStrip();
    return;
  }
  setTradingMode(monitor.run.mode);
  currentSnapshot = snapshot;
  currentMonitor = monitor;
  currentRunId = monitor.run.id;
  runIdElement.textContent = "Run #" + monitor.run.id;
  runStatusElement.textContent = monitor.run.runtimeActive
    ? "Backend runtime · " + humanize(monitor.run.runtimeStatus)
    : "Persisted run · " + humanize(monitor.run.canonicalStatus);
  runBadgeElement.textContent = humanize(monitor.run.runtimeStatus || monitor.run.canonicalStatus);

  symbolElement.textContent = monitor.market.symbol || "—";
  marketTypeElement.textContent = humanize(monitor.market.marketType) || "—";
  intervalElement.textContent = monitor.market.replayInterval || "—";
  strategyElement.textContent = monitor.strategy.id || "—";
  if (monitor.run.mode === "paper") {
    applySnapshotToConfig(snapshot);
    syncMarketStreamToSelection(false);
  }
  setConfigLocked(Boolean(monitor.run.runtimeActive));
  if (monitor.run.runtimeActive && selectedBotId) {
    setBotStatus("Bot #" + selectedBotId + " · Running · Live-Paper · Run #" + monitor.run.id);
  }
  setControlStatus(monitor.run.runtimeActive
    ? ("Run #" + monitor.run.id + " is active. Stop it before changing the saved Bot configuration.")
    : (activeAdapter.locked
      ? activeAdapter.lockReason
      : "Loaded persisted Run #" + monitor.run.id + ". " + activeAdapter.label + " setup is editable for the next run."));
  syncChartFromMonitor(monitor);
  renderPortfolio(monitor);
  renderOpenOrders(monitor);
  syncAuditFromMonitor(monitor);

  const runtimeState = monitor.market.feedStatus || "loading";
  setDot(topDot, runtimeState === "live" ? "live" : runtimeState);
  topLabel.textContent = activeAdapter.label + " · " + humanize(monitor.run.runtimeStatus);
}

async function fetchJson(url) {
  const response = await fetch(url, {
    credentials: "same-origin",
    headers: { Accept: "application/json" },
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json();
}

function closeStream() {
  if (stream) {
    stream.onclose = null;
    stream.close();
    stream = null;
  }
}

function scheduleReconnect(runId) {
  window.clearTimeout(reconnectTimer);
  reconnectTimer = window.setTimeout(() => {
    if (currentMonitor?.run.runtimeActive && currentRunId === runId) {
      connectStream(runId);
    }
  }, 1500);
}

function connectStream(runId) {
  closeStream();
  if (!currentMonitor?.run.runtimeActive || currentRunId !== runId) {
    return;
  }

  setConnection("connecting", `Opening stream for Run #${runId}`);
  const streamPath = activeAdapter.urls.stream(runId);
  if (!streamPath) {
    setConnection("error", activeAdapter.label + " stream adapter is unavailable.");
    return;
  }
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  stream = new WebSocket(protocol + "//" + window.location.host + streamPath);

  stream.addEventListener("open", () => {
    setConnection("streaming", `Authenticated Run #${runId} stream`);
  });

  stream.addEventListener("message", (event) => {
    try {
      const message = JSON.parse(event.data);
      if ((message.type === "snapshot" || message.type === "update") && message.data) {
        renderSnapshot(message.data);
      }
    } catch {
      setConnection("error", "Received an invalid runtime message");
    }
  });

  stream.addEventListener("close", () => {
    stream = null;
    if (currentMonitor?.run.runtimeActive && currentRunId === runId) {
      setConnection("reconnecting", `Run #${runId} stream disconnected`);
      scheduleReconnect(runId);
    } else {
      setConnection("connected", "Protected " + activeAdapter.label + " state loaded");
    }
  });

  stream.addEventListener("error", () => {
    setConnection("reconnecting", `Run #${runId} stream error`);
  });
}

symbolInput.addEventListener("change", () => {
  populateMarketOptions(symbolInput.value, marketTypeInput.value);
  handleBotDraftChange();
  if (!currentMonitor?.run.runtimeActive) syncMarketStreamToSelection(true);
});
marketTypeInput.addEventListener("change", () => {
  handleBotDraftChange();
  if (!currentMonitor?.run.runtimeActive) syncMarketStreamToSelection(true);
});
anchorInput.addEventListener("change", () => {
  syncFixedAnchorState();
  handleBotDraftChange();
});
for (const control of document.querySelectorAll("[data-trading-config]")) {
  if (control === symbolInput || control === marketTypeInput || control === anchorInput) continue;
  control.addEventListener("input", handleBotDraftChange);
  control.addEventListener("change", handleBotDraftChange);
}
botNameInput.addEventListener("input", handleBotDraftChange);
newBotButton.addEventListener("click", openNewBotDraft);
saveBotButton.addEventListener("click", () => void saveCurrentBot());
paperModeButton.addEventListener("click", () => {
  if (!currentMonitor?.run.runtimeActive) setTradingMode("paper");
});
liveModeButton.addEventListener("click", () => {
  const live = TradingContract.adapterFor("live");
  if (!live.locked && !currentMonitor?.run.runtimeActive) setTradingMode("live");
});

tradingForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (currentMonitor?.run.runtimeActive || activeAdapter.locked) return;
  showControlError("");
  try {
    if (!selectedBotId || botDraftMode) throw new Error("Save the Bot before starting Live-Paper.");
    if (hasUnsavedBotChanges()) throw new Error("Save Bot changes before starting Live-Paper.");
    if (anyActiveLivePaperRun() && !activeRunForBot(selectedBotId)) {
      throw new Error("Another Bot is already running Live-Paper. Concurrent Bot runtimes are added in Phase 3.");
    }
    startButton.disabled = true;
    setControlStatus("Starting Bot #" + selectedBotId + " · Live-Paper on Render…");
    const startUrl = activeAdapter.urls.start();
    if (!startUrl) throw new Error(activeAdapter.label + " start adapter is unavailable.");
    const snapshot = await requestJson(startUrl, {
      method: "POST",
      body: JSON.stringify(activeAdapter.buildStartPayload({
        bot_id: selectedBotId,
        ...buildStartConfiguration(),
      })),
    });
    updateRunSummaryFromSnapshot(snapshot);
    renderSnapshot(snapshot);
    renderBotStrip();
    setConnection("connected", "Bot #" + selectedBotId + " · Run #" + snapshot.run_id + " created");
    connectStream(snapshot.run_id);
  } catch (error) {
    showControlError(error.message);
    setConfigLocked(false);
    setControlStatus("Live-Paper run was not started.");
  }
});

auditLoadAllButton.addEventListener("click", () => {
  if (currentRunId) void loadCompleteAudit(currentRunId);
});

stopButton.addEventListener("click", async () => {
  if (!currentMonitor?.run.runtimeActive || !currentRunId) return;
  showControlError("");
  stopButton.disabled = true;
  setControlStatus("Stopping Run #" + currentRunId + "…");
  try {
    const stopUrl = activeAdapter.urls.stop(currentRunId);
    if (!stopUrl) throw new Error(activeAdapter.label + " stop adapter is unavailable.");
    const snapshot = await requestJson(stopUrl, { method: "POST" });
    const stoppedRunId = snapshot.run_id;
    closeStream();
    updateRunSummaryFromSnapshot(snapshot);
    renderBotStrip();
    renderIdleBotWorkspace("Run #" + stoppedRunId + " stopped. Bot #" + selectedBotId + " remains saved and idle.");
    setConnection("connected", "Bot #" + selectedBotId + " idle · Run #" + stoppedRunId + " stopped");
  } catch (error) {
    showControlError(error.message);
    setConfigLocked(true);
    setControlStatus("Run #" + currentRunId + " is still treated as active until backend state confirms otherwise.");
  }
});

themeToggle.addEventListener("click", () => {
  const nextTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  setTheme(nextTheme);
  tradingChart?.applyTheme();
  renderRunOverlays();
});

async function initializeTradingPage() {
  await loadRegisteredInstruments();
  await initializeBotWorkspace();
  syncMarketStreamToSelection(false);
}

initializeTheme();
setTradingMode("paper");
ensureTradingChart();
syncFixedAnchorState();
void initializeTradingPage();
