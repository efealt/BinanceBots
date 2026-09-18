const form = document.querySelector("#download-entry-form");
const saveButton = document.querySelector("#save-download");
const saveFeedback = document.querySelector("#save-feedback");
const catalogStatus = document.querySelector("#catalog-status");
const downloadCount = document.querySelector("#download-count");
const tableWrap = document.querySelector("#download-table-wrap");
const tableBody = document.querySelector("#download-table-body");
const emptyState = document.querySelector("#download-empty");
const themeToggle = document.querySelector("#theme-toggle");

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("theme", theme);
}

function initializeTheme() {
  const savedTheme = localStorage.getItem("theme");
  const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  setTheme(savedTheme ?? systemTheme);
}

function formatDate(timestamp) {
  if (timestamp === null || timestamp === undefined) return "—";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(timestamp));
}

function marketLabel(marketType) {
  return marketType === "spot" ? "Spot" : "Futures · USDⓈ-M";
}

function intervalLabel(interval) {
  const labels = {
    "1m": "1 minute",
    "3m": "3 minutes",
    "5m": "5 minutes",
    "15m": "15 minutes",
    "30m": "30 minutes",
    "1h": "1 hour",
    "2h": "2 hours",
    "4h": "4 hours",
    "6h": "6 hours",
    "8h": "8 hours",
    "12h": "12 hours",
    "1d": "1 day",
    "3d": "3 days",
    "1w": "1 week",
  };
  return labels[interval] ?? interval;
}

function setFeedback(message, tone = "") {
  saveFeedback.textContent = message;
  saveFeedback.dataset.tone = tone;
}

function textCell(value, className = "") {
  const cell = document.createElement("td");
  cell.textContent = value;
  if (className) cell.className = className;
  return cell;
}

function renderDownloads(downloads) {
  tableBody.replaceChildren();
  downloadCount.textContent = `${downloads.length} ${downloads.length === 1 ? "entry" : "entries"}`;

  if (downloads.length === 0) {
    tableWrap.hidden = true;
    emptyState.hidden = false;
    catalogStatus.textContent = "No saved entries.";
    return;
  }

  tableWrap.hidden = false;
  emptyState.hidden = true;
  catalogStatus.textContent = "Saved definitions are stored locally in SQLite.";

  for (const download of downloads) {
    const row = document.createElement("tr");
    row.append(
      textCell(download.name),
      textCell(download.symbol),
      textCell(marketLabel(download.market_type)),
      textCell(download.provider === "binance" ? "Binance" : download.provider),
      textCell(intervalLabel(download.interval)),
      textCell(formatDate(download.last_downloaded_at_ms)),
      textCell(formatDate(download.data_start_time_ms)),
      textCell(formatDate(download.data_end_time_ms)),
    );

    const actionCell = document.createElement("td");
    const actionButton = document.createElement("button");
    actionButton.type = "button";
    actionButton.className = "data-download-action";
    actionButton.textContent = "Download";
    actionButton.addEventListener("click", () => {
      setFeedback("Downloading will be wired in step 2.", "info");
    });
    actionCell.append(actionButton);
    row.append(actionCell);
    tableBody.append(row);
  }
}

async function loadDownloads() {
  catalogStatus.textContent = "Loading saved entries…";
  try {
    const response = await fetch("/api/data/downloads");
    if (!response.ok) throw new Error(await response.text());
    const payload = await response.json();
    renderDownloads(payload.downloads);
  } catch (error) {
    tableWrap.hidden = true;
    emptyState.hidden = true;
    downloadCount.textContent = "Unavailable";
    catalogStatus.textContent = `Could not load saved entries: ${error.message}`;
  }
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  saveButton.disabled = true;
  setFeedback("Saving…");

  const payload = Object.fromEntries(new FormData(form).entries());
  try {
    const response = await fetch("/api/data/downloads", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) throw new Error(await response.text());
    const saved = await response.json();
    form.reset();
    setFeedback(`Saved ${saved.name}.`, "success");
    await loadDownloads();
  } catch (error) {
    setFeedback(error.message || "Could not save the data entry.", "error");
  } finally {
    saveButton.disabled = false;
  }
});

themeToggle.addEventListener("click", () => {
  const nextTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  setTheme(nextTheme);
});

initializeTheme();
loadDownloads();
