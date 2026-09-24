-- Development cleanup after introducing persistent Bots.
-- Preserve downloaded market data and dataset/catalog tables, but remove all
-- pre-Bot trading execution state so the new Bot -> Run model starts clean.

DELETE FROM trading_runs;
DELETE FROM trading_bots;

DELETE FROM sqlite_sequence
WHERE name IN (
    'trading_run_events',
    'trading_orders',
    'trading_runs',
    'trading_bots'
);
