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

let stream = null;
let reconnectTimer = null;
let currentRunId = null;
let currentSnapshot = null;

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

themeToggle.addEventListener("click", () => {
  const nextTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  setTheme(nextTheme);
});

initializeTheme();
initializeTradingStatus();
