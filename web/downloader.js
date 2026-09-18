const form = document.querySelector("#download-entry-form");
const saveButton = document.querySelector("#save-download");
const saveFeedback = document.querySelector("#save-feedback");
const catalogStatus = document.querySelector("#catalog-status");
const downloadCount = document.querySelector("#download-count");
const tableWrap = document.querySelector("#download-table-wrap");
const tableBody = document.querySelector("#download-table-body");
const emptyState = document.querySelector("#download-empty");
const themeToggle = document.querySelector("#theme-toggle");
const editDialog = document.querySelector("#edit-download-dialog");
const editForm = document.querySelector("#edit-download-form");
const editSummary = document.querySelector("#edit-download-summary");
const editStartDate = document.querySelector("#edit-download-start-date");
const editSaveButton = document.querySelector("#save-edit-download");
const editFeedback = document.querySelector("#edit-download-feedback");
const cancelEditButton = document.querySelector("#cancel-edit-download");
const cancelEditIcon = document.querySelector("#cancel-edit-download-icon");
let editingDownloadId = null;

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
  const formatted = new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
    timeZone: "UTC",
  }).format(new Date(timestamp));
  return `${formatted} UTC`;
}

function formatDay(timestamp) {
  if (timestamp === null || timestamp === undefined) return "—";
  return new Date(timestamp).toISOString().slice(0, 10);
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

function setEditFeedback(message, tone = "") {
  editFeedback.textContent = message;
  editFeedback.dataset.tone = tone;
}

function openEditDialog(download) {
  editingDownloadId = download.download_id;
  editSummary.textContent = `${download.name} · ${download.symbol} · ${marketLabel(download.market_type)}`;
  editStartDate.value = formatDay(download.requested_start_time_ms) === "—"
    ? ""
    : formatDay(download.requested_start_time_ms);
  setEditFeedback("");
  editSaveButton.disabled = false;
  editDialog.showModal();
  editStartDate.focus();
}

function closeEditDialog() {
  if (editDialog.open) editDialog.close();
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
  catalogStatus.textContent = "ZIP imports use completed-month archives and daily files through yesterday.";

  for (const download of downloads) {
    const row = document.createElement("tr");
    row.append(
      textCell(download.name),
      textCell(download.symbol),
      textCell(marketLabel(download.market_type)),
      textCell(download.provider === "binance" ? "Binance" : download.provider),
      textCell(intervalLabel(download.interval)),
      textCell(formatDay(download.requested_start_time_ms)),
      textCell(formatDate(download.last_downloaded_at_ms)),
      textCell(formatDate(download.data_start_time_ms)),
      textCell(formatDate(download.data_end_time_ms)),
    );

    const actionCell = document.createElement("td");
    actionCell.className = "download-actions";
    const actionButton = document.createElement("button");
    actionButton.type = "button";
    actionButton.className = "data-download-action";
    actionButton.textContent = "Download missing";
    let startDate;
    if (download.requested_start_time_ms === null || download.requested_start_time_ms === undefined) {
      startDate = document.createElement("input");
      startDate.type = "date";
      startDate.className = "download-start-date";
      startDate.setAttribute("aria-label", `Start date for ${download.name}`);
      actionCell.append(startDate);
    }
    actionButton.addEventListener("click", async () => {
      const payload = startDate ? { start_date: startDate.value } : {};
      if (startDate && !startDate.value) {
        setFeedback("Choose a UTC start date before downloading.", "error");
        return;
      }
      actionButton.disabled = true;
      if (startDate) startDate.disabled = true;
      actionButton.textContent = "Downloading…";
      setFeedback(`Downloading missing ZIP archives for ${download.name}…`, "info");
      try {
        const response = await fetch(`/api/data/downloads/${download.download_id}/run`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        });
        if (!response.ok) throw new Error(await response.text());
        const result = await response.json();
        const failed = result.failed_archives.length;
        const message = `Imported ${result.rows_imported.toLocaleString()} candles from ${result.archives_imported} ZIP ${result.archives_imported === 1 ? "archive" : "archives"}${result.archives_already_present ? `; ${result.archives_already_present} already present` : ""}${failed ? `; ${failed} archive ${failed === 1 ? "needs" : "need"} retry` : ""}.`;
        setFeedback(message, failed ? "error" : "success");
        await loadDownloads();
      } catch (error) {
        setFeedback(error.message || "Could not import the archive data.", "error");
        actionButton.disabled = false;
        if (startDate) startDate.disabled = false;
        actionButton.textContent = "Download missing";
      }
    });
    const editButton = document.createElement("button");
    editButton.type = "button";
    editButton.className = "data-download-edit";
    editButton.textContent = "✎";
    editButton.title = "Edit catalog entry";
    editButton.setAttribute("aria-label", `Edit start date for ${download.name}`);
    editButton.addEventListener("click", () => openEditDialog(download));
    actionCell.append(actionButton, editButton);
    row.append(actionCell);
    tableBody.append(row);
  }
}

editForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!editingDownloadId) return;
  if (!editStartDate.value) {
    setEditFeedback("Choose a UTC start date.", "error");
    return;
  }

  editSaveButton.disabled = true;
  setEditFeedback("Saving start date…", "info");
  try {
    const response = await fetch(`/api/data/downloads/${editingDownloadId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ start_date: editStartDate.value }),
    });
    if (!response.ok) throw new Error(await response.text());
    const saved = await response.json();
    closeEditDialog();
    setFeedback(`Updated ${saved.name}. Click Download missing to backfill from ${formatDay(saved.requested_start_time_ms)}.`, "success");
    await loadDownloads();
  } catch (error) {
    setEditFeedback(error.message || "Could not update the catalog entry.", "error");
    editSaveButton.disabled = false;
  }
});

cancelEditButton.addEventListener("click", closeEditDialog);
cancelEditIcon.addEventListener("click", closeEditDialog);
editDialog.addEventListener("close", () => {
  editingDownloadId = null;
  editSaveButton.disabled = false;
});

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
