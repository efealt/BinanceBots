window.ChartIndicatorCalculations = (() => {
  function normalizePeriod(value, fallback = 20) {
    const period = Number(value);
    if (!Number.isFinite(period)) return fallback;
    return Math.min(500, Math.max(2, Math.round(period)));
  }

  function normalizeMultiplier(value, fallback = 2) {
    const multiplier = Number(value);
    if (!Number.isFinite(multiplier)) return fallback;
    return Math.min(10, Math.max(0.1, Math.round(multiplier * 10) / 10));
  }

  function closeValue(candle) {
    const close = Number(candle.close);
    return Number.isFinite(close) ? close : null;
  }

  function timeValue(candle) {
    return Math.floor(candle.open_time / 1000);
  }

  function simpleMovingAverage(candles, period) {
    const result = [];
    const window = [];
    let sum = 0;

    for (const candle of candles) {
      const close = closeValue(candle);
      if (close === null) {
        window.length = 0;
        sum = 0;
        continue;
      }

      window.push(close);
      sum += close;
      if (window.length > period) sum -= window.shift();
      if (window.length === period) {
        result.push({ time: timeValue(candle), value: sum / period });
      }
    }

    return result;
  }

  function bollingerBands(candles, period, multiplier) {
    const result = [];
    const window = [];
    let sum = 0;
    let sumSquares = 0;

    for (const candle of candles) {
      const close = closeValue(candle);
      if (close === null) {
        window.length = 0;
        sum = 0;
        sumSquares = 0;
        continue;
      }

      window.push(close);
      sum += close;
      sumSquares += close * close;

      if (window.length > period) {
        const removed = window.shift();
        sum -= removed;
        sumSquares -= removed * removed;
      }

      if (window.length === period) {
        const middle = sum / period;
        const variance = Math.max(0, sumSquares / period - middle * middle);
        const deviation = Math.sqrt(variance) * multiplier;
        result.push({
          time: timeValue(candle),
          middle,
          upper: middle + deviation,
          lower: middle - deviation,
        });
      }
    }

    return result;
  }

  return {
    normalizePeriod,
    normalizeMultiplier,
    simpleMovingAverage,
    bollingerBands,
  };
})();
