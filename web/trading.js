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
    text: styles.getPropertyValue("--text").trim(),
    muted: styles.getPropertyValue("--muted").trim(),
    border: styles.getPropertyValue("--border").trim(),
    grid: styles.getPropertyValue("--grid").trim(),
    bid: styles.getPropertyValue("--bid").trim(),
    ask: styles.getPropertyValue("--ask").trim(),
    surface: styles.getPropertyValue("--surface").trim(),
  };
}

function numeric(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function setChartStatus(message) {
  tradingChartStatus.textContent = message;
}

function ensureTradingChart() {
  if (!tradingChartElement || !window.echarts) {
    setChartStatus("Chart library is unavailable.");
    return null;
  }
  if (!tradingChart) {
    tradingChart = window.echarts.init(tradingChartElement, null, { renderer: "canvas" });
  }
  return tradingChart;
}

function resetTradingChart(message = "No Paper run selected.") {
  chartBootstrapToken += 1;
  chartRunId = null;
  chartCandles = [];
  chartOrderLevels = new Map();
  chartFillMarkers = new Map();
  if (tradingChart) tradingChart.clear();
  tradingChartBadge.textContent = "Binance 1m";
  setChartStatus(message);
}

function normalizeOrderLevel(order) {
  const price = numeric(order.price);
  const quantity = numeric(order.quantity ?? order.original_quantity ?? order.originalQuantity);
  const activeFrom = numeric(order.active_from_ms ?? order.submitted_at_ms ?? order.submittedAtMs);
  if (price === null || activeFrom === null) return null;
  return {
    order_id: Number(order.order_id ?? order.id),
    side: String(order.side || "").toLowerCase(),
    price,
    quantity: quantity ?? 0,
    active_from_ms: activeFrom,
    active_to_ms: numeric(order.active_to_ms),
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
  if (!candle) return;
  const normalized = {
    open_time_ms: numeric(candle.open_time_ms),
    close_time_ms: numeric(candle.close_time_ms),
    open: numeric(candle.open),
    high: numeric(candle.high),
    low: numeric(candle.low),
    close: numeric(candle.close),
    volume: numeric(candle.volume) ?? 0,
    is_closed: Boolean(candle.is_closed),
  };
  if ([normalized.open_time_ms, normalized.close_time_ms, normalized.open, normalized.high, normalized.low, normalized.close].some((value) => value === null)) return;
  const existing = chartCandles.findIndex((item) => item.open_time_ms === normalized.open_time_ms);
  if (existing >= 0) {
    chartCandles[existing] = normalized;
  } else {
    chartCandles.push(normalized);
    chartCandles.sort((a, b) => a.open_time_ms - b.open_time_ms);
    while (chartCandles.length > 1000) chartCandles.shift();
  }
}

function candleIndexForTime(eventTimeMs) {
  if (!chartCandles.length) return 0;
  let candidate = 0;
  for (let index = 0; index < chartCandles.length; index += 1) {
    if (chartCandles[index].open_time_ms <= eventTimeMs) candidate = index;
    else break;
  }
  return candidate;
}

function utcLabel(timestamp) {
  return new Intl.DateTimeFormat("en", {
    timeZone: "UTC",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(new Date(timestamp));
}

function renderTradingChart() {
  const chart = ensureTradingChart();
  if (!chart) return;
  if (!chartCandles.length) {
    chart.clear();
    const overlayCount = chartOrderLevels.size;
    const fillCount = chartFillMarkers.size;
    setChartStatus(
      currentMonitor?.run.runtimeActive
        ? "Waiting for Binance 1-minute candles."
        : "No live candle window is retained for this inactive run · " + overlayCount + " persisted order levels · " + fillCount + " persisted fills."
    );
    return;
  }

  const palette = chartPalette();
  const categories = chartCandles.map((candle) => utcLabel(candle.open_time_ms));
  const candleData = chartCandles.map((candle) => [candle.open, candle.close, candle.low, candle.high]);
  const lastIndex = chartCandles.length - 1;

  const orderSegments = Array.from(chartOrderLevels.values())
    .filter((order) => Number.isFinite(order.price))
    .map((order) => ({
      name: (order.side === "buy" ? "Buy" : "Sell") + " order #" + order.order_id,
      value: [
        candleIndexForTime(order.active_from_ms),
        candleIndexForTime(order.active_to_ms ?? chartCandles[lastIndex].close_time_ms),
        order.price,
        order.side === "buy" ? 0 : 1,
        order.order_id,
        order.active_from_ms,
        order.active_to_ms ?? 0,
      ],
    }));

  const buyFills = [];
  const sellFills = [];
  for (const fill of chartFillMarkers.values()) {
    const point = {
      name: (fill.side === "buy" ? "Buy" : "Sell") + " fill #" + fill.order_id,
      value: [candleIndexForTime(fill.event_time_ms), fill.price, fill.order_id, fill.event_time_ms, fill.quantity],
    };
    (fill.side === "buy" ? buyFills : sellFills).push(point);
  }

  const visibleStart = chartCandles.length > 240 ? Math.max(0, 100 - (240 / chartCandles.length) * 100) : 0;
  chart.setOption({
    animation: false,
    backgroundColor: "transparent",
    textStyle: { color: palette.text },
    grid: { left: 68, right: 24, top: 24, bottom: 78 },
    tooltip: {
      trigger: "item",
      backgroundColor: palette.surface,
      borderColor: palette.border,
      textStyle: { color: palette.text },
      formatter(params) {
        if (params.seriesName === "Candles") {
          const candle = chartCandles[params.dataIndex];
          return [
            "<strong>" + utcLabel(candle.open_time_ms) + " UTC</strong>",
            "Open: " + candle.open,
            "High: " + candle.high,
            "Low: " + candle.low,
            "Close: " + candle.close,
          ].join("<br>");
        }
        if (params.seriesName === "Order lifetime") {
          const value = params.value;
          const side = value[3] === 0 ? "Buy" : "Sell";
          return [
            "<strong>" + side + " order #" + value[4] + "</strong>",
            "Price: " + value[2],
            "Active from: " + utcLabel(value[5]) + " UTC",
            value[6] ? "Active to: " + utcLabel(value[6]) + " UTC" : "Active now",
          ].join("<br>");
        }
        const value = params.value;
        return [
          "<strong>" + params.seriesName + " · order #" + value[2] + "</strong>",
          "Price: " + value[1],
          "Quantity: " + value[4],
          "Time: " + utcLabel(value[3]) + " UTC",
        ].join("<br>");
      },
    },
    xAxis: {
      type: "category",
      data: categories,
      boundaryGap: true,
      axisLine: { lineStyle: { color: palette.border } },
      axisLabel: { color: palette.muted, hideOverlap: true },
      splitLine: { show: false },
    },
    yAxis: {
      type: "value",
      scale: true,
      name: "Quote price",
      nameTextStyle: { color: palette.muted },
      axisLine: { show: true, lineStyle: { color: palette.border } },
      axisLabel: { color: palette.muted },
      splitLine: { lineStyle: { color: palette.grid } },
    },
    dataZoom: [
      { type: "inside", start: visibleStart, end: 100, filterMode: "none" },
      { type: "slider", start: visibleStart, end: 100, height: 22, bottom: 24, filterMode: "none" },
    ],
    series: [
      {
        name: "Candles",
        type: "candlestick",
        data: candleData,
        itemStyle: {
          color: palette.bid,
          color0: palette.ask,
          borderColor: palette.bid,
          borderColor0: palette.ask,
        },
      },
      {
        name: "Order lifetime",
        type: "custom",
        data: orderSegments,
        encode: { x: [0, 1], y: 2 },
        renderItem(params, api) {
          const start = api.coord([api.value(0), api.value(2)]);
          const end = api.coord([api.value(1), api.value(2)]);
          const buy = api.value(3) === 0;
          return {
            type: "line",
            shape: { x1: start[0], y1: start[1], x2: end[0], y2: end[1] },
            style: {
              stroke: buy ? palette.bid : palette.ask,
              lineWidth: 2,
              lineDash: [7, 4],
              opacity: 0.9,
            },
          };
        },
      },
      {
        name: "Buy fills",
        type: "scatter",
        data: buyFills,
        symbol: "triangle",
        symbolSize: 13,
        itemStyle: { color: palette.bid },
      },
      {
        name: "Sell fills",
        type: "scatter",
        data: sellFills,
        symbol: "triangle",
        symbolRotate: 180,
        symbolSize: 13,
        itemStyle: { color: palette.ask },
      },
    ],
  }, true);

  tradingChartBadge.textContent = "Binance 1m · " + (currentMonitor?.market.symbol || "market");
  setChartStatus(
    chartCandles.length + " live-feed candles · " + chartOrderLevels.size + " canonical order levels · " + chartFillMarkers.size + " persisted fills · UTC"
  );
}

async function loadChartBootstrap(runId, quiet = false) {
  const token = ++chartBootstrapToken;
  if (!quiet) setChartStatus("Loading chart state for Run #" + runId + "…");
  chartRefreshInFlight = true;
  try {
    const chartUrl = activeAdapter.urls.chart(runId);
    if (!chartUrl) throw new Error(activeAdapter.label + " chart adapter is unavailable.");
    const payload = await fetchJson(chartUrl);
    if (token !== chartBootstrapToken || currentRunId !== runId) return;
    chartRunId = runId;
    chartCandles = [];
    for (const candle of payload.candles ?? []) upsertChartCandle(candle);
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
      setChartStatus("Could not load chart state · " + error.message);
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

  if (monitor.market.latestBaseCandle) upsertChartCandle(monitor.market.latestBaseCandle);

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

  renderTradingChart();
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

function resetAudit(message = "No Paper run selected.") {
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
  configLockBadge.textContent = locked ? "Locked · active run" : "Editable";
  startButton.disabled = locked;
  stopButton.disabled = !locked;
  paperModeButton.disabled = locked || activeMode !== "paper";
  liveModeButton.disabled = TradingContract.adapterFor("live").locked || locked;
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
  symbolInput.value = snapshot.symbol || symbolInput.value;
  marketTypeInput.value = snapshot.market_type || marketTypeInput.value;
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
  const anchor = anchorInput.value;
  const fixedAnchorPrice = anchor === "fixed" ? numberValue(fixedAnchorInput, "Fixed anchor price") : null;
  return {
    symbol,
    market_type: marketTypeInput.value,
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

function renderNoRun() {
  currentSnapshot = null;
  currentMonitor = null;
  currentRunId = null;
  runIdElement.textContent = "No run";
  runStatusElement.textContent = "No " + activeAdapter.label + " run has been created yet.";
  runBadgeElement.textContent = "Idle";
  setDot(feedDot, "pending");
  feedLabel.textContent = "Inactive";
  feedDetail.textContent = "No backend " + activeAdapter.label + " runtime selected";
  setDot(topDot, "pending");
  topLabel.textContent = activeAdapter.label + " mode";
  symbolElement.textContent = "—";
  marketTypeElement.textContent = "—";
  intervalElement.textContent = "—";
  strategyElement.textContent = "—";
  setConfigLocked(false);
  setControlStatus(activeAdapter.locked ? activeAdapter.lockReason : "Ready to start a backend " + activeAdapter.label + " run.");
  resetTradingChart("No Paper run selected.");
  portfolioBadge.textContent = "Snapshot";
  for (const element of [portfolioPosition, portfolioCash, portfolioEquity, portfolioExposure, portfolioRealized, portfolioUnrealized, portfolioFees, portfolioMark]) {
    element.textContent = "—";
  }
  ordersBadge.textContent = "0 open";
  ordersBody.replaceChildren(emptyTableRow(8, "No active orders."));
  ordersStatus.textContent = "No trading run selected.";
  resetAudit("No trading run selected.");
}

function renderSnapshot(snapshot) {
  const monitor = TradingContract.normalizeSnapshot(snapshot);
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
  if (monitor.run.mode === "paper") applySnapshotToConfig(snapshot);
  setConfigLocked(Boolean(monitor.run.runtimeActive));
  setControlStatus(monitor.run.runtimeActive
    ? ("Run #" + monitor.run.id + " is active. Stop it before changing " + activeAdapter.label + " setup.")
    : (activeAdapter.locked
      ? activeAdapter.lockReason
      : "Loaded persisted Run #" + monitor.run.id + ". " + activeAdapter.label + " setup is editable for the next run."));
  syncChartFromMonitor(monitor);
  renderPortfolio(monitor);
  renderOpenOrders(monitor);
  syncAuditFromMonitor(monitor);

  if (!monitor.run.runtimeActive) {
    setDot(feedDot, "pending");
    feedLabel.textContent = "Runtime inactive";
    feedDetail.textContent = "Persisted state only · no live feed claimed";
    setDot(topDot, "pending");
    topLabel.textContent = activeAdapter.label + " · inactive";
    return;
  }

  const feedState = monitor.market.feedStatus || "loading";
  setDot(feedDot, feedState);
  feedLabel.textContent = humanize(feedState);
  feedDetail.textContent = monitor.market.symbol
    ? monitor.market.symbol + " · Binance public 1m base feed"
    : "Binance public market data";
  setDot(topDot, feedState === "live" ? "live" : feedState);
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

function selectRun(runs) {
  return TradingContract.selectRun(runs);
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

async function initializeTradingStatus() {
  setConnection("connecting", "Reading protected " + activeAdapter.label + " state");
  try {
    const runsUrl = activeAdapter.urls.runs(100);
    if (!runsUrl) throw new Error(activeAdapter.label + " runtime adapter is unavailable.");
    const listing = await fetchJson(runsUrl);
    const selected = selectRun(listing.runs ?? []);
    if (!selected) {
      renderNoRun();
      setConnection("connected", "Protected Trading API available");
      return;
    }

    const snapshotUrl = activeAdapter.urls.snapshot(selected.run_id);
    if (!snapshotUrl) throw new Error(activeAdapter.label + " snapshot adapter is unavailable.");
    const snapshot = await fetchJson(snapshotUrl);
    renderSnapshot(snapshot);
    setConnection("connected", `Loaded Run #${selected.run_id}`);
    if (snapshot.runtime_active) {
      connectStream(snapshot.run_id);
    }
  } catch (error) {
    renderNoRun();
    setConnection("error", `Could not read Trading API · ${error.message}`);
  }
}

anchorInput.addEventListener("change", syncFixedAnchorState);
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
  startButton.disabled = true;
  setControlStatus("Starting " + activeAdapter.label + " runtime on Render…");
  try {
    const startUrl = activeAdapter.urls.start();
    if (!startUrl) throw new Error(activeAdapter.label + " start adapter is unavailable.");
    const snapshot = await requestJson(startUrl, {
      method: "POST",
      body: JSON.stringify(activeAdapter.buildStartPayload(buildStartConfiguration())),
    });
    renderSnapshot(snapshot);
    setConnection("connected", "Created Run #" + snapshot.run_id);
    connectStream(snapshot.run_id);
  } catch (error) {
    showControlError(error.message);
    setConfigLocked(false);
    setControlStatus(activeAdapter.label + " run was not started.");
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
    closeStream();
    renderSnapshot(snapshot);
    setConnection("connected", "Run #" + snapshot.run_id + " stopped; persisted state loaded");
  } catch (error) {
    showControlError(error.message);
    setConfigLocked(true);
    setControlStatus("Run #" + currentRunId + " is still treated as active until backend state confirms otherwise.");
  }
});

themeToggle.addEventListener("click", () => {
  const nextTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  setTheme(nextTheme);
  renderTradingChart();
});

if (tradingChartElement && "ResizeObserver" in window) {
  new ResizeObserver(() => tradingChart?.resize()).observe(tradingChartElement);
}

initializeTheme();
setTradingMode("paper");
ensureTradingChart();
syncFixedAnchorState();
setConfigLocked(false);
initializeTradingStatus();
