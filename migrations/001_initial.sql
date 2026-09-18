CREATE TABLE IF NOT EXISTS market_instruments (
    instrument_id INTEGER PRIMARY KEY,
    venue TEXT NOT NULL CHECK (venue = 'binance'),
    market_type TEXT NOT NULL CHECK (market_type IN ('spot', 'usd_m_perpetual')),
    symbol TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    UNIQUE (venue, market_type, symbol)
);

CREATE TABLE IF NOT EXISTS historical_datasets (
    dataset_id INTEGER PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES market_instruments(instrument_id),
    interval TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'binance_rest',
    start_time_ms INTEGER NOT NULL,
    end_time_ms INTEGER NOT NULL,
    downloaded_at_ms INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('complete', 'partial', 'failed')),
    CHECK (end_time_ms >= start_time_ms),
    UNIQUE (instrument_id, interval, source, start_time_ms, end_time_ms)
);

CREATE TABLE IF NOT EXISTS historical_ohlcv (
    dataset_id INTEGER NOT NULL REFERENCES historical_datasets(dataset_id) ON DELETE CASCADE,
    open_time_ms INTEGER NOT NULL,
    close_time_ms INTEGER NOT NULL,
    open_price REAL NOT NULL,
    high_price REAL NOT NULL,
    low_price REAL NOT NULL,
    close_price REAL NOT NULL,
    base_volume REAL NOT NULL,
    quote_volume REAL,
    trade_count INTEGER,
    taker_buy_base_volume REAL,
    taker_buy_quote_volume REAL,
    PRIMARY KEY (dataset_id, open_time_ms)
);

CREATE INDEX IF NOT EXISTS idx_historical_ohlcv_time
    ON historical_ohlcv(dataset_id, open_time_ms);

CREATE TABLE IF NOT EXISTS live_capture_sessions (
    capture_id INTEGER PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES market_instruments(instrument_id),
    interval TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'binance_websocket',
    started_at_ms INTEGER NOT NULL,
    ended_at_ms INTEGER,
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed')),
    stop_reason TEXT,
    CHECK (ended_at_ms IS NULL OR ended_at_ms >= started_at_ms)
);

CREATE INDEX IF NOT EXISTS idx_live_capture_instrument
    ON live_capture_sessions(instrument_id, interval, started_at_ms);

CREATE TABLE IF NOT EXISTS live_events (
    event_id INTEGER PRIMARY KEY,
    capture_id INTEGER NOT NULL REFERENCES live_capture_sessions(capture_id) ON DELETE CASCADE,
    event_type TEXT NOT NULL CHECK (event_type IN ('kline', 'book_ticker', 'depth', 'trade')),
    stream_name TEXT NOT NULL,
    exchange_event_time_ms INTEGER,
    received_at_ms INTEGER NOT NULL,
    raw_payload TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_live_events_capture_time
    ON live_events(capture_id, received_at_ms, event_id);

CREATE TABLE IF NOT EXISTS live_kline_updates (
    event_id INTEGER PRIMARY KEY REFERENCES live_events(event_id) ON DELETE CASCADE,
    capture_id INTEGER NOT NULL REFERENCES live_capture_sessions(capture_id) ON DELETE CASCADE,
    open_time_ms INTEGER NOT NULL,
    close_time_ms INTEGER NOT NULL,
    open_price REAL NOT NULL,
    high_price REAL NOT NULL,
    low_price REAL NOT NULL,
    close_price REAL NOT NULL,
    base_volume REAL NOT NULL,
    is_closed INTEGER NOT NULL CHECK (is_closed IN (0, 1)),
    received_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_live_kline_capture_time
    ON live_kline_updates(capture_id, open_time_ms, received_at_ms, event_id);

CREATE TABLE IF NOT EXISTS live_book_ticker (
    event_id INTEGER PRIMARY KEY REFERENCES live_events(event_id) ON DELETE CASCADE,
    capture_id INTEGER NOT NULL REFERENCES live_capture_sessions(capture_id) ON DELETE CASCADE,
    update_id INTEGER NOT NULL,
    bid_price REAL NOT NULL,
    bid_quantity REAL NOT NULL,
    ask_price REAL NOT NULL,
    ask_quantity REAL NOT NULL,
    received_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_live_book_ticker_capture_time
    ON live_book_ticker(capture_id, received_at_ms, event_id);

CREATE TABLE IF NOT EXISTS live_depth_snapshots (
    event_id INTEGER PRIMARY KEY REFERENCES live_events(event_id) ON DELETE CASCADE,
    capture_id INTEGER NOT NULL REFERENCES live_capture_sessions(capture_id) ON DELETE CASCADE,
    update_id INTEGER,
    received_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS live_depth_levels (
    event_id INTEGER NOT NULL REFERENCES live_depth_snapshots(event_id) ON DELETE CASCADE,
    side TEXT NOT NULL CHECK (side IN ('bid', 'ask')),
    level_index INTEGER NOT NULL CHECK (level_index >= 0),
    price REAL NOT NULL,
    quantity REAL NOT NULL,
    PRIMARY KEY (event_id, side, level_index)
);

CREATE INDEX IF NOT EXISTS idx_live_depth_capture
    ON live_depth_snapshots(capture_id, received_at_ms, event_id);

CREATE TABLE IF NOT EXISTS live_trades (
    event_id INTEGER PRIMARY KEY REFERENCES live_events(event_id) ON DELETE CASCADE,
    capture_id INTEGER NOT NULL REFERENCES live_capture_sessions(capture_id) ON DELETE CASCADE,
    trade_id INTEGER NOT NULL,
    trade_kind TEXT NOT NULL CHECK (trade_kind IN ('trade', 'agg_trade')),
    price REAL NOT NULL,
    quantity REAL NOT NULL,
    trade_time_ms INTEGER NOT NULL,
    is_buyer_maker INTEGER NOT NULL CHECK (is_buyer_maker IN (0, 1)),
    received_at_ms INTEGER NOT NULL,
    UNIQUE (capture_id, trade_id, trade_kind)
);

CREATE INDEX IF NOT EXISTS idx_live_trades_capture_time
    ON live_trades(capture_id, trade_time_ms, event_id);

CREATE VIEW IF NOT EXISTS market_candles AS
WITH latest_live AS (
    SELECT
        k.*,
        s.instrument_id,
        s.interval,
        ROW_NUMBER() OVER (
            PARTITION BY s.instrument_id, s.interval, k.open_time_ms
            ORDER BY k.received_at_ms DESC, k.event_id DESC
        ) AS row_number
    FROM live_kline_updates AS k
    JOIN live_capture_sessions AS s ON s.capture_id = k.capture_id
)
SELECT
    i.instrument_id,
    i.venue,
    i.market_type,
    i.symbol,
    d.interval,
    h.open_time_ms,
    h.close_time_ms,
    h.open_price,
    h.high_price,
    h.low_price,
    h.close_price,
    h.base_volume,
    1 AS is_closed,
    'historical' AS source,
    d.dataset_id AS source_id
FROM historical_ohlcv AS h
JOIN historical_datasets AS d ON d.dataset_id = h.dataset_id
JOIN market_instruments AS i ON i.instrument_id = d.instrument_id
UNION ALL
SELECT
    i.instrument_id,
    i.venue,
    i.market_type,
    i.symbol,
    l.interval,
    l.open_time_ms,
    l.close_time_ms,
    l.open_price,
    l.high_price,
    l.low_price,
    l.close_price,
    l.base_volume,
    l.is_closed,
    'live_websocket' AS source,
    l.capture_id AS source_id
FROM latest_live AS l
JOIN market_instruments AS i ON i.instrument_id = l.instrument_id
WHERE l.row_number = 1;
