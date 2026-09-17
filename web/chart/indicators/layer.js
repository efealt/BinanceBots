window.ChartIndicatorLayer = class ChartIndicatorLayer {
  constructor(chart) {
    this.chart = chart;
    this.candles = [];
    this.items = new Map();
  }

  add(type, parameters = {}) {
    const definition = window.ChartIndicatorDefinitions.get(type);
    if (!definition || this.items.has(type)) return null;

    const item = {
      type,
      parameters: definition.normalize({ ...definition.defaults, ...parameters }),
      series: [],
    };

    for (const specification of definition.series([], item.parameters)) {
      const series = this.chart.addSeries(LightweightCharts.LineSeries, {
        color: this.color(specification.colorToken),
        lineWidth: 2,
        priceLineVisible: false,
        lastValueVisible: false,
        crosshairMarkerVisible: false,
        title: specification.title,
      });
      item.series.push({ specification, series });
    }

    this.items.set(type, item);
    this.renderItem(item);
    return this.snapshot(item);
  }

  remove(type) {
    const item = this.items.get(type);
    if (!item) return false;

    for (const entry of item.series) this.chart.removeSeries(entry.series);
    this.items.delete(type);
    return true;
  }

  updateParameters(type, parameters) {
    const item = this.items.get(type);
    const definition = window.ChartIndicatorDefinitions.get(type);
    if (!item || !definition) return null;

    item.parameters = definition.normalize({ ...item.parameters, ...parameters });
    this.renderItem(item);
    return this.snapshot(item);
  }

  setCandles(candles) {
    this.candles = candles;
    for (const item of this.items.values()) this.renderItem(item);
  }

  reset() {
    this.candles = [];
    for (const item of this.items.values()) this.renderItem(item);
  }

  applyTheme() {
    for (const item of this.items.values()) {
      for (const entry of item.series) {
        entry.series.applyOptions({ color: this.color(entry.specification.colorToken) });
      }
    }
  }

  snapshots() {
    return [...this.items.values()].map((item) => this.snapshot(item));
  }

  renderItem(item) {
    const definition = window.ChartIndicatorDefinitions.get(item.type);
    const specifications = definition.series(this.candles, item.parameters);

    specifications.forEach((specification, index) => {
      const entry = item.series[index];
      entry.specification = specification;
      entry.series.setData(specification.data);
    });
  }

  snapshot(item) {
    return {
      type: item.type,
      parameters: { ...item.parameters },
    };
  }

  color(token) {
    return getComputedStyle(document.documentElement).getPropertyValue(token).trim();
  }
};
