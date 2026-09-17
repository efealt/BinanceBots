const themeToggle = document.querySelector("#theme-toggle");
const symbolSelect = document.querySelector("#market-symbol");
const timeframeSelect = document.querySelector("#market-timeframe");
const chartTitle = document.querySelector("#chart-title");
const chartEmpty = document.querySelector("#chart-empty");
const feedDot = document.querySelector("#feed-dot");
const feedLabel = document.querySelector("#feed-label");
const barCount = document.querySelector("#bar-count");
const lastPrice = document.querySelector("#last-price");
const quoteStatus = document.querySelector("#quote-status");
const bestBid = document.querySelector("#best-bid");
const bidQuantity = document.querySelector("#bid-quantity");
const bestAsk = document.querySelector("#best-ask");
const askQuantity = document.querySelector("#ask-quantity");
const spreadValue = document.querySelector("#spread-value");
const spreadBps = document.querySelector("#spread-bps");
const midPrice = document.querySelector("#mid-price");
const orderBookStatus = document.querySelector("#order-book-status");
const orderBookMid = document.querySelector("#order-book-mid");
const orderBookSpread = document.querySelector("#order-book-spread");
const orderBookAsks = document.querySelector("#order-book-asks");
const orderBookBids = document.querySelector("#order-book-bids");
const tradesStatus = document.querySelector("#trades-status");
const recentTrades = document.querySelector("#recent-trades");
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
  const number = Number(value);
  if (!Number.isFinite(number)) return "—";
  const maximumFractionDigits = number >= 100 ? 2 : number >= 1 ? 4 : 8;
  return number.toLocaleString(undefined, { maximumFractionDigits });
}

function formatQuantity(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "—";
  return number.toLocaleString(undefined, { maximumFractionDigits: 6 });
}

function formatUtcTime(value) {
  const timestamp = Number(value);
  if (!Number.isFinite(timestamp)) return "—";
  return new Intl.DateTimeFormat(undefined, {
    timeZone: "UTC",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(timestamp));
}

function appendTextCell(row, text, className = "") {
  const cell = document.createElement("span");
  cell.textContent = text;
  if (className) cell.className = className;
  row.append(cell);
}

function renderQuote(quote = {}) {
  const bid = Number(quote.best_bid);
  const bidSize = Number(quote.best_bid_quantity);
  const ask = Number(quote.best_ask);
  const askSize = Number(quote.best_ask_quantity);
  const spread = Number.isFinite(Number(quote.spread))
    ? Number(quote.spread)
    : Number.isFinite(bid) && Number.isFinite(ask)
      ? ask - bid
      : NaN;
  const midpoint = Number.isFinite(Number(quote.mid_price))
    ? Number(quote.mid_price)
    : Number.isFinite(bid) && Number.isFinite(ask)
      ? (bid + ask) / 2
      : NaN;

  bestBid.textContent = formatPrice(bid);
  bidQuantity.textContent = Number.isFinite(bidSize) ? "Size " + formatQuantity(bidSize) : "Size —";
  bestAsk.textContent = formatPrice(ask);
  askQuantity.textContent = Number.isFinite(askSize) ? "Size " + formatQuantity(askSize) : "Size —";
  spreadValue.textContent = formatPrice(spread);
  midPrice.textContent = formatPrice(midpoint);

  const basisPoints = Number.isFinite(spread) && Number.isFinite(midpoint) && midpoint > 0
    ? (spread / midpoint) * 10_000
    : NaN;
  spreadBps.textContent = Number.isFinite(basisPoints) ? basisPoints.toFixed(2) + " bps" : "—";
  quoteStatus.textContent = Number.isFinite(bid) && Number.isFinite(ask) ? "Live" : "Waiting";
}

function renderBookSide(container, levels, side) {
  container.replaceChildren();
  const normalized = Array.isArray(levels)
    ? levels
        .map((level) => ({
          price: Number(level.price),
          quantity: Number(level.quantity),
        }))
        .filter((level) => Number.isFinite(level.price) && Number.isFinite(level.quantity))
        .slice(0, 10)
    : [];

  if (!normalized.length) {
    const emptyRow = document.createElement("li");
    emptyRow.className = "market-empty-row";
    emptyRow.textContent = "Waiting for depth";
    container.append(emptyRow);
    return;
  }

  let cumulative = 0;
  const totals = normalized.map((level) => {
    cumulative += level.quantity;
    return cumulative;
  });
  const totalQuantity = totals[totals.length - 1];
  const displayLevels = (side === "ask" ? normalized.slice().reverse() : normalized).map((level) => {
    const index = normalized.indexOf(level);
    return { level, cumulative: totals[index] };
  });

  displayLevels.forEach(({ level, cumulative: levelTotal }) => {
    const row = document.createElement("li");
    row.className = "order-book-row order-book-row--" + side;
    const depth = totalQuantity > 0 ? Math.min(100, (levelTotal / totalQuantity) * 100) : 0;
    row.style.setProperty("--depth", String(depth) + "%");
    appendTextCell(row, formatPrice(level.price), "order-book-price");
    appendTextCell(row, formatQuantity(level.quantity));
    appendTextCell(row, formatQuantity(levelTotal));
    container.append(row);
  });
}

function renderOrderBook(orderBook = {}, quote = {}) {
  const asks = Array.isArray(orderBook.asks) ? orderBook.asks : [];
  const bids = Array.isArray(orderBook.bids) ? orderBook.bids : [];
  renderBookSide(orderBookAsks, asks, "ask");
  renderBookSide(orderBookBids, bids, "bid");

  const midpoint = Number(quote.mid_price);
  const spread = Number(quote.spread);
  orderBookMid.textContent = formatPrice(midpoint);
  orderBookSpread.textContent = Number.isFinite(spread) ? "Spread " + formatPrice(spread) : "Spread —";
  orderBookStatus.textContent = asks.length > 0 && bids.length > 0
    ? "Live · " + Math.min(asks.length, bids.length) + " levels"
    : "Waiting";
}

function renderTrades(trades = []) {
  recentTrades.replaceChildren();
  const rows = Array.isArray(trades) ? trades.slice(0, 30) : [];

  if (!rows.length) {
    const emptyRow = document.createElement("li");
    emptyRow.className = "market-empty-row";
    emptyRow.textContent = "Waiting for trades";
    recentTrades.append(emptyRow);
    tradesStatus.textContent = "Waiting";
    return;
  }

  rows.forEach((trade) => {
    const row = document.createElement("li");
    const side = trade.is_buyer_maker ? "sell" : "buy";
    row.className = "trade-row trade-row--" + side;
    appendTextCell(row, formatPrice(trade.price), "trade-price");
    appendTextCell(row, formatQuantity(trade.quantity));
    appendTextCell(row, formatUtcTime(trade.trade_time));
    recentTrades.append(row);
  });
  tradesStatus.textContent = "Live · " + rows.length + " trades";
}

function clearMarketPanels() {
  renderQuote();
  renderOrderBook();
  renderTrades();
}

async function refreshMarket(fitContent = false) {
  const currentRequest = ++requestId;
  const params = new URLSearchParams({
    symbol: symbolSelect.value,
    interval: timeframeSelect.value,
    market_type: symbolSelect.options[symbolSelect.selectedIndex].dataset.marketType ?? "spot",
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
    renderQuote(snapshot.quote);
    renderOrderBook(snapshot.order_book, snapshot.quote);
    renderTrades(snapshot.trades);
  } catch (error) {
    if (currentRequest !== requestId) return;
    setFeedState("reconnecting", "Market feed unavailable");
    barCount.textContent = "";
    lastPrice.textContent = "—";
    chartEmpty.hidden = false;
    chartEmpty.textContent = "Market data could not load";
    clearMarketPanels();
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
  clearMarketPanels();
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
