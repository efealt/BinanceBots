class MarketChart {
  constructor(container, { showWeekends = false } = {}) {
    this.container = container;
    this.chart = null;
    this.series = null;
    this.indicatorLayer = null;
    this.candleCount = 0;
    this.firstCandleTime = null;
    this.weekendOverlay = showWeekends ? new WeekendOverlay(container) : null;
  }

  initialize() {
    this.chart = LightweightCharts.createChart(this.container, this.options());
    this.series = this.chart.addSeries(LightweightCharts.CandlestickSeries, {
      upColor: "#36c984",
      downColor: "#eb6f92",
      borderVisible: false,
      wickUpColor: "#36c984",
      wickDownColor: "#eb6f92",
    });
    this.indicatorLayer = new ChartIndicatorLayer(this.chart);
    this.weekendOverlay?.attach(this.chart);

    new ResizeObserver(([entry]) => {
      this.chart.applyOptions({
        width: entry.contentRect.width,
        height: entry.contentRect.height,
      });
    }).observe(this.container);
  }

  setCandles(candles, fitContent = false) {
    const data = candles.map((candle) => ({
      time: Math.floor(candle.open_time / 1000),
      open: candle.open,
      high: candle.high,
      low: candle.low,
      close: candle.close,
    }));

    if (!data.length) return;

    const firstCandleChanged = data[0].time !== this.firstCandleTime;
    const bufferChanged = data.length !== this.candleCount || firstCandleChanged;

    if (!this.candleCount || bufferChanged) {
      this.series.setData(data);
    } else {
      this.series.update(data[data.length - 1]);
    }

    this.candleCount = data.length;
    this.firstCandleTime = data[0].time;
    this.indicatorLayer?.setCandles(candles);
    this.weekendOverlay?.setCandles(candles);
    if (fitContent) this.chart.timeScale().fitContent();
    this.weekendOverlay?.render();
  }

  reset() {
    this.series.setData([]);
    this.indicatorLayer?.reset();
    this.candleCount = 0;
    this.firstCandleTime = null;
    this.weekendOverlay?.reset();
  }

  applyTheme() {
    if (!this.chart) return;
    this.chart.applyOptions(this.options());
    this.indicatorLayer?.applyTheme();
  }

  options() {
    const styles = getComputedStyle(document.documentElement);
    const surface = styles.getPropertyValue("--surface-raised").trim();
    const text = styles.getPropertyValue("--muted").trim();
    const grid = styles.getPropertyValue("--border").trim();

    return {
      autoSize: true,
      layout: {
        background: { type: "solid", color: surface },
        textColor: text,
      },
      grid: {
        vertLines: { color: grid },
        horzLines: { color: grid },
      },
      crosshair: {
        mode: LightweightCharts.CrosshairMode.Normal,
      },
      rightPriceScale: {
        borderColor: grid,
      },
      timeScale: {
        borderColor: grid,
        timeVisible: true,
        secondsVisible: false,
      },
    };
  }
}

const marketChart = new MarketChart(document.querySelector("#market-chart"), { showWeekends: true });
marketChart.initialize();
