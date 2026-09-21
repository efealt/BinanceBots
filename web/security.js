const themeToggle = document.querySelector("#theme-toggle");
const count = document.querySelector("#auth-event-count");
const status = document.querySelector("#auth-event-status");
const tableWrap = document.querySelector("#auth-event-table-wrap");
const tableBody = document.querySelector("#auth-event-table-body");
const emptyState = document.querySelector("#auth-event-empty");

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("theme", theme);
}

function initializeTheme() {
  const savedTheme = localStorage.getItem("theme");
  const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  setTheme(savedTheme ?? systemTheme);
}

function formatUtc(timestamp) {
  if (!Number.isFinite(Number(timestamp))) return "—";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "medium",
    timeZone: "UTC",
  }).format(new Date(Number(timestamp))) + " UTC";
}

function eventLabel(eventType) {
  return {
    login_success: "Login success",
    login_failed: "Login failed",
    logout: "Logout",
  }[eventType] ?? eventType;
}

function textCell(value) {
  const cell = document.createElement("td");
  cell.textContent = value ?? "—";
  return cell;
}

function render(events) {
  tableBody.replaceChildren();
  count.textContent = `${events.length} ${events.length === 1 ? "event" : "events"}`;

  if (!events.length) {
    tableWrap.hidden = true;
    emptyState.hidden = false;
    status.textContent = "No authentication events recorded yet.";
    return;
  }

  tableWrap.hidden = false;
  emptyState.hidden = true;
  status.textContent = "Newest first · up to 200 persisted authentication events.";

  for (const event of events) {
    const row = document.createElement("tr");
    row.append(
      textCell(formatUtc(event.occurred_at_ms)),
      textCell(eventLabel(event.event_type)),
      textCell(event.source_ip || "—"),
      textCell(event.user_agent || "—"),
    );
    tableBody.append(row);
  }
}

async function loadEvents() {
  try {
    const response = await fetch("/api/security/auth-events");
    if (!response.ok) throw new Error(await response.text());
    const payload = await response.json();
    render(payload.events ?? []);
  } catch (error) {
    tableWrap.hidden = true;
    emptyState.hidden = true;
    count.textContent = "Unavailable";
    status.textContent = error.message || "Could not load authentication events.";
  }
}

themeToggle.addEventListener("click", () => {
  setTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark");
});

initializeTheme();
loadEvents();
