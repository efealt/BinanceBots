const themeToggle = document.querySelector("#theme-toggle");
const datasetSelect = document.querySelector("#backtest-dataset");
const timeframeSelect = document.querySelector("#backtest-timeframe");
const weekendOverlayToggle = document.querySelector("#backtest-weekend-overlay");
const afterHoursOverlayToggle = document.querySelector("#backtest-after-hours-overlay");
const afterHoursLabel = document.querySelector("#backtest-after-hours-label");
const dataStatus = document.querySelector("#backtest-data-status");
const chartEmpty = document.querySelector("#backtest-data-empty");
const diagnosticsSection = document.querySelector(".diagnostics-section");
const diagnosticsToggle = document.querySelector("#diagnostics-toggle");
const diagnosticsContent = document.querySelector("#diagnostics-content");
const diagnosticsContext = document.querySelector("#diagnostics-context");
const diagnosticsStatus = document.querySelector("#diagnostics-status");
const diagnosticsRows = document.querySelector("#diagnostics-rows");
const rollingPredictionSection = document.querySelector(".rolling-prediction-section");
const rollingPredictionToggle = document.querySelector("#rolling-prediction-toggle");
const rollingPredictionContent = document.querySelector("#rolling-prediction-content");
const rollingPredictionCard = document.querySelector("#rolling-prediction-card");
const rollingPredictionStatus = document.querySelector("#rolling-prediction-status");
const backtestRunForm = document.querySelector("#backtest-run-form");
const backtestRunContext = document.querySelector("#backtest-run-context");
const backtestStrategy = document.querySelector("#backtest-strategy");
const backtestInitialCapital = document.querySelector("#backtest-initial-capital");
const backtestStartDate = document.querySelector("#backtest-start-date");
const backtestEndDate = document.querySelector("#backtest-end-date");
const backtestGridAnchor = document.querySelector("#backtest-grid-anchor");
const backtestFixedAnchor = document.querySelector("#backtest-fixed-anchor");
const backtestGridSpacing = document.querySelector("#backtest-grid-spacing");
const backtestGridLevels = document.querySelector("#backtest-grid-levels");
const backtestGridQuantity = document.querySelector("#backtest-grid-quantity");
const backtestFeeBps = document.querySelector("#backtest-fee-bps");
const backtestSpreadBps = document.querySelector("#backtest-spread-bps");
const backtestSlippageBps = document.querySelector("#backtest-slippage-bps");
const backtestLatencyMs = document.querySelector("#backtest-latency-ms");
const backtestLimitPolicy = document.querySelector("#backtest-limit-policy");
const backtestPartialFill = document.querySelector("#backtest-partial-fill");
const backtestFormError = document.querySelector("#backtest-form-error");
const runBacktestButton = document.querySelector("#run-backtest-button");
const backtestRunStatusBadge = document.querySelector("#backtest-run-status-badge");
const backtestProgressMessage = document.querySelector("#backtest-progress-message");
const backtestProgressPercent = document.querySelector("#backtest-progress-percent");
const backtestProgressTrack = document.querySelector(".backtest-progress-track");
const backtestProgressFill = document.querySelector("#backtest-progress-fill");
const backtestResultEmpty = document.querySelector("#backtest-result-empty");
const backtestResultContent = document.querySelector("#backtest-result-content");
const backtestKpiGrid = document.querySelector("#backtest-kpi-grid");
const backtestResultContext = document.querySelector("#backtest-result-context");
const backtestFillAuditCount = document.querySelector("#backtest-fill-audit-count");
const backtestFillTableBody = document.querySelector("#backtest-fill-table-body");
const TIMEFRAME_MS = { "1m": 60_000, "1h": 3_600_000, "1d": 86_400_000 };
const TIMEFRAME_LABELS = { "1m": "1 minute", "1h": "1 hour", "1d": "1 day" };
const ROLLING_WINDOW_SIZE = 30;
const ROLLING_SCOPE_LABELS = { full: "Full sample", weekdays: "Weekdays", weekends: "Weekends" };
const DIAGNOSTIC_WINDOWS = {
  postNewYork: { startHour: 21, endHour: 24, label: "21:00–00:00 UTC" },
  middleAsia: { startHour: 2, endHour: 5, label: "02:00–05:00 UTC" },
};
const DEFAULT_AFTER_HOURS_SCHEDULE = {
  startHour: 16,
  endHour: 20,
  label: "After-hours · 16:00–20:00 UTC",
};
const METAL_AFTER_HOURS_SCHEDULE = {
  startHour: 22,
  endHour: 7,
  label: "After-hours · 22:00–07:00 UTC",
};
const METAL_SYMBOL_PREFIXES = ["XAG", "XAU", "XPT", "XPD"];
const OVERLAY_PREFERENCES = {
  weekend: "backtest-overlay-weekend",
  afterHours: "backtest-overlay-after-hours",
};

class BacktestDataChart {
  constructor(container) { this.container = container; this.chart = null; this.series = null; this.volumeSeries = null; }
  initialize() {
    this.chart = LightweightCharts.createChart(this.container, this.options());
    this.series = this.chart.addSeries(LightweightCharts.CandlestickSeries, {
      upColor: "#36c984", downColor: "#eb6f92", borderVisible: false,
      wickUpColor: "#36c984", wickDownColor: "#eb6f92",
    });
    this.volumeSeries = this.chart.addSeries(LightweightCharts.HistogramSeries, {
      priceFormat: { type: "volume" },
      priceScaleId: "",
    }, 1);
    this.chart.panes()[1]?.setHeight(150);
    new ResizeObserver(([entry]) => this.chart.applyOptions({ width: entry.contentRect.width, height: entry.contentRect.height })).observe(this.container);
  }
  setCandles(candles) {
    this.series.setData(candles.map((candle) => ({
      time: Math.floor(candle.open_time_ms / 1000), open: candle.open_price,
      high: candle.high_price, low: candle.low_price, close: candle.close_price,
    })));
    this.volumeSeries.setData(candles.map((candle) => ({
      time: Math.floor(candle.open_time_ms / 1000),
      value: candle.base_volume,
      color: candle.close_price >= candle.open_price ? "rgba(54, 201, 132, 0.72)" : "rgba(235, 111, 146, 0.72)",
    })));
    this.chart.timeScale().fitContent();
  }
  reset() { this.series.setData([]); this.volumeSeries.setData([]); }
  applyTheme() { this.chart.applyOptions(this.options()); }
  options() {
    const styles = getComputedStyle(document.documentElement);
    const surface = styles.getPropertyValue("--surface-raised").trim();
    const text = styles.getPropertyValue("--muted").trim();
    const grid = styles.getPropertyValue("--border").trim();
    return {
      autoSize: true,
      layout: { background: { type: "solid", color: surface }, textColor: text },
      panes: { separatorColor: grid, separatorHoverColor: grid, enableResize: true },
      grid: { vertLines: { color: grid }, horzLines: { color: grid } },
      crosshair: { mode: LightweightCharts.CrosshairMode.Normal },
      rightPriceScale: { borderColor: grid },
      timeScale: { borderColor: grid, timeVisible: true, secondsVisible: false },
    };
  }
}

const chart = new BacktestDataChart(document.querySelector("#backtest-data-chart"));
const weekendOverlay = new WeekendOverlay(document.querySelector("#backtest-data-chart"));
const afterHoursOverlay = new AfterHoursOverlay(
  document.querySelector("#backtest-data-chart"),
  DEFAULT_AFTER_HOURS_SCHEDULE,
);
let rawCandles = [];
let datasetsById = new Map();
let diagnosticCharts = [];
let diagnosticCandles = [];
let rollingPredictionScope = "full";

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("theme", theme);
  chart.applyTheme();
  refreshDiagnosticCharts();
}

function initializeTheme() {
  const savedTheme = localStorage.getItem("theme");
  const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  setTheme(savedTheme ?? systemTheme);
}

function initializeOverlayPreferences() {
  weekendOverlayToggle.checked = localStorage.getItem(OVERLAY_PREFERENCES.weekend) !== "false";
  afterHoursOverlayToggle.checked = localStorage.getItem(OVERLAY_PREFERENCES.afterHours) !== "false";
  weekendOverlay.setEnabled(weekendOverlayToggle.checked);
  afterHoursOverlay.setEnabled(afterHoursOverlayToggle.checked);
}

function updateOverlayPreference(toggle, storageKey, overlay) {
  localStorage.setItem(storageKey, String(toggle.checked));
  overlay.setEnabled(toggle.checked);
}

function tickerLabel(dataset) {
  const market = dataset.market_type === "spot" ? "Spot" : "Futures · USDⓈ-M";
  return `${dataset.symbol} · ${market} · ${dataset.interval}`;
}

function afterHoursScheduleFor(dataset) {
  const symbol = dataset?.symbol?.toUpperCase() ?? "";
  return METAL_SYMBOL_PREFIXES.some((prefix) => symbol.startsWith(prefix))
    ? METAL_AFTER_HOURS_SCHEDULE
    : DEFAULT_AFTER_HOURS_SCHEDULE;
}

function updateAfterHoursSchedule(dataset) {
  const schedule = afterHoursScheduleFor(dataset);
  afterHoursLabel.textContent = schedule.label;
  afterHoursOverlayToggle.setAttribute("aria-label", schedule.label);
  afterHoursOverlay.setSchedule(schedule);
}

function aggregateCandles(candles, interval) {
  const intervalMs = TIMEFRAME_MS[interval];
  if (intervalMs === TIMEFRAME_MS["1m"]) return candles;

  const aggregated = [];
  let current = null;
  for (const candle of candles) {
    const bucketTime = Math.floor(candle.open_time_ms / intervalMs) * intervalMs;
    if (!current || current.open_time_ms !== bucketTime) {
      current = {
        ...candle,
        open_time_ms: bucketTime,
        close_time_ms: candle.close_time_ms,
      };
      aggregated.push(current);
      continue;
    }
    current.close_time_ms = candle.close_time_ms;
    current.high_price = Math.max(current.high_price, candle.high_price);
    current.low_price = Math.min(current.low_price, candle.low_price);
    current.close_price = candle.close_price;
    current.base_volume += candle.base_volume;
    current.quote_volume += candle.quote_volume;
    current.trade_count += candle.trade_count;
    current.taker_buy_base_volume += candle.taker_buy_base_volume;
    current.taker_buy_quote_volume += candle.taker_buy_quote_volume;
  }
  return aggregated;
}

function formatCount(value) {
  return new Intl.NumberFormat("en-US").format(value);
}

function formatCompact(value) {
  if (!Number.isFinite(value)) return "—";
  return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 2 }).format(value);
}

function formatPercent(value, signed = true) {
  if (!Number.isFinite(value)) return "—";
  const digits = Math.abs(value) >= 1 ? 2 : 3;
  const sign = signed && value > 0 ? "+" : "";
  return `${sign}${value.toFixed(digits)}%`;
}

function clamp(value, minimum, maximum) {
  return Math.min(maximum, Math.max(minimum, value));
}

function quantileSorted(values, probability) {
  if (!values.length) return NaN;
  const index = (values.length - 1) * probability;
  const lower = Math.floor(index);
  const upper = Math.ceil(index);
  if (lower === upper) return values[lower];
  return values[lower] + (values[upper] - values[lower]) * (index - lower);
}

function summarize(values) {
  if (!values.length) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const total = values.reduce((sum, value) => sum + value, 0);
  const mean = total / values.length;
  const variance = values.reduce((sum, value) => sum + ((value - mean) ** 2), 0) / values.length;
  return {
    count: values.length,
    min: sorted[0],
    max: sorted[sorted.length - 1],
    p01: quantileSorted(sorted, 0.01),
    p05: quantileSorted(sorted, 0.05),
    p25: quantileSorted(sorted, 0.25),
    p50: quantileSorted(sorted, 0.5),
    p75: quantileSorted(sorted, 0.75),
    p90: quantileSorted(sorted, 0.9),
    p95: quantileSorted(sorted, 0.95),
    p99: quantileSorted(sorted, 0.99),
    total,
    mean,
    stddev: Math.sqrt(variance),
    positiveShare: values.filter((value) => value > 0).length / values.length,
  };
}

function isWithinDiagnosticWindow(hour, window) {
  return hour >= window.startHour && hour < window.endHour;
}

function createDiagnosticStates(interval) {
  const hasSessionResolution = interval !== "1d";
  const bucketCount = interval === "1d" ? 7 : 24;
  return [
    {
      id: "full",
      label: "Full sample",
      description: "All selected candles",
      color: "var(--accent)",
      disabled: false,
    },
    {
      id: "weekend",
      label: "Weekends",
      description: "UTC Saturday + Sunday",
      color: "var(--chart-weekend-key)",
      disabled: false,
    },
    {
      id: "trading",
      label: "Trading hours",
      description: hasSessionResolution ? "Weekdays · outside the two candidate windows" : "Unavailable at daily resolution",
      color: "var(--bid)",
      disabled: !hasSessionResolution,
    },
    {
      id: "post-new-york",
      label: "Post-New York / pre-Asia",
      description: hasSessionResolution ? `Weekdays · ${DIAGNOSTIC_WINDOWS.postNewYork.label}` : "Unavailable at daily resolution",
      color: "var(--chart-after-hours-key)",
      disabled: !hasSessionResolution,
    },
    {
      id: "middle-asia",
      label: "Middle Asia",
      description: hasSessionResolution ? `Weekdays · ${DIAGNOSTIC_WINDOWS.middleAsia.label}` : "Unavailable at daily resolution",
      color: "var(--pending)",
      disabled: !hasSessionResolution,
    },
  ].map((state) => ({
    ...state,
    count: 0,
    candles: [],
    returns: [],
    ranges: [],
    volumes: [],
    driftBuckets: Array.from({ length: bucketCount }, () => []),
  }));
}

function appendDiagnosticObservation(state, candle, bucket) {
  state.count += 1;
  state.candles.push(candle);
  const open = Number(candle.open_price);
  const close = Number(candle.close_price);
  const high = Number(candle.high_price);
  const low = Number(candle.low_price);
  const volume = Number(candle.base_volume);
  if (!Number.isFinite(open) || open === 0) return;

  const returnPct = ((close - open) / open) * 100;
  const rangePct = ((high - low) / open) * 100;
  if (Number.isFinite(returnPct)) {
    state.returns.push(returnPct);
    state.driftBuckets[bucket].push(returnPct);
  }
  if (Number.isFinite(rangePct)) state.ranges.push(rangePct);
  if (Number.isFinite(volume)) state.volumes.push(volume);
}

function prepareDiagnosticStates(candles, interval) {
  const states = createDiagnosticStates(interval);
  for (const candle of candles) {
    const timestamp = Number(candle.open_time_ms);
    if (!Number.isFinite(timestamp)) continue;
    const date = new Date(timestamp);
    const weekend = date.getUTCDay() === 0 || date.getUTCDay() === 6;
    const weekday = !weekend;
    const postNewYork = isWithinDiagnosticWindow(date.getUTCHours(), DIAGNOSTIC_WINDOWS.postNewYork);
    const middleAsia = isWithinDiagnosticWindow(date.getUTCHours(), DIAGNOSTIC_WINDOWS.middleAsia);
    const memberships = [
      true,
      weekend,
      !states[2].disabled && weekday && !postNewYork && !middleAsia,
      !states[3].disabled && weekday && postNewYork,
      !states[4].disabled && weekday && middleAsia,
    ];
    const bucket = interval === "1d" ? date.getUTCDay() === 0 ? 6 : date.getUTCDay() - 1 : date.getUTCHours();
    states.forEach((state, index) => {
      if (memberships[index]) appendDiagnosticObservation(state, candle, bucket);
    });
  }
  states.forEach((state) => {
    state.returnStats = summarize(state.returns);
    state.rangeStats = summarize(state.ranges);
    state.volumeStats = summarize(state.volumes);
    state.driftMeans = state.driftBuckets.map((bucket) => bucket.length
      ? bucket.reduce((sum, value) => sum + value, 0) / bucket.length
      : null);
  });
  return states;
}

function diagnosticDomains(states) {
  const full = states[0];
  const returnStats = full.returnStats;
  const rangeStats = full.rangeStats;
  const volumeStats = full.volumeStats;
  const returnDomain = returnStats
    ? Math.max(Math.abs(returnStats.p01), Math.abs(returnStats.p99), 0.01)
    : 0.01;
  const rangeDomain = rangeStats ? Math.max(rangeStats.p99, 0.01) : 0.01;
  const volumeDomain = volumeStats ? Math.max(volumeStats.p95, 0.01) : 0.01;
  const driftValues = states.flatMap((state) => state.driftBuckets.flatMap((bucket) => {
    if (!bucket.length) return [];
    const stats = summarize(bucket);
    return [stats.p05, stats.p95];
  })).filter(Number.isFinite);
  const driftDomain = driftValues.length
    ? Math.max(...driftValues.map((value) => Math.abs(value)), 0.001)
    : 0.001;
  return { returnDomain, rangeDomain, volumeDomain, driftDomain };
}

function driftBucketLabels(interval) {
  return interval === "1d"
    ? ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
    : Array.from({ length: 24 }, (_, index) => String(index).padStart(2, "0"));
}

function cumulativeReturnsByHour(observationCandles, timelineCandles = observationCandles) {
  const intervalMs = TIMEFRAME_MS["1h"];
  const candlesByTime = new Map();
  timelineCandles.forEach((candle) => {
    const timestamp = Number(candle.open_time_ms);
    if (!Number.isFinite(timestamp)) return;
    candlesByTime.set(timestamp, candle);
  });

  const returnsByHour = Array.from({ length: 24 }, () => []);
  observationCandles.forEach((candle) => {
    const timestamp = Number(candle.open_time_ms);
    const nextCandle = candlesByTime.get(timestamp + intervalMs);
    const entryPrice = Number(candle.open_price);
    const exitPrice = Number(nextCandle?.open_price);
    if (!nextCandle || !Number.isFinite(entryPrice) || entryPrice <= 0 || !Number.isFinite(exitPrice) || exitPrice <= 0) return;
    returnsByHour[new Date(timestamp).getUTCHours()].push(exitPrice / entryPrice);
  });

  return returnsByHour.map((returns, hour) => {
    const compoundedFactor = returns.reduce((factor, value) => factor * value, 1);
    return {
      cumulativeReturn: returns.length ? (compoundedFactor - 1) * 100 : null,
      hour,
      observationCount: returns.length,
    };
  });
}

function rollingObservationCandles(candles, scope) {
  if (scope === "full") return candles;
  return candles.filter((candle) => {
    const timestamp = Number(candle.open_time_ms);
    if (!Number.isFinite(timestamp)) return false;
    const day = new Date(timestamp).getUTCDay();
    const weekend = day === 0 || day === 6;
    return scope === "weekends" ? weekend : !weekend;
  });
}

function rollingPredictionByHour(observationCandles, windowSize, timelineCandles = observationCandles) {
  const intervalMs = TIMEFRAME_MS["1h"];
  const candlesByTime = new Map();
  timelineCandles.forEach((candle) => {
    const timestamp = Number(candle.open_time_ms);
    if (Number.isFinite(timestamp)) candlesByTime.set(timestamp, candle);
  });
  const returnsByHour = Array.from({ length: 24 }, () => []);
  observationCandles.forEach((candle) => {
    const timestamp = Number(candle.open_time_ms);
    const nextCandle = candlesByTime.get(timestamp + intervalMs);
    const entryPrice = Number(candle.open_price);
    const exitPrice = Number(nextCandle?.open_price);
    if (!nextCandle || !Number.isFinite(entryPrice) || entryPrice <= 0 || !Number.isFinite(exitPrice) || exitPrice <= 0) return;
    returnsByHour[new Date(timestamp).getUTCHours()].push((exitPrice / entryPrice) - 1);
  });

  return returnsByHour.map((returns, hour) => {
    let correct = 0;
    let incorrect = 0;
    for (let index = windowSize; index < returns.length; index += 1) {
      const window = returns.slice(index - windowSize, index);
      const windowAverage = window.reduce((sum, value) => sum + value, 0) / window.length;
      const predictedPositive = windowAverage > 0;
      const actualPositive = returns[index] > 0;
      if (predictedPositive === actualPositive) correct += 1;
      else incorrect += 1;
    }
    const total = correct + incorrect;
    return {
      accuracy: total ? correct / total : null,
      correct,
      hour,
      incorrect,
      total,
    };
  });
}

function diagnosticCard(title, unit, chartKey, ariaLabel, stats) {
  const statMarkup = stats.map(([label, value]) => `<span><strong>${value}</strong><small>${label}</small></span>`).join("");
  return `<article class="diagnostic-card">
    <div class="diagnostic-card-heading"><h4>${title}</h4><span>${unit}</span></div>
    <div class="diagnostic-chart" data-diagnostic-chart="${chartKey}" role="img" aria-label="${ariaLabel}"></div>
    ${statMarkup ? `<div class="diagnostic-card-stats">${statMarkup}</div>` : ""}
  </article>`;
}

function resolveDiagnosticColor(value, styles) {
  const variable = value.match(/^var\((--[^)]+)\)$/)?.[1];
  return variable ? styles.getPropertyValue(variable).trim() : value;
}

function diagnosticPalette(state) {
  const styles = getComputedStyle(document.documentElement);
  const css = (name) => styles.getPropertyValue(name).trim();
  return {
    accent: resolveDiagnosticColor(state.color, styles),
    ask: css("--ask"),
    bid: css("--bid"),
    border: css("--border"),
    grid: css("--grid"),
    muted: css("--muted"),
    surface: css("--surface-raised"),
    text: css("--text"),
  };
}

function diagnosticTooltipStyle(palette) {
  return {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderWidth: 1,
    confine: true,
    padding: [8, 10],
    textStyle: { color: palette.text, fontSize: 12 },
  };
}

function diagnosticAxisStyle(palette) {
  return {
    axisLabel: { color: palette.muted, fontSize: 10, hideOverlap: true },
    axisLine: { lineStyle: { color: palette.border } },
    axisTick: { show: false },
  };
}

function diagnosticGrid(left = 34, bottom = 30) {
  return { bottom, containLabel: true, left, right: 8, top: 8 };
}

function histogramBins(values, minimum, maximum, formatter) {
  if (!values.length) return [];
  const binCount = 14;
  const span = maximum - minimum || 1;
  const counts = Array.from({ length: binCount }, () => 0);
  values.forEach((value) => {
    const index = clamp(Math.floor(((clamp(value, minimum, maximum) - minimum) / span) * binCount), 0, binCount - 1);
    counts[index] += 1;
  });
  return counts.map((count, index) => {
    const start = minimum + (span * index) / binCount;
    const end = index === binCount - 1 ? maximum : minimum + (span * (index + 1)) / binCount;
    return {
      count,
      end,
      label: formatter((start + end) / 2),
      share: count / values.length,
      start,
    };
  });
}

function visibleHistogramLabel(index, binCount) {
  return index === 0 || index === Math.floor(binCount / 2) || index === binCount - 1;
}

function histogramChartOption(state, values, minimum, maximum, formatter, palette, valueLabel) {
  const bins = histogramBins(values, minimum, maximum, formatter);
  if (!bins.length) return null;
  const data = bins.map((bin) => ({
    value: bin.count,
    start: bin.start,
    end: bin.end,
    share: bin.share,
    binLabel: bin.label,
    itemStyle: { color: palette.accent },
  }));
  return {
    animation: false,
    grid: diagnosticGrid(),
    tooltip: {
      ...diagnosticTooltipStyle(palette),
      axisPointer: { type: "shadow" },
      formatter: (params) => {
        const item = Array.isArray(params) ? params[0] : params;
        const bin = item?.data;
        if (!bin) return "";
        return `<strong>${valueLabel}</strong><br>Range: ${formatter(bin.start)} to ${formatter(bin.end)}<br>Observations: ${formatCount(bin.value)}<br>Share: ${(bin.share * 100).toFixed(1)}%`;
      },
      trigger: "item",
    },
    xAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: {
        ...diagnosticAxisStyle(palette).axisLabel,
        formatter: (value, index) => {
          if (!visibleHistogramLabel(index, bins.length)) return "";
          if (index === 0) return formatter(minimum);
          if (index === bins.length - 1) return formatter(maximum);
          return formatter((minimum + maximum) / 2);
        },
        hideOverlap: false,
        interval: 0,
      },
      boundaryGap: true,
      data: bins.map((bin) => bin.label),
      type: "category",
    },
    yAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: {
        ...diagnosticAxisStyle(palette).axisLabel,
        formatter: (value) => formatCompact(value),
      },
      min: 0,
      name: "n",
      nameTextStyle: { color: palette.muted, fontSize: 10 },
      splitLine: { lineStyle: { color: palette.grid } },
      type: "value",
    },
    series: [{
      type: "bar",
      name: valueLabel,
      barMaxWidth: 18,
      data,
      emphasis: { focus: "series", itemStyle: { shadowBlur: 8, shadowColor: palette.accent } },
    }],
  };
}

function boxplotChartOption(state, stats, domain, palette) {
  if (!stats) return null;
  return {
    animation: false,
    grid: diagnosticGrid(12, 32),
    tooltip: {
      ...diagnosticTooltipStyle(palette),
      formatter: () => `<strong>${state.label} range</strong><br>p05: ${formatPercent(stats.p05, false)}<br>p25: ${formatPercent(stats.p25, false)}<br>Median: ${formatPercent(stats.p50, false)}<br>p75: ${formatPercent(stats.p75, false)}<br>p95: ${formatPercent(stats.p95, false)}`,
      trigger: "item",
    },
    xAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: {
        ...diagnosticAxisStyle(palette).axisLabel,
        formatter: (value) => formatPercent(value, false),
      },
      max: domain,
      min: 0,
      splitNumber: 3,
      splitLine: { lineStyle: { color: palette.grid } },
      type: "value",
    },
    yAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: { ...diagnosticAxisStyle(palette).axisLabel, show: false },
      axisLine: { show: false },
      data: ["High-low range"],
      type: "category",
    },
    series: [{
      type: "boxplot",
      name: "High-low range",
      data: [[stats.p05, stats.p25, stats.p50, stats.p75, stats.p95]],
      layout: "horizontal",
      itemStyle: { color: palette.surface, borderColor: palette.accent, borderWidth: 1.5 },
      emphasis: { itemStyle: { color: palette.accent, opacity: 0.28 } },
    }],
  };
}

function driftChartOption(state, interval, domain, palette) {
  if (!state.returnStats) return null;
  const labels = driftBucketLabels(interval);
  const visibleLabels = interval === "1d" ? labels.map((_, index) => index) : [0, 6, 12, 18, 23];
  const data = state.driftBuckets.map((values, index) => {
    if (!values.length) return "-";
    const stats = summarize(values);
    const medianColor = stats.p50 >= 0 ? palette.bid : palette.ask;
    return {
      value: [stats.p05, stats.p25, stats.p50, stats.p75, stats.p95],
      bucketLabel: labels[index],
      sampleCount: values.length,
      stats,
      itemStyle: {
        color: medianColor,
        borderColor: medianColor,
        borderWidth: 1.5,
        opacity: 0.72,
      },
    };
  });
  return {
    animation: false,
    grid: diagnosticGrid(42, 30),
    tooltip: {
      ...diagnosticTooltipStyle(palette),
      formatter: (params) => {
        const item = Array.isArray(params) ? params[0] : params;
        const bucket = item?.data;
        const stats = bucket?.stats;
        if (!stats) return "";
        const bucketLabel = interval === "1d" ? bucket.bucketLabel : `UTC ${bucket.bucketLabel}`;
        return `<strong>${bucketLabel}</strong><br>p05: ${formatPercent(stats.p05, false)}<br>p25: ${formatPercent(stats.p25, false)}<br>Median: ${formatPercent(stats.p50, false)}<br>p75: ${formatPercent(stats.p75, false)}<br>p95: ${formatPercent(stats.p95, false)}<br>Observations: ${formatCount(bucket.sampleCount)}`;
      },
      trigger: "item",
    },
    xAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: {
        ...diagnosticAxisStyle(palette).axisLabel,
        hideOverlap: false,
        interval: (index) => visibleLabels.includes(index),
      },
      data: labels,
      type: "category",
    },
    yAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: {
        ...diagnosticAxisStyle(palette).axisLabel,
        formatter: (value) => formatPercent(value, false),
      },
      max: domain,
      min: -domain,
      splitNumber: 2,
      splitLine: { lineStyle: { color: palette.grid } },
      type: "value",
    },
    series: [{
      type: "boxplot",
      name: "Return distribution",
      data,
      layout: "vertical",
      itemStyle: { color: palette.surface, borderColor: palette.accent, borderWidth: 1.5 },
      emphasis: { itemStyle: { color: palette.accent, opacity: 0.28 } },
      markLine: {
        data: [{ yAxis: 0 }],
        label: { show: false },
        lineStyle: { color: palette.border, width: 1 },
        silent: true,
        symbol: "none",
      },
    }],
  };
}

function cumulativeReturnByHourChartOption(candles, timelineCandles, palette) {
  const hourlyReturns = cumulativeReturnsByHour(candles, timelineCandles);
  const finiteReturns = hourlyReturns
    .map((hour) => hour.cumulativeReturn)
    .filter(Number.isFinite);
  const domain = finiteReturns.length
    ? Math.max(...finiteReturns.map((value) => Math.abs(value)), 0.01)
    : 0.01;
  const labels = hourlyReturns.map((hour) => String(hour.hour).padStart(2, "0"));
  const data = hourlyReturns.map((hour, index) => {
    if (!Number.isFinite(hour.cumulativeReturn)) return "-";
    const color = hour.cumulativeReturn >= 0 ? palette.bid : palette.ask;
    return {
      value: hour.cumulativeReturn,
      hourLabel: labels[index],
      observationCount: hour.observationCount,
      itemStyle: {
        color,
        borderRadius: hour.cumulativeReturn >= 0 ? [3, 3, 0, 0] : [0, 0, 3, 3],
        opacity: 0.82,
      },
    };
  });
  return {
    animation: false,
    grid: diagnosticGrid(42, 34),
    tooltip: {
      ...diagnosticTooltipStyle(palette),
      formatter: (params) => {
        const item = Array.isArray(params) ? params[0] : params;
        const hour = item?.data;
        if (!hour || !Number.isFinite(hour.value)) return "";
        return `<strong>${hour.hourLabel}:00 UTC</strong><br>Cumulative return: ${formatPercent(hour.value)}<br>Observations: ${formatCount(hour.observationCount)}<br>Entry: hourly candle open<br>Exit: following hourly candle open`;
      },
      trigger: "item",
    },
    xAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: {
        ...diagnosticAxisStyle(palette).axisLabel,
        interval: 2,
      },
      data: labels,
      type: "category",
    },
    yAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: {
        ...diagnosticAxisStyle(palette).axisLabel,
        formatter: (value) => formatPercent(value, false),
      },
      max: domain,
      min: -domain,
      name: "raw cumulative %",
      nameTextStyle: { color: palette.muted, fontSize: 10 },
      splitNumber: 2,
      splitLine: { lineStyle: { color: palette.grid } },
      type: "value",
    },
    series: [{
      type: "bar",
      name: "Cumulative hourly return",
      barMaxWidth: 28,
      barMinHeight: 2,
      data,
      emphasis: { focus: "series", itemStyle: { opacity: 1, shadowBlur: 8, shadowColor: palette.accent } },
      markLine: {
        data: [{ yAxis: 0 }],
        label: { show: false },
        lineStyle: { color: palette.border, width: 1 },
        silent: true,
        symbol: "none",
      },
    }],
  };
}

function rollingPredictionChartOption(observationCandles, timelineCandles, windowSize, palette) {
  const results = rollingPredictionByHour(observationCandles, windowSize, timelineCandles);
  const labels = results.map((result) => String(result.hour).padStart(2, "0"));
  const dataFor = (key) => results.map((result, index) => ({
    value: result[key],
    hourLabel: labels[index],
    total: result.total,
    accuracy: result.accuracy,
  }));
  return {
    animation: false,
    grid: { bottom: 36, containLabel: true, left: 42, right: 8, top: 30 },
    legend: {
      data: ["Correct", "False"],
      icon: "roundRect",
      itemHeight: 9,
      itemWidth: 12,
      right: 8,
      textStyle: { color: palette.muted, fontSize: 10 },
      top: 0,
    },
    tooltip: {
      ...diagnosticTooltipStyle(palette),
      axisPointer: { type: "shadow" },
      formatter: (params) => {
        const items = Array.isArray(params) ? params : [params];
        const first = items[0]?.data;
        if (!first) return "";
        const correct = items.find((item) => item.seriesName === "Correct")?.value ?? 0;
        const incorrect = items.find((item) => item.seriesName === "False")?.value ?? 0;
        const accuracy = Number.isFinite(first.accuracy) ? `${(first.accuracy * 100).toFixed(1)}%` : "—";
        return `<strong>UTC ${first.hourLabel}:00</strong><br>Correct: ${formatCount(correct)}<br>False: ${formatCount(incorrect)}<br>Accuracy: ${accuracy}<br>Eligible predictions: ${formatCount(first.total)}`;
      },
      trigger: "axis",
    },
    xAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: {
        ...diagnosticAxisStyle(palette).axisLabel,
        interval: 2,
      },
      data: labels,
      type: "category",
    },
    yAxis: {
      ...diagnosticAxisStyle(palette),
      axisLabel: {
        ...diagnosticAxisStyle(palette).axisLabel,
        formatter: (value) => formatCompact(value),
      },
      min: 0,
      name: "predictions",
      nameTextStyle: { color: palette.muted, fontSize: 10 },
      splitLine: { lineStyle: { color: palette.grid } },
      type: "value",
    },
    series: [
      {
        name: "Correct",
        type: "bar",
        stack: "outcome",
        barMaxWidth: 28,
        data: dataFor("correct"),
        itemStyle: { color: palette.bid, opacity: 0.86 },
      },
      {
        name: "False",
        type: "bar",
        stack: "outcome",
        barMaxWidth: 28,
        data: dataFor("incorrect"),
        itemStyle: { color: palette.ask, opacity: 0.86 },
      },
    ],
  };
}

function diagnosticChartOption(record) {
  const palette = diagnosticPalette(record.state);
  if (record.kind === "cumulative") {
    return cumulativeReturnByHourChartOption(record.candles, record.timelineCandles, palette);
  }
  if (record.kind === "rolling") {
    const observations = rollingObservationCandles(record.candles, rollingPredictionScope);
    return rollingPredictionChartOption(observations, record.timelineCandles, ROLLING_WINDOW_SIZE, palette);
  }
  if (record.kind === "returns") {
    return histogramChartOption(
      record.state,
      record.state.returns,
      -record.domains.returnDomain,
      record.domains.returnDomain,
      (value) => formatPercent(value, false),
      palette,
      "Return distribution",
    );
  }
  if (record.kind === "range") {
    return boxplotChartOption(record.state, record.state.rangeStats, record.domains.rangeDomain, palette);
  }
  if (record.kind === "volume") {
    return histogramChartOption(
      record.state,
      record.state.volumes,
      0,
      record.domains.volumeDomain,
      (value) => formatCompact(value),
      palette,
      "Base volume distribution",
    );
  }
  return driftChartOption(record.state, record.interval, record.domains.driftDomain, palette);
}

function disposeDiagnosticCharts() {
  diagnosticCharts.forEach(({ instance, resizeObserver }) => {
    resizeObserver?.disconnect();
    instance.dispose();
  });
  diagnosticCharts = [];
}

function initializeDiagnosticCharts(states, interval, domains, candles) {
  disposeDiagnosticCharts();
  if (!window.echarts) {
    diagnosticsStatus.textContent += " Interactive diagnostics are unavailable because ECharts did not load.";
    document.querySelectorAll(".diagnostic-chart").forEach((element) => {
      element.textContent = "Interactive chart library unavailable.";
      element.classList.add("diagnostic-chart--empty");
    });
    return;
  }
  const definitions = [
    ["returns", "Return distribution"],
    ["range", "Candle range / spread"],
    ["volume", "Volume profile"],
    ["drift", "Average return by UTC bucket"],
    ["cumulative", "Cumulative return by hour"],
  ];
  const records = [];
  states.filter((state) => !state.disabled).forEach((state) => {
    definitions.forEach(([kind, label]) => {
      const key = `${state.id}:${kind}`;
      const element = diagnosticsRows.querySelector(`[data-diagnostic-chart="${key}"]`);
      if (!element) return;
      const record = {
        candles: kind === "cumulative" ? state.candles : undefined,
        domains,
        element,
        interval,
        kind,
        label,
        state,
        timelineCandles: kind === "cumulative" ? candles : undefined,
      };
      const instance = window.echarts.init(element, null, { renderer: "canvas" });
      const option = diagnosticChartOption(record);
      if (option) instance.setOption(option);
      const resizeObserver = new ResizeObserver(() => instance.resize());
      resizeObserver.observe(element);
      records.push({ ...record, instance, resizeObserver });
    });
  });
  const rollingElement = rollingPredictionCard?.querySelector('[data-diagnostic-chart="rolling-prediction"]');
  if (rollingElement && interval === "1h") {
    const record = {
      candles,
      element: rollingElement,
      interval,
      kind: "rolling",
      label: "Rolling same-hour prediction",
      state: states[0],
      timelineCandles: candles,
    };
    const instance = window.echarts.init(rollingElement, null, { renderer: "canvas" });
    const option = diagnosticChartOption(record);
    if (option) instance.setOption(option);
    const resizeObserver = new ResizeObserver(() => instance.resize());
    resizeObserver.observe(rollingElement);
    records.push({ ...record, instance, resizeObserver });
  }
  diagnosticCharts = records;
}

function refreshDiagnosticCharts() {
  diagnosticCharts.forEach((record) => {
    const option = diagnosticChartOption(record);
    if (option) record.instance.setOption(option, true);
  });
}

function renderCumulativeReturnCard(state, interval) {
  if (interval !== "1h") {
    return `<article class="diagnostic-card diagnostic-card--unavailable">
      <div class="diagnostic-card-heading"><h4>Cumulative return by hour</h4><span>1-hour candles</span></div>
      <div class="diagnostic-unavailable">Select the 1-hour chart interval to calculate cumulative returns by UTC hour.</div>
    </article>`;
  }
  return diagnosticCard(
    "Cumulative return by hour",
    "raw cumulative %",
    `${state.id}:cumulative`,
    `${state.label} cumulative return by UTC hour`,
    [],
  );
}

function rollingScopeToggleMarkup(disabled = false) {
  return `<div class="rolling-scope-toggle" role="group" aria-label="Rolling prediction sample">
    ${Object.entries(ROLLING_SCOPE_LABELS).map(([scope, label]) => {
      const active = scope === rollingPredictionScope;
      return `<button class="rolling-scope-toggle-button${active ? " is-active" : ""}" type="button" data-rolling-scope="${scope}" aria-pressed="${active}"${disabled ? " disabled" : ""}>${label}</button>`;
    }).join("")}
  </div>`;
}

function bindRollingScopeToggle() {
  rollingPredictionCard.querySelectorAll("[data-rolling-scope]").forEach((button) => {
    button.addEventListener("click", () => {
      const nextScope = button.dataset.rollingScope;
      if (!nextScope || nextScope === rollingPredictionScope) return;
      rollingPredictionScope = nextScope;
      renderDiagnostics(diagnosticCandles);
    });
  });
}

function renderRollingPrediction(candles, interval) {
  const scopeLabel = ROLLING_SCOPE_LABELS[rollingPredictionScope];
  if (interval !== "1h") {
    rollingPredictionStatus.textContent = `${scopeLabel} · This check uses hourly candles.`;
    rollingPredictionCard.innerHTML = `<article class="diagnostic-card rolling-prediction-card rolling-prediction-card--unavailable">
      <div class="diagnostic-card-heading"><h4>Correct vs false by UTC hour</h4><span>${ROLLING_WINDOW_SIZE} prior same-hour bars</span></div>
      ${rollingScopeToggleMarkup(true)}
      <div class="diagnostic-unavailable">Select the 1-hour chart interval to run the rolling same-hour prediction check.</div>
    </article>`;
    bindRollingScopeToggle();
    return;
  }
  const observations = rollingObservationCandles(candles, rollingPredictionScope);
  const results = rollingPredictionByHour(observations, ROLLING_WINDOW_SIZE, candles);
  const totals = results.reduce((summary, result) => ({
    correct: summary.correct + result.correct,
    incorrect: summary.incorrect + result.incorrect,
  }), { correct: 0, incorrect: 0 });
  const total = totals.correct + totals.incorrect;
  const accuracy = total ? `${((totals.correct / total) * 100).toFixed(1)}%` : "—";
  rollingPredictionStatus.textContent = `${scopeLabel} · For each UTC hour, the previous ${ROLLING_WINDOW_SIZE} same-hour returns are averaged. Positive average predicts positive; otherwise negative.`;
  rollingPredictionCard.innerHTML = `<article class="diagnostic-card rolling-prediction-card">
    <div class="diagnostic-card-heading"><h4>Correct vs false by UTC hour</h4><span>${ROLLING_WINDOW_SIZE} prior same-hour bars</span></div>
    ${rollingScopeToggleMarkup()}
    <div class="diagnostic-chart" data-diagnostic-chart="rolling-prediction" role="img" aria-label="Correct and false rolling predictions by UTC hour"></div>
    <div class="diagnostic-card-stats">
      <span><strong>${formatCount(totals.correct)}</strong><small>Correct</small></span>
      <span><strong>${formatCount(totals.incorrect)}</strong><small>False</small></span>
      <span><strong>${accuracy}</strong><small>Accuracy</small></span>
    </div>
  </article>`;
  bindRollingScopeToggle();
}

function renderDiagnosticRow(state, interval, domains) {
  const rowHeading = `<div class="diagnostic-row-heading">
    <div class="diagnostic-state-title"><span class="diagnostic-state-dot"></span><div><h3>${state.label}</h3><p>${state.description}</p></div></div>
    <span class="diagnostic-sample">${state.disabled ? "Not available" : `n = ${formatCount(state.count)} candles`}</span>
  </div>`;
  if (state.disabled) {
    return `<article class="diagnostic-row diagnostic-row--unavailable" style="--diagnostic-accent:${state.color}">${rowHeading}<div class="diagnostic-unavailable">Daily candles contain the full day, so this state needs 1-minute or 1-hour resolution.</div></article>`;
  }
  const returnStats = state.returnStats;
  const rangeStats = state.rangeStats;
  const volumeStats = state.volumeStats;
  const peakIndex = state.driftMeans.reduce((best, value, index, means) => (
    Number.isFinite(value) && (!Number.isFinite(means[best]) || Math.abs(value) > Math.abs(means[best])) ? index : best
  ), -1);
  const labels = driftBucketLabels(interval);
  const peakLabel = peakIndex >= 0 ? labels[peakIndex] : "—";
  const averageByBucketTitle = interval === "1d" ? "Average return by weekday" : "Average return by hour";
  return `<article class="diagnostic-row" style="--diagnostic-accent:${state.color}">
    ${rowHeading}
    <div class="diagnostic-card-grid">
      ${diagnosticCard("Return distribution", `% per ${TIMEFRAME_LABELS[interval]}`, `${state.id}:returns`, `${state.label} return distribution`, [
        ["Mean", formatPercent(returnStats?.mean)],
        ["Std dev", formatPercent(returnStats?.stddev, false)],
        ["Positive", returnStats ? `${(returnStats.positiveShare * 100).toFixed(1)}%` : "—"],
      ])}
      ${diagnosticCard("Candle range / spread", `% per ${TIMEFRAME_LABELS[interval]}`, `${state.id}:range`, `${state.label} high-low range distribution`, [
        ["Median", formatPercent(rangeStats?.p50, false)],
        ["P75", formatPercent(rangeStats?.p75, false)],
        ["P95", formatPercent(rangeStats?.p95, false)],
      ])}
      ${diagnosticCard("Volume profile", "base asset", `${state.id}:volume`, `${state.label} base volume distribution`, [
        ["Median", formatCompact(volumeStats?.p50)],
        ["P90", formatCompact(volumeStats?.p90)],
        ["Total", formatCompact(volumeStats?.total)],
      ])}
      ${diagnosticCard(averageByBucketTitle, `% return per ${TIMEFRAME_LABELS[interval]}`, `${state.id}:drift`, averageByBucketTitle, [
        ["Mean", formatPercent(returnStats?.mean)],
        ["Positive", returnStats ? `${(returnStats.positiveShare * 100).toFixed(1)}%` : "—"],
        ["Peak UTC bucket", peakLabel],
      ])}
      ${renderCumulativeReturnCard(state, interval)}
    </div>
  </article>`;
}

function resetDiagnostics(message) {
  disposeDiagnosticCharts();
  diagnosticCandles = [];
  diagnosticsContext.textContent = `Selected interval · ${TIMEFRAME_LABELS[timeframeSelect.value]}`;
  diagnosticsStatus.textContent = message;
  diagnosticsRows.replaceChildren();
  rollingPredictionStatus.textContent = message;
  rollingPredictionCard.replaceChildren();
}

function renderDiagnostics(candles) {
  const interval = timeframeSelect.value;
  const intervalLabel = TIMEFRAME_LABELS[interval];
  diagnosticCandles = candles;
  diagnosticsContext.textContent = `Selected interval · ${intervalLabel}`;
  if (!candles.length) {
    resetDiagnostics("Select a stored ticker to calculate calendar effects.");
    return;
  }
  const states = prepareDiagnosticStates(candles, interval);
  const domains = diagnosticDomains(states);
  diagnosticsStatus.textContent = `All metrics use ${intervalLabel} OHLCV candles. Hover a chart for exact bins and statistics. UTC weekends are separated; weekday candidate windows exclude weekends.`;
  diagnosticsRows.innerHTML = states.map((state) => renderDiagnosticRow(state, interval, domains)).join("");
  renderRollingPrediction(candles, interval);
  initializeDiagnosticCharts(states, interval, domains, candles);
}

function renderSelectedTimeframe() {
  const candles = aggregateCandles(rawCandles, timeframeSelect.value);
  chart.setCandles(candles);
  weekendOverlay.setCandles(candles);
  afterHoursOverlay.setCandles(candles);
  renderDiagnostics(candles);
  chartEmpty.hidden = candles.length > 0;
  if (!candles.length) chartEmpty.textContent = "This ticker has no stored OHLCV candles.";
}

const BACKTEST_JOB_STORAGE_KEY = "binance-grid-active-backtest-job";
const BACKTEST_RUN_STORAGE_KEY = "binance-grid-last-backtest-run";
const BACKTEST_POLL_MS = 2500;
let backtestJobActive = false;
let backtestPollTimer = null;

function utcDateInput(ms) {
  if (!Number.isFinite(Number(ms))) return "";
  return new Date(Number(ms)).toISOString().slice(0, 10);
}

function utcDayStartMs(value) {
  const parsed = Date.parse(`${value}T00:00:00.000Z`);
  return Number.isFinite(parsed) ? parsed : null;
}

function utcDayEndMs(value) {
  const start = utcDayStartMs(value);
  return start === null ? null : start + 86_400_000 - 1;
}

function selectedBacktestDataset() {
  return datasetsById.get(Number(datasetSelect.value)) ?? null;
}

function syncBacktestSetupContext(resetDates = false) {
  const dataset = selectedBacktestDataset();
  const interval = timeframeSelect.value;
  if (!dataset) {
    backtestRunContext.textContent = "Select historical data above";
    return;
  }
  backtestRunContext.textContent = `${dataset.symbol} · ${TIMEFRAME_LABELS[interval]} · backend replay`;
  const minimumDate = utcDateInput(dataset.start_time_ms);
  const maximumDate = utcDateInput(dataset.end_time_ms);
  backtestStartDate.min = minimumDate;
  backtestStartDate.max = maximumDate;
  backtestEndDate.min = minimumDate;
  backtestEndDate.max = maximumDate;
  if (resetDates || !backtestStartDate.value) backtestStartDate.value = minimumDate;
  if (resetDates || !backtestEndDate.value) backtestEndDate.value = maximumDate;
}

function setBacktestFormBusy(busy) {
  backtestJobActive = busy;
  runBacktestButton.disabled = busy;
  runBacktestButton.textContent = busy ? "Backtest running…" : "Run backtest on Render";
  backtestRunForm.querySelectorAll("input, select").forEach((control) => {
    if (control.matches("[data-always-disabled]")) {
      control.disabled = true;
    } else if (control === backtestFixedAnchor) {
      control.disabled = busy || backtestGridAnchor.value !== "fixed";
    } else {
      control.disabled = busy;
    }
  });
  datasetSelect.disabled = busy;
  timeframeSelect.disabled = busy;
}

function setBacktestProgress(percent, message, status = null) {
  const value = Math.max(0, Math.min(100, Number(percent) || 0));
  backtestProgressPercent.textContent = `${Math.round(value)}%`;
  backtestProgressFill.style.width = `${value}%`;
  backtestProgressTrack?.setAttribute("aria-valuenow", String(Math.round(value)));
  if (message) backtestProgressMessage.textContent = message;
  if (status) {
    backtestRunStatusBadge.textContent = status;
    backtestRunStatusBadge.classList.toggle("badge--success", status === "Completed");
  }
}

function formatDecimal(value, digits = 2) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "—";
  return number.toLocaleString(undefined, { maximumFractionDigits: digits, minimumFractionDigits: digits });
}

function formatQuantity(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "—";
  return number.toLocaleString(undefined, { maximumFractionDigits: 8 });
}

function formatSignedPercent(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "—";
  return `${number >= 0 ? "+" : ""}${number.toFixed(3)}%`;
}

function formatUtcTimestamp(ms) {
  const value = Number(ms);
  if (!Number.isFinite(value)) return "—";
  return new Date(value).toISOString().replace("T", " ").replace(".000Z", "Z");
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function renderBacktestResult(result) {
  backtestResultEmpty.hidden = true;
  backtestResultContent.hidden = false;
  backtestRunStatusBadge.textContent = "Completed";
  backtestRunStatusBadge.classList.add("badge--success");

  const kpis = [
    ["Total return", formatSignedPercent(result.total_return_percent)],
    ["Final equity", formatDecimal(result.final_equity)],
    ["Max drawdown", formatSignedPercent(result.max_drawdown_percent)],
    ["Fees", formatDecimal(result.fees_paid, 6)],
    ["Fills", Number(result.fill_count ?? 0).toLocaleString()],
    ["Final position", formatQuantity(result.final_position_quantity)],
    ["Realized PnL", formatDecimal(result.realized_pnl)],
    ["Run ID", `#${result.run_id}`],
  ];
  backtestKpiGrid.innerHTML = kpis.map(([label, value]) => `
    <div class="backtest-kpi">
      <span>${escapeHtml(label)}</span>
      <strong>${escapeHtml(value)}</strong>
    </div>
  `).join("");

  const preRoll = result.reserved_first_candle_as_preroll
    ? " · first available replay candle reserved as pre-roll"
    : "";
  backtestResultContext.textContent =
    `${result.symbol} · ${result.replay_interval} · ${Number(result.candles_processed).toLocaleString()} active candles · ` +
    `${formatUtcTimestamp(result.effective_start_time_ms)} → ${formatUtcTimestamp(result.effective_end_time_ms)}${preRoll}`;

  const fills = Array.isArray(result.fills) ? result.fills : [];
  backtestFillAuditCount.textContent = `${fills.length.toLocaleString()} fills`;
  backtestFillTableBody.innerHTML = fills.length
    ? fills.map((fill) => `
      <tr>
        <td>${escapeHtml(formatUtcTimestamp(fill.event_time_ms))}</td>
        <td><span class="fill-side fill-side--${escapeHtml(fill.side)}">${escapeHtml(fill.side)}</span></td>
        <td>${escapeHtml(fill.order_type)}</td>
        <td>${escapeHtml(formatQuantity(fill.price))}</td>
        <td>${escapeHtml(formatQuantity(fill.quantity))}</td>
        <td>${escapeHtml(formatQuantity(fill.fee ?? 0))}</td>
        <td>${escapeHtml(fill.liquidity_role ?? "—")}</td>
      </tr>
    `).join("")
    : '<tr><td colspan="7" class="backtest-fill-empty">No fills occurred in this run.</td></tr>';
}

function backtestRequestPayload() {
  const dataset = selectedBacktestDataset();
  if (!dataset) throw new Error("Select a stored historical dataset first.");
  if (!backtestStartDate.value || !backtestEndDate.value) throw new Error("Start and end dates are required.");
  const startTime = utcDayStartMs(backtestStartDate.value);
  const endTime = utcDayEndMs(backtestEndDate.value);
  if (startTime === null || endTime === null || startTime > endTime) throw new Error("Start date must be on or before end date.");

  const numericInputs = [
    ["Initial capital", backtestInitialCapital.value, 0, null],
    ["Grid spacing", backtestGridSpacing.value, 0, 10_000],
    ["Levels per side", backtestGridLevels.value, 0, 101],
    ["Quantity per order", backtestGridQuantity.value, 0, null],
    ["Fee bps", backtestFeeBps.value, -1, null],
    ["Spread bps", backtestSpreadBps.value, -1, null],
    ["Slippage bps", backtestSlippageBps.value, -1, null],
    ["Latency", backtestLatencyMs.value, -1, null],
    ["Partial fill ratio", backtestPartialFill.value, 0, 1.0000001],
  ];
  for (const [label, raw, minExclusive, maxExclusive] of numericInputs) {
    const value = Number(raw);
    if (!Number.isFinite(value) || value <= minExclusive || (maxExclusive !== null && value >= maxExclusive)) {
      throw new Error(`${label} has an invalid value.`);
    }
  }
  if (!Number.isInteger(Number(backtestGridLevels.value))) throw new Error("Levels per side must be a whole number.");
  if (!Number.isInteger(Number(backtestLatencyMs.value))) throw new Error("Latency must be a whole number of milliseconds.");

  const anchor = backtestGridAnchor.value;
  const fixedAnchor = anchor === "fixed" ? Number(backtestFixedAnchor.value) : null;
  if (anchor === "fixed" && (!Number.isFinite(fixedAnchor) || fixedAnchor <= 0)) {
    throw new Error("Enter a positive fixed anchor price.");
  }

  return {
    dataset_id: dataset.dataset_id,
    replay_interval: timeframeSelect.value,
    start_time_ms: startTime,
    end_time_ms: endTime,
    initial_capital: String(backtestInitialCapital.value),
    strategy_id: backtestStrategy.value,
    grid: {
      anchor,
      fixed_anchor_price: fixedAnchor,
      spacing_bps: Number(backtestGridSpacing.value),
      levels_per_side: Number(backtestGridLevels.value),
      quantity_per_order: Number(backtestGridQuantity.value),
    },
    execution: {
      fee_bps: Number(backtestFeeBps.value),
      spread_bps: Number(backtestSpreadBps.value),
      slippage_bps: Number(backtestSlippageBps.value),
      latency_ms: Number(backtestLatencyMs.value),
      limit_fill_policy: backtestLimitPolicy.value,
      partial_fill_ratio: Number(backtestPartialFill.value),
    },
  };
}

async function pollBacktestJob(jobId) {
  clearTimeout(backtestPollTimer);
  try {
    const response = await fetch(`/api/backtests/jobs/${jobId}`);
    if (response.status === 404) {
      localStorage.removeItem(BACKTEST_JOB_STORAGE_KEY);
      setBacktestFormBusy(false);
      setBacktestProgress(0, "Previous backend job is no longer available.", "No run");
      return;
    }
    if (!response.ok) throw new Error(await response.text());
    const job = await response.json();
    const label = job.status === "completed"
      ? "Completed"
      : job.status === "failed"
        ? "Failed"
        : job.status === "queued"
          ? "Queued"
          : "Running";
    setBacktestProgress(job.progress_percent, job.message, label);

    if (job.status === "completed") {
      localStorage.removeItem(BACKTEST_JOB_STORAGE_KEY);
      if (job.run_id) localStorage.setItem(BACKTEST_RUN_STORAGE_KEY, String(job.run_id));
      setBacktestFormBusy(false);
      if (job.result) renderBacktestResult(job.result);
      return;
    }
    if (job.status === "failed") {
      localStorage.removeItem(BACKTEST_JOB_STORAGE_KEY);
      setBacktestFormBusy(false);
      backtestFormError.hidden = false;
      backtestFormError.textContent = job.message || "Backtest failed.";
      return;
    }
    backtestPollTimer = setTimeout(() => pollBacktestJob(jobId), BACKTEST_POLL_MS);
  } catch (error) {
    backtestPollTimer = setTimeout(() => pollBacktestJob(jobId), BACKTEST_POLL_MS);
    backtestProgressMessage.textContent = `Status check delayed: ${error.message || "network error"}`;
  }
}

async function restoreBacktestState() {
  const activeJob = Number(localStorage.getItem(BACKTEST_JOB_STORAGE_KEY));
  if (Number.isFinite(activeJob) && activeJob > 0) {
    setBacktestFormBusy(true);
    await pollBacktestJob(activeJob);
    return;
  }
  const lastRun = Number(localStorage.getItem(BACKTEST_RUN_STORAGE_KEY));
  if (!Number.isFinite(lastRun) || lastRun <= 0) return;
  try {
    const response = await fetch(`/api/backtests/runs/${lastRun}`);
    if (!response.ok) return;
    const result = await response.json();
    setBacktestProgress(100, "Last persisted run loaded.", "Completed");
    renderBacktestResult(result);
  } catch (_) {
    // The historical page still works even if the optional previous-result restore fails.
  }
}

backtestGridAnchor.addEventListener("change", () => {
  backtestFixedAnchor.disabled = backtestJobActive || backtestGridAnchor.value !== "fixed";
});

backtestRunForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  backtestFormError.hidden = true;
  backtestResultEmpty.hidden = false;
  backtestResultEmpty.textContent = "Backtest is running on the Render backend…";
  backtestResultContent.hidden = true;
  setBacktestProgress(0, "Submitting backend job…", "Queued");

  let payload;
  try {
    payload = backtestRequestPayload();
  } catch (error) {
    backtestFormError.hidden = false;
    backtestFormError.textContent = error.message;
    setBacktestProgress(0, "Ready to run.", "No run");
    return;
  }

  setBacktestFormBusy(true);
  try {
    const response = await fetch("/api/backtests/jobs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) throw new Error(await response.text());
    const job = await response.json();
    localStorage.setItem(BACKTEST_JOB_STORAGE_KEY, String(job.job_id));
    setBacktestProgress(job.progress_percent, job.message, "Queued");
    await pollBacktestJob(job.job_id);
  } catch (error) {
    setBacktestFormBusy(false);
    backtestFormError.hidden = false;
    backtestFormError.textContent = error.message || "Could not start the backtest.";
    backtestResultEmpty.textContent = "Configure the strategy above and start a historical replay.";
    setBacktestProgress(0, "Backtest was not started.", "No run");
  }
});

async function loadSeries() {
  const datasetId = Number(datasetSelect.value);
  if (!datasetId) {
    rawCandles = [];
    updateAfterHoursSchedule(null);
    chart.reset();
    weekendOverlay.reset();
    afterHoursOverlay.reset();
    resetDiagnostics("No stored ticker is selected.");
    chartEmpty.hidden = false; dataStatus.textContent = "No stored ticker is selected."; return;
  }
  datasetSelect.disabled = true;
  timeframeSelect.disabled = true;
  updateAfterHoursSchedule(datasetsById.get(datasetId));
  syncBacktestSetupContext(true);
  dataStatus.textContent = "Loading complete OHLCV series…";
  chartEmpty.hidden = false;
  chartEmpty.textContent = "Loading OHLCV data…";
  try {
    const response = await fetch(`/api/data/ohlcv?dataset_id=${datasetId}`);
    if (!response.ok) throw new Error(await response.text());
    const { candles } = await response.json();
    rawCandles = candles;
    renderSelectedTimeframe();
    dataStatus.textContent = "";
  } catch (error) {
    rawCandles = [];
    chart.reset();
    weekendOverlay.reset();
    afterHoursOverlay.reset();
    resetDiagnostics("Could not load diagnostics for the selected ticker.");
    chartEmpty.hidden = false; chartEmpty.textContent = "Could not load OHLCV data.";
    dataStatus.textContent = error.message || "Could not load the selected ticker.";
  } finally { datasetSelect.disabled = false; timeframeSelect.disabled = false; }
}

async function loadDatasets() {
  datasetSelect.disabled = true;
  dataStatus.textContent = "Loading stored tickers…";
  try {
    const response = await fetch("/api/data/datasets");
    if (!response.ok) throw new Error(await response.text());
    const { datasets } = await response.json();
    datasetsById = new Map(datasets.map((dataset) => [dataset.dataset_id, dataset]));
    datasetSelect.replaceChildren();
    if (!datasets.length) {
      datasetSelect.append(new Option("No stored OHLCV tickers", ""));
      chartEmpty.hidden = false;
      dataStatus.textContent = "Download a ticker before selecting backtest data.";
      return;
    }
    for (const dataset of datasets) datasetSelect.append(new Option(tickerLabel(dataset), dataset.dataset_id));
    await loadSeries();
  } catch (error) {
    resetDiagnostics(error.message || "Could not load interval diagnostics.");
    chartEmpty.hidden = false; chartEmpty.textContent = "Could not load stored tickers.";
    dataStatus.textContent = error.message || "Could not load stored tickers.";
  } finally { datasetSelect.disabled = false; }
}

function setCollapsiblePanelExpanded(section, toggle, content, expanded) {
  section?.classList.toggle("is-collapsed", !expanded);
  toggle?.setAttribute("aria-expanded", String(expanded));
  content?.setAttribute("aria-hidden", String(!expanded));
  const label = toggle?.querySelector(".collapsible-toggle-label");
  if (label) label.textContent = expanded ? "Collapse" : "Open";
  if (expanded) {
    requestAnimationFrame(() => {
      diagnosticCharts.forEach(({ instance }) => instance.resize());
    });
  }
}

diagnosticsToggle?.addEventListener("click", () => {
  setCollapsiblePanelExpanded(
    diagnosticsSection,
    diagnosticsToggle,
    diagnosticsContent,
    diagnosticsToggle.getAttribute("aria-expanded") !== "true",
  );
});

rollingPredictionToggle?.addEventListener("click", () => {
  setCollapsiblePanelExpanded(
    rollingPredictionSection,
    rollingPredictionToggle,
    rollingPredictionContent,
    rollingPredictionToggle.getAttribute("aria-expanded") !== "true",
  );
});

setCollapsiblePanelExpanded(diagnosticsSection, diagnosticsToggle, diagnosticsContent, false);
setCollapsiblePanelExpanded(rollingPredictionSection, rollingPredictionToggle, rollingPredictionContent, false);

datasetSelect.addEventListener("change", loadSeries);
timeframeSelect.addEventListener("change", () => {
  renderSelectedTimeframe();
  syncBacktestSetupContext(false);
});
weekendOverlayToggle.addEventListener("change", () => updateOverlayPreference(
  weekendOverlayToggle, OVERLAY_PREFERENCES.weekend, weekendOverlay,
));
afterHoursOverlayToggle.addEventListener("change", () => updateOverlayPreference(
  afterHoursOverlayToggle, OVERLAY_PREFERENCES.afterHours, afterHoursOverlay,
));
themeToggle.addEventListener("click", () => {
  const nextTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  setTheme(nextTheme);
});

chart.initialize();
weekendOverlay.attach(chart.chart);
afterHoursOverlay.attach(chart.chart);
initializeOverlayPreferences();
initializeTheme();
loadDatasets().then(() => {
  syncBacktestSetupContext(false);
  restoreBacktestState();
});
