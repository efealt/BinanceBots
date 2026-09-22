(function (root, factory) {
  const api = factory();
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  if (root) root.BacktestAnalysisMath = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  function finiteNumber(value, label) {
    const number = Number(value);
    if (!Number.isFinite(number)) throw new Error(`${label} must be finite`);
    return number;
  }

  function buildBuyAndHold(candles, initialCapital) {
    if (!Array.isArray(candles) || !candles.length) {
      return { entryPrice: null, quantity: 0, equity: [], finalEquity: null, totalReturnPercent: null };
    }
    const capital = finiteNumber(initialCapital, "initial capital");
    const entryPrice = finiteNumber(candles[0].open_price, "first active candle open");
    if (capital <= 0 || entryPrice <= 0) throw new Error("buy-and-hold benchmark requires positive capital and entry price");
    const quantity = capital / entryPrice;
    const equity = candles.map((candle) => {
      const close = finiteNumber(candle.close_price, "candle close");
      return [Number(candle.close_time_ms), quantity * close];
    });
    const finalEquity = equity[equity.length - 1][1];
    return {
      entryPrice,
      quantity,
      equity,
      finalEquity,
      totalReturnPercent: ((finalEquity / capital) - 1) * 100,
    };
  }

  function drawdownSeries(equityPoints) {
    let peak = Number.NEGATIVE_INFINITY;
    let maxDrawdownPercent = 0;
    const points = [];
    for (const point of equityPoints || []) {
      const time = Number(point[0]);
      const value = finiteNumber(point[1], "equity");
      if (value > peak) peak = value;
      const drawdown = peak > 0 ? ((value / peak) - 1) * 100 : 0;
      if (drawdown < maxDrawdownPercent) maxDrawdownPercent = drawdown;
      points.push([time, drawdown]);
    }
    return { points, maxDrawdownPercent };
  }

  function buildPositionExposure(candles, equityPoints, positionEvents) {
    const equityByTime = new Map((equityPoints || []).map((point) => [Number(point[0]), finiteNumber(point[1], "equity")]));
    const events = [...(positionEvents || [])]
      .map((event) => ({
        event_time_ms: Number(event.event_time_ms),
        position_quantity: finiteNumber(event.position_quantity, "position quantity"),
      }))
      .sort((left, right) => left.event_time_ms - right.event_time_ms);

    const position = [];
    const exposure = [];
    let eventIndex = 0;
    let currentQuantity = 0;

    for (const candle of candles || []) {
      const time = Number(candle.close_time_ms);
      while (eventIndex < events.length && events[eventIndex].event_time_ms <= time) {
        currentQuantity = events[eventIndex].position_quantity;
        eventIndex += 1;
      }
      const equity = equityByTime.get(time);
      const close = finiteNumber(candle.close_price, "candle close");
      const exposurePercent = equity && equity !== 0
        ? Math.abs(currentQuantity * close) / Math.abs(equity) * 100
        : 0;
      position.push([time, currentQuantity]);
      exposure.push([time, exposurePercent]);
    }

    return { position, exposure };
  }

  return { buildBuyAndHold, drawdownSeries, buildPositionExposure };
});
