-- Development-only trading-state reset requested before the realtime Live-Paper execution path.
-- Preserve downloaded/historical market data and live-capture tables.
-- Remove only Bot/Run operational trading state; child run records cascade from trading_runs.

DELETE FROM trading_runs;
DELETE FROM trading_bots;

DELETE FROM sqlite_sequence
WHERE name IN (
    'trading_run_events',
    'trading_orders',
    'trading_runs',
    'trading_bots'
);
