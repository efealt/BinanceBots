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

let stream = null;
let reconnectTimer = null;
let currentRunId = null;
let currentSnapshot = null;
let tradingChart = null;
let chartRunId = null;
let chartCandles = [];
let chartOrderLevels = new Map();
let chartFillMarkers = new Map();
let chartBootstrapToken = 0;
let chartRefreshInFlight = false;

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
  const quantity = numeric(order.quantity ?? order.original_quantity);
  const activeFrom = numeric(order.active_from_ms ?? order.submitted_at_ms);
  if (price === null || activeFrom === null) return null;
  return {
    order_id: Number(order.order_id),
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
  const eventTime = numeric(fill.event_time_ms);
  if (price === null || eventTime === null) return null;
  return {
    order_id: Number(fill.order_id),
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
      currentSnapshot?.runtime_active
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

  tradingChartBadge.textContent = "Binance 1m · " + (currentSnapshot?.symbol || "market");
  setChartStatus(
    chartCandles.length + " live-feed candles · " + chartOrderLevels.size + " canonical order levels · " + chartFillMarkers.size + " persisted fills · UTC"
  );
}

async function loadChartBootstrap(runId, quiet = false) {
  const token = ++chartBootstrapToken;
  if (!quiet) setChartStatus("Loading chart state for Run #" + runId + "…");
  chartRefreshInFlight = true;
  try {
    const payload = await fetchJson("/api/trading/runs/" + runId + "/chart");
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

function syncChartFromSnapshot(snapshot) {
  if (!snapshot?.run_id) return;
  if (chartRunId !== snapshot.run_id) {
    void loadChartBootstrap(snapshot.run_id);
    return;
  }

  if (snapshot.latest_base_candle) upsertChartCandle(snapshot.latest_base_candle);

  let overlaysChanged = false;
  const activeOrderIds = new Set();
  for (const raw of snapshot.open_orders ?? []) {
    activeOrderIds.add(Number(raw.order_id));
    if (!chartOrderLevels.has(Number(raw.order_id))) {
      const order = normalizeOrderLevel(raw);
      if (order) {
        chartOrderLevels.set(order.order_id, order);
        overlaysChanged = true;
      }
    }
  }

  for (const raw of snapshot.recent_fills ?? []) {
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
    } else if (!snapshot.runtime_active && snapshot.ended_at_ms) {
      order.active_to_ms = snapshot.ended_at_ms;
      overlaysChanged = true;
    }
  }

  renderTradingChart();
  if (overlaysChanged && !chartRefreshInFlight) {
    void loadChartBootstrap(snapshot.run_id, true);
  }
}

function showControlError(message) {
  controlError.textContent = message;
  controlError.hidden = !message;
}

function syncFixedAnchorState() {
  fixedAnchorInput.disabled = Boolean(currentSnapshot?.runtime_active) || anchorInput.value !== "fixed";
}

function setConfigLocked(locked) {
  document.querySelectorAll("[data-trading-config]").forEach((control) => {
    control.disabled = locked;
  });
  configLockBadge.textContent = locked ? "Locked · active run" : "Editable";
  startButton.disabled = locked;
  stopButton.disabled = !locked;
  paperModeButton.disabled = locked;
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

function buildStartRequest() {
  const symbol = symbolInput.value.trim().toUpperCase();
  if (!/^[A-Z0-9]+$/.test(symbol)) {
    throw new Error("Symbol must contain only letters and numbers.");
  }
  const anchor = anchorInput.value;
  const fixedAnchorPrice = anchor === "fixed" ? numberValue(fixedAnchorInput, "Fixed anchor price") : null;
  return {
    mode: "paper",
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
  currentRunId = null;
  runIdElement.textContent = "No run";
  runStatusElement.textContent = "No Paper run has been created yet.";
  runBadgeElement.textContent = "Idle";
  setDot(feedDot, "pending");
  feedLabel.textContent = "Inactive";
  feedDetail.textContent = "No backend Paper runtime selected";
  setDot(topDot, "pending");
  topLabel.textContent = "Paper mode";
  symbolElement.textContent = "—";
  marketTypeElement.textContent = "—";
  intervalElement.textContent = "—";
  strategyElement.textContent = "—";
  setConfigLocked(false);
  setControlStatus("Ready to start a backend Paper run.");
  resetTradingChart("No Paper run selected.");
}

function renderSnapshot(snapshot) {
  currentSnapshot = snapshot;
  currentRunId = snapshot.run_id;
  runIdElement.textContent = `Run #${snapshot.run_id}`;
  runStatusElement.textContent = snapshot.runtime_active
    ? `Backend runtime · ${humanize(snapshot.runtime_status)}`
    : `Persisted run · ${humanize(snapshot.canonical_status)}`;
  runBadgeElement.textContent = humanize(snapshot.runtime_status || snapshot.canonical_status);

  symbolElement.textContent = snapshot.symbol || "—";
  marketTypeElement.textContent = humanize(snapshot.market_type) || "—";
  intervalElement.textContent = snapshot.replay_interval || "—";
  strategyElement.textContent = snapshot.strategy_id || "—";
  applySnapshotToConfig(snapshot);
  setConfigLocked(Boolean(snapshot.runtime_active));
  setControlStatus(snapshot.runtime_active
    ? ("Run #" + snapshot.run_id + " is active. Stop it before changing configuration.")
    : ("Loaded persisted Run #" + snapshot.run_id + ". Configuration is editable for the next Paper run."));
  syncChartFromSnapshot(snapshot);

  if (!snapshot.runtime_active) {
    setDot(feedDot, "pending");
    feedLabel.textContent = "Runtime inactive";
    feedDetail.textContent = "Persisted state only · no live feed claimed";
    setDot(topDot, "pending");
    topLabel.textContent = "Paper · inactive";
    return;
  }

  const feedState = snapshot.feed_status || "loading";
  setDot(feedDot, feedState);
  feedLabel.textContent = humanize(feedState);
  feedDetail.textContent = snapshot.symbol
    ? `${snapshot.symbol} · Binance public 1m base feed`
    : "Binance public market data";
  setDot(topDot, feedState === "live" ? "live" : feedState);
  topLabel.textContent = `Paper · ${humanize(snapshot.runtime_status)}`;
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
  return runs.find((run) => ["arming", "running"].includes(run.runtime_status)) ?? runs[0] ?? null;
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
    if (currentSnapshot?.runtime_active && currentRunId === runId) {
      connectStream(runId);
    }
  }, 1500);
}

function connectStream(runId) {
  closeStream();
  if (!currentSnapshot?.runtime_active || currentRunId !== runId) {
    return;
  }

  setConnection("connecting", `Opening stream for Run #${runId}`);
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  stream = new WebSocket(`${protocol}//${window.location.host}/api/trading/runs/${runId}/stream`);

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
    if (currentSnapshot?.runtime_active && currentRunId === runId) {
      setConnection("reconnecting", `Run #${runId} stream disconnected`);
      scheduleReconnect(runId);
    } else {
      setConnection("connected", "Protected Paper state loaded");
    }
  });

  stream.addEventListener("error", () => {
    setConnection("reconnecting", `Run #${runId} stream error`);
  });
}

async function initializeTradingStatus() {
  setConnection("connecting", "Reading protected Paper state");
  try {
    const listing = await fetchJson("/api/trading/runs?limit=100");
    const selected = selectRun(listing.runs ?? []);
    if (!selected) {
      renderNoRun();
      setConnection("connected", "Protected Trading API available");
      return;
    }

    const snapshot = await fetchJson(`/api/trading/runs/${selected.run_id}`);
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

tradingForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (currentSnapshot?.runtime_active) return;
  showControlError("");
  startButton.disabled = true;
  setControlStatus("Starting Paper runtime on Render…");
  try {
    const snapshot = await requestJson("/api/trading/runs", {
      method: "POST",
      body: JSON.stringify(buildStartRequest()),
    });
    renderSnapshot(snapshot);
    setConnection("connected", "Created Run #" + snapshot.run_id);
    connectStream(snapshot.run_id);
  } catch (error) {
    showControlError(error.message);
    setConfigLocked(false);
    setControlStatus("Paper run was not started.");
  }
});

stopButton.addEventListener("click", async () => {
  if (!currentSnapshot?.runtime_active || !currentRunId) return;
  showControlError("");
  stopButton.disabled = true;
  setControlStatus("Stopping Run #" + currentRunId + "…");
  try {
    const snapshot = await requestJson("/api/trading/runs/" + currentRunId + "/stop", { method: "POST" });
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
ensureTradingChart();
syncFixedAnchorState();
setConfigLocked(false);
initializeTradingStatus();
