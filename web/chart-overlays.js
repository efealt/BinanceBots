class ChartTimeWindowOverlay {
  constructor(container, { bandClass, ariaLabel, windowsForRange }) {
    this.container = container;
    this.chart = null;
    this.candles = [];
    this.enabled = true;
    this.bandClass = bandClass;
    this.ariaLabel = ariaLabel;
    this.windowsForRange = windowsForRange;
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

  setEnabled(enabled) {
    this.enabled = enabled;
    this.render();
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
    if (!this.enabled || !this.chart || this.candles.length < 2) return;

    const firstTime = candleUnixSeconds(this.candles[0]);
    const lastTime = candleUnixSeconds(this.candles.at(-1));
    const coverageEnd = lastTime + candleStepSeconds(this.candles);
    const timeScale = this.chart.timeScale();

    for (const [start, end] of this.windowsForRange(firstTime, coverageEnd)) {
      if (end <= firstTime || start >= coverageEnd) continue;

      const bandStart = Math.max(start, firstTime);
      const bandEnd = Math.min(end, coverageEnd);
      const left = coordinateAtTime(timeScale, this.candles, bandStart);
      const right = coordinateAtTime(timeScale, this.candles, bandEnd);
      if (left === null || right === null || right <= left) continue;

      const band = document.createElement("div");
      band.className = `chart-overlay-band ${this.bandClass}`;
      band.style.left = `${left}px`;
      band.style.width = `${right - left}px`;
      band.setAttribute("aria-label", this.ariaLabel);
      this.root.appendChild(band);
    }
  }
}

class WeekendOverlay extends ChartTimeWindowOverlay {
  constructor(container) {
    super(container, {
      bandClass: "chart-overlay-band--weekend",
      ariaLabel: "Weekend · UTC",
      windowsForRange: (firstTime, lastTime) => weekendWindows(firstTime, lastTime),
    });
  }
}

class AfterHoursOverlay extends ChartTimeWindowOverlay {
  constructor(container, schedule = {
    startHour: 16,
    endHour: 20,
    label: "After-hours · 16:00–20:00 UTC",
  }) {
    super(container, {
      bandClass: "chart-overlay-band--after-hours",
      ariaLabel: schedule.label,
      windowsForRange: () => [],
    });
    this.schedule = schedule;
    this.windowsForRange = (firstTime, lastTime) => afterHoursWindows(firstTime, lastTime, this.schedule);
  }

  setSchedule(schedule) {
    this.schedule = schedule;
    this.ariaLabel = schedule.label;
    this.render();
  }
}

function candleUnixSeconds(candle) {
  const value = candle.open_time_ms ?? candle.open_time;
  return value > 10_000_000_000 ? Math.floor(value / 1000) : Math.floor(value);
}

function candleStepSeconds(candles) {
  for (let index = 1; index < candles.length; index += 1) {
    const step = candleUnixSeconds(candles[index]) - candleUnixSeconds(candles[index - 1]);
    if (step > 0) return step;
  }
  return 60;
}

function coordinateAtTime(timeScale, candles, targetTime) {
  const exact = timeScale.timeToCoordinate(targetTime);
  if (exact !== null && exact !== undefined) return exact;

  const afterIndex = candles.findIndex((candle) => candleUnixSeconds(candle) >= targetTime);
  if (afterIndex === 0) {
    const firstTime = candleUnixSeconds(candles[0]);
    const firstCoordinate = timeScale.timeToCoordinate(firstTime);
    const nextCoordinate = candles.length > 1
      ? timeScale.timeToCoordinate(candleUnixSeconds(candles[1]))
      : null;
    if (firstCoordinate === null || nextCoordinate === null) return null;
    const step = candleUnixSeconds(candles[1]) - firstTime;
    return firstCoordinate + ((targetTime - firstTime) / step) * (nextCoordinate - firstCoordinate);
  }

  if (afterIndex === -1) {
    const lastIndex = candles.length - 1;
    const lastTime = candleUnixSeconds(candles[lastIndex]);
    const lastCoordinate = timeScale.timeToCoordinate(lastTime);
    const previousCoordinate = timeScale.timeToCoordinate(candleUnixSeconds(candles[lastIndex - 1]));
    if (lastCoordinate === null || previousCoordinate === null) return null;
    const step = lastTime - candleUnixSeconds(candles[lastIndex - 1]);
    return lastCoordinate + ((targetTime - lastTime) / step) * (lastCoordinate - previousCoordinate);
  }

  const previous = candles[afterIndex - 1];
  const next = candles[afterIndex];
  const previousTime = candleUnixSeconds(previous);
  const nextTime = candleUnixSeconds(next);
  const previousCoordinate = timeScale.timeToCoordinate(previousTime);
  const nextCoordinate = timeScale.timeToCoordinate(nextTime);
  if (previousCoordinate === null || nextCoordinate === null) return null;
  return previousCoordinate + ((targetTime - previousTime) / (nextTime - previousTime)) * (nextCoordinate - previousCoordinate);
}

function weekendWindows(firstTime, lastTime) {
  const windows = [];
  const week = 7 * 24 * 60 * 60;
  const weekendLength = 2 * 24 * 60 * 60;
  for (let start = utcWeekendStart(firstTime); start <= lastTime; start += week) {
    windows.push([start, start + weekendLength]);
  }
  return windows;
}

function afterHoursWindows(firstTime, lastTime, schedule) {
  const windows = [];
  const day = 24 * 60 * 60;
  const endHour = schedule.endHour <= schedule.startHour
    ? schedule.endHour + 24
    : schedule.endHour;
  for (let start = utcDayStart(firstTime) - day; start <= lastTime; start += day) {
    windows.push([
      start + schedule.startHour * 60 * 60,
      start + endHour * 60 * 60,
    ]);
  }
  return windows;
}

function utcDayStart(unixSeconds) {
  const date = new Date(unixSeconds * 1000);
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()) / 1000;
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
