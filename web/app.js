const botStrip = document.querySelector("#bot-strip");
const botCount = document.querySelector("#bot-count");
const consoleDetail = document.querySelector("#console-detail");
const themeToggle = document.querySelector("#theme-toggle");

const bots = [];
let selectedBotId = null;

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("theme", theme);
}

function initializeTheme() {
  const savedTheme = localStorage.getItem("theme");
  const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  setTheme(savedTheme ?? systemTheme);
}

function selectedBot() {
  return bots.find((bot) => bot.id === selectedBotId);
}

function renderBotStrip() {
  botCount.textContent = `${bots.length} ${bots.length === 1 ? "bot" : "bots"}`;
  const cards = bots
    .map(
      (bot) => `
        <button class="bot-card ${bot.id === selectedBotId ? "active" : ""}" type="button" data-bot-id="${bot.id}">
          <span class="bot-card-symbol">${bot.symbol}</span>
          <span>${bot.strategy}</span>
          <small>Not running</small>
        </button>`,
    )
    .join("");

  botStrip.innerHTML = `
    <button class="bot-create-card ${selectedBotId === null ? "active" : ""}" type="button" data-action="create">
      <span class="create-mark">+</span>
      <span>Create bot</span>
      <small>Choose strategy + asset</small>
    </button>
    ${cards}`;

  botStrip.querySelector('[data-action="create"]').addEventListener("click", () => {
    selectedBotId = null;
    render();
  });

  botStrip.querySelectorAll("[data-bot-id]").forEach((card) => {
    card.addEventListener("click", () => {
      selectedBotId = card.dataset.botId;
      render();
    });
  });
}

function renderCreateBot() {
  consoleDetail.innerHTML = `
    <article class="panel create-bot-panel">
      <div class="panel-heading">
        <div>
          <p class="eyebrow">New bot</p>
          <h2>Create bot</h2>
        </div>
          <span class="badge badge--muted">UI preview</span>
      </div>

      <form id="create-bot-form" class="form-grid">
        <label>
          Strategy script
          <select name="strategy" required>
            <option value="Grid strategy">Grid strategy</option>
          </select>
        </label>
        <label>
          Asset
          <select name="symbol" required>
            <option value="BTCUSDT">BTCUSDT</option>
            <option value="XAGUSDT">XAGUSDT</option>
          </select>
        </label>
        <label>
          Bot name
          <input name="name" value="Grid bot" required />
        </label>
      </form>

      <button id="create-bot-button" class="button button--primary" type="submit" form="create-bot-form">Create bot</button>
      <p class="panel-note">This creates a local UI card only. It does not start trading or create an exchange connection.</p>
    </article>`;

  document.querySelector("#create-bot-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const formData = new FormData(event.currentTarget);
    const id = crypto.randomUUID();
    bots.push({
      id,
      strategy: formData.get("strategy"),
      symbol: formData.get("symbol"),
      name: formData.get("name"),
    });
    selectedBotId = id;
    render();
  });
}

function renderSelectedBot(bot) {
  consoleDetail.innerHTML = `
    <section class="detail-stack">
      <article class="panel selected-bot-panel">
        <div class="panel-heading">
          <div>
            <p class="eyebrow">Selected bot</p>
            <h2>${bot.name} · ${bot.symbol}</h2>
          </div>
          <span class="badge badge--muted">Not running</span>
        </div>
        <p class="panel-note">${bot.strategy}. This bot is a local UI preview only; no strategy runtime or exchange connection exists yet.</p>
      </article>

      <article class="panel chart-panel">
        <div class="panel-heading">
          <div>
            <p class="eyebrow">Market view</p>
            <h2>Chart &amp; grid view</h2>
          </div>
          <span class="badge badge--muted">Waiting for market data</span>
        </div>
        <div class="chart-placeholder" role="img" aria-label="Empty chart placeholder">
          <div class="chart-axis chart-axis-y"><span>Price</span></div>
          <div class="chart-axis chart-axis-x"><span>Time</span></div>
          <div class="empty-chart-copy">
            <strong>No market feed yet</strong>
            <span>Price, grid levels, fills, and paper/live state for ${bot.symbol} will render here.</span>
          </div>
        </div>
      </article>

      <section class="workspace-grid lower-grid">
        <article class="panel">
          <div class="panel-heading">
            <div>
              <p class="eyebrow">Inventory</p>
              <h2>Open positions</h2>
            </div>
            <span class="badge badge--muted">0 positions</span>
          </div>
          <div class="empty-table">No positions for this bot.</div>
        </article>
        <article class="panel">
          <div class="panel-heading">
            <div>
              <p class="eyebrow">Activity</p>
              <h2>Orders &amp; fills</h2>
            </div>
            <span class="badge badge--muted">No activity</span>
          </div>
          <div class="empty-table">No orders or fills for this bot.</div>
        </article>
      </section>
    </section>`;
}

function render() {
  renderBotStrip();
  const bot = selectedBot();
  if (bot) {
    renderSelectedBot(bot);
  } else {
    renderCreateBot();
  }
}

themeToggle.addEventListener("click", () => {
  const nextTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  setTheme(nextTheme);
});

initializeTheme();
render();
