const fs = require("fs");

const html = fs.readFileSync("web/index.html", "utf8");
const js = fs.readFileSync("web/app.js", "utf8");
const css = fs.readFileSync("web/styles.css", "utf8");
const tradingJs = fs.readFileSync("web/trading.js", "utf8");
const tradingApi = fs.readFileSync("src/api/trading.rs", "utf8");

for (const required of [
  'id="console-bot-grid"',
  'id="console-running-count"',
  'id="console-idle-count"',
  'id="console-last-refresh"',
  'Read-only · refreshes about every 45 seconds',
  '/app.js?v=2',
]) {
  if (!html.includes(required)) throw new Error("Console shell missing: " + required);
}

for (const required of [
  "const CONSOLE_REFRESH_MS = 45_000",
  'fetch("/api/trading/console"',
  'card.href = "/trading.html?bot_id="',
  '"Running · Live-Paper"',
  '"Realized PnL"',
  '"Equity"',
  '"Position"',
  '"Fees"',
  "document.hidden",
]) {
  if (!js.includes(required)) throw new Error("Console monitor contract missing: " + required);
}

if (js.includes("new WebSocket") || js.includes("WebSocket(")) {
  throw new Error("Console must not open a WebSocket");
}
if (js.includes("/api/market/") || js.includes("/stream")) {
  throw new Error("Console must not duplicate Trading market/run streams");
}
if (/\b(start|stop|create|update)[A-Za-z]*Bot\b/.test(js) || js.includes('method: "POST"') || js.includes('method: "PUT"')) {
  throw new Error("Console must remain read-only");
}

for (const required of [
  ".console-page { display: grid;",
  ".console-bot-grid { display: grid;",
  "minmax(min(100%, 300px), 1fr)",
  ".console-bot-metrics { display: grid;",
  "@media (max-width: 900px)",
]) {
  if (!css.includes(required)) throw new Error("Console responsive CSS missing: " + required);
}

if (!tradingJs.includes('new URLSearchParams(window.location.search).get("bot_id")')) {
  throw new Error("Trading must select a Bot supplied by the Console deep link");
}

if (!tradingApi.includes('.route("/api/trading/console", get(console_overview))')) {
  throw new Error("Trading API missing compact Console endpoint");
}
if (!tradingApi.includes("async fn console_overview(") || !tradingApi.includes("console_bot_summary(")) {
  throw new Error("Trading API missing Console summary construction");
}

console.log("Lightweight Console contract OK");
