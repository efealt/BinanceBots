const themeToggle = document.querySelector("#theme-toggle");
const symbolSelect = document.querySelector("#market-symbol");
const timeframeSelect = document.querySelector("#market-timeframe");
const chartTitle = document.querySelector("#chart-title");
const chartEmpty = document.querySelector("#chart-empty");
const feedDot = document.querySelector("#feed-dot");
const feedLabel = document.querySelector("#feed-label");
const barCount = document.querySelector("#bar-count");
const lastPrice = document.querySelector("#last-price");
const indicatorToolbar = new ChartIndicatorToolbar(
  document.querySelector("#indicator-toolbar"),
  marketChart.indicatorLayer,
);

let requestId = 0;

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("theme", theme);
  marketChart.applyTheme();
}

function initializeTheme() {
  const savedTheme = localStorage.getItem("theme");
  const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  setTheme(savedTheme ?? systemTheme);
}

function updateTitle() {
  const interval = timeframeSelect.options[timeframeSelect.selectedIndex].text;
  chartTitle.textContent = `${symbolSelect.value} · ${interval}`;
}

function setFeedState(status, detail) {
  const labels = {
    live: "Live",
    loading: "Connecting",
    reconnecting: "Reconnecting",
  };
  feedDot.className = `status-dot ${status}`;
  feedLabel.textContent = detail || labels[status] || "Feed unavailable";
}

function formatPrice(value) {
  const maximumFractionDigits = value >= 100 ? 2 : value >= 1 ? 4 : 8;
  return value.toLocaleString(undefined, { maximumFractionDigits });
}

async function refreshMarket(fitContent = false) {
  const currentRequest = ++requestId;
  const params = new URLSearchParams({
    symbol: symbolSelect.value,
    interval: timeframeSelect.value,
  });

  try {
    const response = await fetch(`/api/market/candles?${params}`);
    if (!response.ok) throw new Error(await response.text());

    const snapshot = await response.json();
    if (currentRequest !== requestId) return;

    setFeedState(snapshot.status);
    barCount.textContent = `${snapshot.candles.length.toLocaleString()} bars in memory`;
    lastPrice.textContent = snapshot.candles.length
      ? formatPrice(snapshot.candles[snapshot.candles.length - 1].close)
      : "—";
    chartEmpty.hidden = snapshot.candles.length > 0;
    marketChart.setCandles(snapshot.candles, fitContent);
  } catch (error) {
    if (currentRequest !== requestId) return;
    setFeedState("reconnecting", "Market feed unavailable");
    barCount.textContent = "";
    lastPrice.textContent = "—";
    chartEmpty.hidden = false;
    chartEmpty.textContent = "Market data could not load";
  }
}

function changeMarket() {
  requestId += 1;
  marketChart.reset();
  updateTitle();
  setFeedState("loading", "Loading 1,000 candles");
  barCount.textContent = "";
  lastPrice.textContent = "—";
  chartEmpty.hidden = false;
  chartEmpty.textContent = "Loading market data…";
  refreshMarket(true);
}

themeToggle.addEventListener("click", () => {
  setTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark");
});
symbolSelect.addEventListener("change", changeMarket);
timeframeSelect.addEventListener("change", changeMarket);

initializeTheme();
changeMarket();
window.setInterval(() => refreshMarket(false), 2_000);
