const CONSOLE_REFRESH_MS = 45_000;

const botGrid = document.querySelector("#console-bot-grid");
const botCount = document.querySelector("#console-bot-count");
const runningCount = document.querySelector("#console-running-count");
const idleCount = document.querySelector("#console-idle-count");
const lastRefresh = document.querySelector("#console-last-refresh");
const refreshLabel = document.querySelector("#console-refresh-label");
const refreshDot = document.querySelector("#console-refresh-dot");
const consoleStatus = document.querySelector("#console-status");
const topDot = document.querySelector("#console-top-dot");
const topLabel = document.querySelector("#console-top-label");
const themeToggle = document.querySelector("#theme-toggle");

let refreshTimer = null;
let refreshInFlight = false;
let lastRefreshAt = 0;

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

function numeric(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function formatNumber(value, maximumFractionDigits = 8) {
  const number = numeric(value);
  if (number === null) return "—";
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits,
    minimumFractionDigits: 0,
  }).format(number);
}

function formatMoney(value) {
  const number = numeric(value);
  if (number === null) return "—";
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits: 4,
    minimumFractionDigits: 0,
  }).format(number);
}

function formatMarketType(value) {
  if (value === "spot") return "Spot";
  if (value === "usd_m_perpetual") return "USD-M perpetual";
  return value || "—";
}

function formatAge(timestamp) {
  const value = numeric(timestamp);
  if (value === null) return "—";
  const seconds = Math.max(0, Math.floor((Date.now() - value) / 1000));
  if (seconds < 60) return seconds + "s ago";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return minutes + "m ago";
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return hours + "h ago";
  return Math.floor(hours / 24) + "d ago";
}

function metric(label, value, tone = "") {
  const item = document.createElement("div");
  item.className = "console-bot-metric";
  const name = document.createElement("span");
  name.textContent = label;
  const strong = document.createElement("strong");
  strong.textContent = value;
  if (tone) strong.classList.add(tone);
  item.append(name, strong);
  return item;
}

function pnlTone(value) {
  const number = numeric(value);
  if (number === null || number === 0) return "";
  return number > 0 ? "is-positive" : "is-negative";
}

function renderBotCard(bot) {
  const card = document.createElement("a");
  card.className = "console-bot-card" + (bot.runtime_state === "running" ? " console-bot-card--running" : "");
  card.href = "/trading.html?bot_id=" + encodeURIComponent(bot.bot_id);
  card.setAttribute("aria-label", "Open " + bot.bot_name + " in Trading");

  const heading = document.createElement("div");
  heading.className = "console-bot-card-heading";

  const identity = document.createElement("div");
  identity.className = "console-bot-identity";
  const name = document.createElement("strong");
  name.textContent = bot.bot_name;
  const market = document.createElement("span");
  market.textContent = (bot.symbol || "—") + " · " + formatMarketType(bot.market_type);
  identity.append(name, market);

  const state = document.createElement("span");
  state.className = "badge " + (bot.runtime_state === "running" ? "console-running-badge" : "badge--muted");
  state.textContent = bot.runtime_state === "running" ? "Running · Live-Paper" : "Idle";
  heading.append(identity, state);

  const runLine = document.createElement("div");
  runLine.className = "console-bot-runline";
  if (bot.active_run_id != null) {
    runLine.textContent = "Bot #" + bot.bot_id + " · Run #" + bot.active_run_id;
  } else if (bot.latest_run_id != null) {
    const status = bot.latest_run_status ? " · last " + String(bot.latest_run_status).replaceAll("_", " ") : "";
    runLine.textContent = "Bot #" + bot.bot_id + " · Last Run #" + bot.latest_run_id + status;
  } else {
    runLine.textContent = "Bot #" + bot.bot_id + " · No runs yet";
  }

  const metrics = document.createElement("div");
  metrics.className = "console-bot-metrics";
  metrics.append(
    metric("Position", formatNumber(bot.position_quantity)),
    metric("Equity", formatMoney(bot.latest_equity)),
    metric("Realized PnL", formatMoney(bot.realized_pnl), pnlTone(bot.realized_pnl)),
    metric("Fees", formatMoney(bot.fees_paid))
  );

  const footer = document.createElement("div");
  footer.className = "console-bot-footer";
  const fills = bot.fill_count == null ? "No run activity" : (bot.fill_count + " fill" + (bot.fill_count === 1 ? "" : "s"));
  const updated = document.createElement("span");
  updated.textContent = fills + " · updated " + formatAge(bot.updated_at_ms);
  const action = document.createElement("strong");
  action.textContent = "Open Trading →";
  footer.append(updated, action);

  card.append(heading, runLine, metrics, footer);
  return card;
}

function renderOverview(payload) {
  const bots = Array.isArray(payload?.bots) ? payload.bots : [];
  const running = bots.filter((bot) => bot.runtime_state === "running").length;
  const idle = bots.length - running;

  botCount.textContent = bots.length + " " + (bots.length === 1 ? "bot" : "bots");
  runningCount.textContent = String(running);
  idleCount.textContent = String(idle);
  botGrid.replaceChildren();

  if (!bots.length) {
    const empty = document.createElement("div");
    empty.className = "console-empty";
    empty.textContent = "No persisted Bots yet. Create and configure Bots in Trading.";
    botGrid.appendChild(empty);
  } else {
    for (const bot of bots) botGrid.appendChild(renderBotCard(bot));
  }

  lastRefreshAt = Date.now();
  lastRefresh.textContent = new Intl.DateTimeFormat("en", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(lastRefreshAt));
  refreshLabel.textContent = "Bot state current";
  consoleStatus.textContent = "Read-only monitor · one compact refresh every 45 seconds · detailed streams stay in Trading.";
  topLabel.textContent = running ? (running + " Live-Paper running") : "Low-bandwidth monitor";
  setDot(refreshDot, "live");
  setDot(topDot, running ? "live" : "pending");
}

async function refreshConsole() {
  if (refreshInFlight || document.hidden) return;
  refreshInFlight = true;
  setDot(refreshDot, "loading");
  refreshLabel.textContent = "Refreshing Bot state…";
  try {
    const response = await fetch("/api/trading/console", {
      credentials: "same-origin",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) throw new Error(response.status + " " + response.statusText);
    renderOverview(await response.json());
  } catch (error) {
    setDot(refreshDot, "reconnecting");
    setDot(topDot, "reconnecting");
    refreshLabel.textContent = "Refresh failed";
    consoleStatus.textContent = "Could not refresh Bot state · " + error.message;
  } finally {
    refreshInFlight = false;
  }
}

function scheduleRefresh() {
  window.clearTimeout(refreshTimer);
  refreshTimer = window.setTimeout(async () => {
    await refreshConsole();
    scheduleRefresh();
  }, CONSOLE_REFRESH_MS);
}

document.addEventListener("visibilitychange", () => {
  if (!document.hidden && Date.now() - lastRefreshAt >= CONSOLE_REFRESH_MS) {
    void refreshConsole();
  }
});

themeToggle.addEventListener("click", () => {
  const nextTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  setTheme(nextTheme);
});

initializeTheme();
void refreshConsole();
scheduleRefresh();
