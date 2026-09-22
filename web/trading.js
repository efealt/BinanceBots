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
});

initializeTheme();
syncFixedAnchorState();
setConfigLocked(false);
initializeTradingStatus();
