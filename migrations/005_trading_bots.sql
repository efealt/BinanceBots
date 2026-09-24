CREATE TABLE trading_bots (
    bot_id INTEGER PRIMARY KEY AUTOINCREMENT,
    bot_name TEXT NOT NULL CHECK (length(trim(bot_name)) > 0 AND length(bot_name) <= 120),
    config_json TEXT NOT NULL CHECK (json_valid(config_json) AND json_type(config_json) = 'object'),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_trading_bots_updated
ON trading_bots(updated_at_ms DESC, bot_id DESC);

ALTER TABLE trading_runs
ADD COLUMN bot_id INTEGER REFERENCES trading_bots(bot_id);

CREATE INDEX idx_trading_runs_bot
ON trading_runs(bot_id, run_id);
