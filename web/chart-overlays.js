class WeekendOverlay {
  constructor(container) {
    this.container = container;
    this.chart = null;
    this.candles = [];
    this.root = document.createElement("div");
    this.root.className = "chart-overlay";
    this.container.appendChild(this.root);
    this.handleScaleChange = () => this.render();
  }

  attach(chart) {
    this.chart = chart;
    const timeScale = chart.timeScale();
    timeScale.subscribeVisibleTimeRangeChange(this.handleScaleChange);
    timeScale.subscribeSizeChange(this.handleScaleChange);
  }

  setCandles(candles) {
    this.candles = candles;
    this.render();
  }

  reset() {
    this.candles = [];
    this.root.replaceChildren();
  }

  render() {
    this.root.replaceChildren();
    if (!this.chart || this.candles.length < 2) return;

    const firstTime = Math.floor(this.candles[0].open_time / 1000);
    const lastTime = Math.floor(this.candles[this.candles.length - 1].open_time / 1000);
    const firstWeekend = utcWeekendStart(firstTime);
    const week = 7 * 24 * 60 * 60;
    const weekendLength = 2 * 24 * 60 * 60;
    const timeScale = this.chart.timeScale();

    for (let start = firstWeekend; start <= lastTime; start += week) {
      const end = start + weekendLength;
      if (end <= firstTime || start >= lastTime) continue;

      const bandStart = Math.max(start, firstTime);
      const bandEnd = Math.min(end, lastTime);
      const startCandle = candleAtOrAfter(this.candles, bandStart) || this.candles[0];
      const endCandle = candleAtOrAfter(this.candles, bandEnd) || this.candles.at(-1);
      const left = timeScale.timeToCoordinate(Math.floor(startCandle.open_time / 1000));
      const right = timeScale.timeToCoordinate(Math.floor(endCandle.open_time / 1000));
      if (left === null || right === null) continue;

      const band = document.createElement("div");
      band.className = "chart-overlay-band";
      band.style.left = `${Math.min(left, right)}px`;
      band.style.width = `${Math.max(0, Math.abs(right - left))}px`;
      band.setAttribute("aria-label", "Weekend · UTC");
      this.root.appendChild(band);
    }
  }
}

function candleAtOrAfter(candles, targetTime) {
  return candles.find((candle) => Math.floor(candle.open_time / 1000) >= targetTime);
}

function utcWeekendStart(unixSeconds) {
  const date = new Date(unixSeconds * 1000);
  const daysSinceSaturday = (date.getUTCDay() + 1) % 7;
  return Date.UTC(
    date.getUTCFullYear(),
    date.getUTCMonth(),
    date.getUTCDate() - daysSinceSaturday,
  ) / 1000;
}
