const themeToggle = document.querySelector("#theme-toggle");
const datasetSelect = document.querySelector("#backtest-dataset");
const timeframeSelect = document.querySelector("#backtest-timeframe");
const weekendOverlayToggle = document.querySelector("#backtest-weekend-overlay");
const afterHoursOverlayToggle = document.querySelector("#backtest-after-hours-overlay");
const afterHoursLabel = document.querySelector("#backtest-after-hours-label");
const dataStatus = document.querySelector("#backtest-data-status");
const chartEmpty = document.querySelector("#backtest-data-empty");
const TIMEFRAME_MS = { "1m": 60_000, "1h": 3_600_000, "1d": 86_400_000 };
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

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("theme", theme);
  chart.applyTheme();
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

function renderSelectedTimeframe() {
  const candles = aggregateCandles(rawCandles, timeframeSelect.value);
  chart.setCandles(candles);
  weekendOverlay.setCandles(candles);
  afterHoursOverlay.setCandles(candles);
  chartEmpty.hidden = candles.length > 0;
  if (!candles.length) chartEmpty.textContent = "This ticker has no stored OHLCV candles.";
}

async function loadSeries() {
  const datasetId = Number(datasetSelect.value);
  if (!datasetId) {
    rawCandles = [];
    updateAfterHoursSchedule(null);
    chart.reset();
    weekendOverlay.reset();
    afterHoursOverlay.reset();
    chartEmpty.hidden = false; dataStatus.textContent = "No stored ticker is selected."; return;
  }
  datasetSelect.disabled = true;
  timeframeSelect.disabled = true;
  updateAfterHoursSchedule(datasetsById.get(datasetId));
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
    chartEmpty.hidden = false; chartEmpty.textContent = "Could not load stored tickers.";
    dataStatus.textContent = error.message || "Could not load stored tickers.";
  } finally { datasetSelect.disabled = false; }
}

datasetSelect.addEventListener("change", loadSeries);
timeframeSelect.addEventListener("change", renderSelectedTimeframe);
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
loadDatasets();
