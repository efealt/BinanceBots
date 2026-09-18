CREATE TABLE IF NOT EXISTS data_downloads (
    download_id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL CHECK (provider = 'binance'),
    symbol TEXT NOT NULL,
    market_type TEXT NOT NULL CHECK (market_type IN ('spot', 'usd_m_perpetual')),
    name TEXT NOT NULL,
    interval TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    last_downloaded_at_ms INTEGER,
    data_start_time_ms INTEGER,
    data_end_time_ms INTEGER,
    status TEXT NOT NULL DEFAULT 'not_started'
        CHECK (status IN ('not_started', 'downloading', 'complete', 'partial', 'failed')),
    UNIQUE (provider, symbol, market_type, interval)
);

CREATE INDEX IF NOT EXISTS idx_data_downloads_created
    ON data_downloads(created_at_ms DESC, download_id DESC);
