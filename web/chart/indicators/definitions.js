window.ChartIndicatorDefinitions = (() => {
  const calculations = window.ChartIndicatorCalculations;

  const definitions = {
    sma: {
      label: "Simple moving average",
      shortLabel: "SMA",
      defaults: { period: 20 },
      parameters: [
        { key: "period", label: "Period", min: 2, max: 500, step: 1 },
      ],
      normalize(parameters) {
        return {
          period: calculations.normalizePeriod(parameters.period),
        };
      },
      series(candles, parameters) {
        return [
          {
            key: "line",
            title: "SMA",
            colorToken: "--chart-sma",
            data: calculations.simpleMovingAverage(candles, parameters.period),
          },
        ];
      },
    },
    bollinger: {
      label: "Bollinger Bands",
      shortLabel: "BB",
      defaults: { period: 20, multiplier: 2 },
      parameters: [
        { key: "period", label: "Period", min: 2, max: 500, step: 1 },
        { key: "multiplier", label: "Std dev", min: 0.1, max: 10, step: 0.1 },
      ],
      normalize(parameters) {
        return {
          period: calculations.normalizePeriod(parameters.period),
          multiplier: calculations.normalizeMultiplier(parameters.multiplier),
        };
      },
      series(candles, parameters) {
        const values = calculations.bollingerBands(
          candles,
          parameters.period,
          parameters.multiplier,
        );

        return [
          {
            key: "upper",
            title: "BB upper",
            colorToken: "--chart-bollinger-edge",
            data: values.map(({ time, upper }) => ({ time, value: upper })),
          },
          {
            key: "middle",
            title: "BB middle",
            colorToken: "--chart-bollinger-middle",
            data: values.map(({ time, middle }) => ({ time, value: middle })),
          },
          {
            key: "lower",
            title: "BB lower",
            colorToken: "--chart-bollinger-edge",
            data: values.map(({ time, lower }) => ({ time, value: lower })),
          },
        ];
      },
    },
  };

  return {
    get(type) {
      return definitions[type] ?? null;
    },
    entries() {
      return Object.entries(definitions);
    },
  };
})();
