const themeToggle = document.querySelector("#theme-toggle");
const lastUpdated = document.querySelector("#last-updated");

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("theme", theme);
}

function initializeTheme() {
  const savedTheme = localStorage.getItem("theme");
  const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  setTheme(savedTheme ?? systemTheme);
}

function renderStatus(status) {
  for (const key of ["exchange", "execution", "storage"]) {
    const item = status[key];
    document.querySelector(`#${key}-title`).textContent = item.title;
    document.querySelector(`#${key}-detail`).textContent = item.detail;

    const indicator = document.querySelector(`#${key}-state`);
    indicator.className = `status-dot ${item.state}`;
    indicator.setAttribute("aria-label", item.state);
  }

  lastUpdated.textContent = `Local service checked ${new Date().toLocaleTimeString()}`;
}

async function refreshStatus() {
  try {
    const response = await fetch("/api/status", { cache: "no-store" });
    if (!response.ok) throw new Error("Status request failed");
    renderStatus(await response.json());
  } catch {
    lastUpdated.textContent = "Local service unavailable";
  }
}

themeToggle.addEventListener("click", () => {
  const nextTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  setTheme(nextTheme);
});

initializeTheme();
refreshStatus();
window.setInterval(refreshStatus, 10_000);
