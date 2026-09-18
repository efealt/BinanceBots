# Binance Grid

Local Rust dashboard and market-data foundation for the trading system.

Run `cargo run`, then open `http://127.0.0.1:8080`.

To capture one minute of public Binance WebSocket market data into SQLite:

```text
cargo run -- --capture-live --duration-seconds 60
```

The default capture is BTCUSDT Spot and writes to `data/binance_grid.sqlite3`. Use `--symbol`, `--interval`, `--market-type`, `--duration-seconds`, and `--database` to change it. No API keys are used.
