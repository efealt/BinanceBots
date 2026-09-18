CREATE TABLE IF NOT EXISTS market_instruments (
    instrument_id INTEGER PRIMARY KEY,
    venue TEXT NOT NULL CHECK (venue = 'binance'),
    market_type TEXT NOT NULL CHECK (market_type IN ('spot', 'usd_m_perpetual')),
    symbol TEXT NOT NULL,
    base_asset TEXT,
    quote_asset TEXT,
    margin_asset TEXT,
    contract_type TEXT CHECK (contract_type IN ('perpetual', 'delivery')),
    contract_size REAL,
    listed_at_ms INTEGER,
    delisted_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    CHECK (delisted_at_ms IS NULL OR listed_at_ms IS NULL OR delisted_at_ms >= listed_at_ms),
    UNIQUE (venue, market_type, symbol)
);

CREATE TABLE IF NOT EXISTS instrument_trading_rules (
    instrument_id INTEGER NOT NULL REFERENCES market_instruments(instrument_id) ON DELETE CASCADE,
    effective_from_ms INTEGER NOT NULL,
    effective_to_ms INTEGER,
    tick_size REAL NOT NULL CHECK (tick_size > 0),
    step_size REAL NOT NULL CHECK (step_size > 0),
    min_quantity REAL,
    max_quantity REAL,
    min_notional REAL,
    max_notional REAL,
    max_leverage INTEGER CHECK (max_leverage IS NULL OR max_leverage > 0),
    source TEXT NOT NULL DEFAULT 'binance_exchange_info',
    captured_at_ms INTEGER NOT NULL,
    PRIMARY KEY (instrument_id, effective_from_ms),
    CHECK (effective_to_ms IS NULL OR effective_to_ms >= effective_from_ms)
);

CREATE TABLE IF NOT EXISTS futures_fee_schedules (
    fee_schedule_id INTEGER PRIMARY KEY,
    market_type TEXT NOT NULL CHECK (market_type = 'usd_m_perpetual'),
    effective_from_ms INTEGER NOT NULL,
    effective_to_ms INTEGER,
    maker_fee_rate REAL NOT NULL,
    taker_fee_rate REAL NOT NULL,
    liquidation_fee_rate REAL,
    source TEXT NOT NULL,
    captured_at_ms INTEGER NOT NULL,
    CHECK (effective_to_ms IS NULL OR effective_to_ms >= effective_from_ms),
    UNIQUE (market_type, effective_from_ms, source)
);

CREATE TABLE IF NOT EXISTS futures_margin_brackets (
    margin_bracket_id INTEGER PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES market_instruments(instrument_id) ON DELETE CASCADE,
    bracket INTEGER NOT NULL CHECK (bracket > 0),
    notional_floor REAL NOT NULL CHECK (notional_floor >= 0),
    notional_cap REAL,
    initial_leverage INTEGER NOT NULL CHECK (initial_leverage > 0),
    maintenance_margin_ratio REAL NOT NULL CHECK (maintenance_margin_ratio >= 0),
    maintenance_amount REAL NOT NULL DEFAULT 0,
    effective_from_ms INTEGER NOT NULL,
    effective_to_ms INTEGER,
    source TEXT NOT NULL DEFAULT 'binance_leverage_bracket',
    captured_at_ms INTEGER NOT NULL,
    CHECK (notional_cap IS NULL OR notional_cap > notional_floor),
    CHECK (effective_to_ms IS NULL OR effective_to_ms >= effective_from_ms),
    UNIQUE (instrument_id, bracket, effective_from_ms)
);

CREATE TABLE IF NOT EXISTS data_downloads (
    download_id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL CHECK (provider = 'binance'),
    symbol TEXT NOT NULL,
    market_type TEXT NOT NULL CHECK (market_type IN ('spot', 'usd_m_perpetual')),
    dataset_kind TEXT NOT NULL DEFAULT 'traded_kline'
        CHECK (dataset_kind IN (
            'traded_kline',
            'mark_price_kline',
            'index_price_kline',
            'premium_index_kline',
            'funding_rate',
            'trade',
            'agg_trade',
            'book_ticker',
            'order_book_depth'
        )),
    name TEXT NOT NULL,
    interval TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    last_downloaded_at_ms INTEGER,
    data_start_time_ms INTEGER,
    data_end_time_ms INTEGER,
    status TEXT NOT NULL DEFAULT 'not_started'
        CHECK (status IN ('not_started', 'downloading', 'complete', 'partial', 'failed')),
    UNIQUE (provider, symbol, market_type, dataset_kind, interval)
);

CREATE INDEX IF NOT EXISTS idx_data_downloads_created
    ON data_downloads(created_at_ms DESC, download_id DESC);

CREATE TABLE IF NOT EXISTS historical_datasets (
    dataset_id INTEGER PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES market_instruments(instrument_id),
    dataset_kind TEXT NOT NULL CHECK (dataset_kind IN (
        'traded_kline',
        'mark_price_kline',
        'index_price_kline',
        'premium_index_kline',
        'funding_rate',
        'trade',
        'agg_trade',
        'book_ticker',
        'order_book_depth'
    )),
    interval TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'binance_public_data',
    start_time_ms INTEGER NOT NULL,
    end_time_ms INTEGER NOT NULL,
    downloaded_at_ms INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('complete', 'partial', 'failed')),
    CHECK (end_time_ms >= start_time_ms),
    UNIQUE (instrument_id, dataset_kind, interval, source)
);

CREATE INDEX IF NOT EXISTS idx_historical_datasets_catalog
    ON historical_datasets(instrument_id, dataset_kind, interval);

CREATE TABLE IF NOT EXISTS historical_imports (
    import_id INTEGER PRIMARY KEY,
    dataset_id INTEGER NOT NULL REFERENCES historical_datasets(dataset_id) ON DELETE CASCADE,
    archive_kind TEXT NOT NULL CHECK (archive_kind IN ('daily_zip', 'monthly_zip', 'rest_backfill')),
    source_url TEXT NOT NULL,
    checksum_sha256 TEXT,
    start_time_ms INTEGER NOT NULL,
    end_time_ms INTEGER NOT NULL,
    downloaded_at_ms INTEGER NOT NULL,
    imported_at_ms INTEGER,
    row_count INTEGER NOT NULL DEFAULT 0 CHECK (row_count >= 0),
    status TEXT NOT NULL CHECK (status IN ('downloaded', 'imported', 'failed')),
    failure_reason TEXT,
    CHECK (end_time_ms >= start_time_ms),
    UNIQUE (dataset_id, source_url)
);

CREATE INDEX IF NOT EXISTS idx_historical_imports_coverage
    ON historical_imports(dataset_id, start_time_ms, end_time_ms);

CREATE TABLE IF NOT EXISTS historical_ohlcv (
    dataset_id INTEGER NOT NULL REFERENCES historical_datasets(dataset_id) ON DELETE CASCADE,
    open_time_ms INTEGER NOT NULL,
    close_time_ms INTEGER NOT NULL,
    open_price REAL NOT NULL,
    high_price REAL NOT NULL,
    low_price REAL NOT NULL,
    close_price REAL NOT NULL,
    base_volume REAL NOT NULL,
    quote_volume REAL NOT NULL,
    trade_count INTEGER NOT NULL,
    taker_buy_base_volume REAL NOT NULL,
    taker_buy_quote_volume REAL NOT NULL,
    PRIMARY KEY (dataset_id, open_time_ms),
    CHECK (close_time_ms >= open_time_ms)
);

CREATE INDEX IF NOT EXISTS idx_historical_ohlcv_time
    ON historical_ohlcv(dataset_id, open_time_ms);

CREATE TABLE IF NOT EXISTS historical_reference_klines (
    dataset_id INTEGER NOT NULL REFERENCES historical_datasets(dataset_id) ON DELETE CASCADE,
    open_time_ms INTEGER NOT NULL,
    close_time_ms INTEGER NOT NULL,
    open_price REAL NOT NULL,
    high_price REAL NOT NULL,
    low_price REAL NOT NULL,
    close_price REAL NOT NULL,
    PRIMARY KEY (dataset_id, open_time_ms),
    CHECK (close_time_ms >= open_time_ms)
);

CREATE INDEX IF NOT EXISTS idx_historical_reference_klines_time
    ON historical_reference_klines(dataset_id, open_time_ms);

CREATE TABLE IF NOT EXISTS historical_trades (
    dataset_id INTEGER NOT NULL REFERENCES historical_datasets(dataset_id) ON DELETE CASCADE,
    trade_id INTEGER NOT NULL,
    price REAL NOT NULL,
    quantity REAL NOT NULL,
    quote_quantity REAL NOT NULL,
    trade_time_ms INTEGER NOT NULL,
    is_buyer_maker INTEGER NOT NULL CHECK (is_buyer_maker IN (0, 1)),
    PRIMARY KEY (dataset_id, trade_id)
);

CREATE INDEX IF NOT EXISTS idx_historical_trades_time
    ON historical_trades(dataset_id, trade_time_ms, trade_id);

CREATE TABLE IF NOT EXISTS historical_aggregate_trades (
    dataset_id INTEGER NOT NULL REFERENCES historical_datasets(dataset_id) ON DELETE CASCADE,
    aggregate_trade_id INTEGER NOT NULL,
    price REAL NOT NULL,
    quantity REAL NOT NULL,
    first_trade_id INTEGER NOT NULL,
    last_trade_id INTEGER NOT NULL,
    trade_time_ms INTEGER NOT NULL,
    is_buyer_maker INTEGER NOT NULL CHECK (is_buyer_maker IN (0, 1)),
    PRIMARY KEY (dataset_id, aggregate_trade_id),
    CHECK (last_trade_id >= first_trade_id)
);

CREATE INDEX IF NOT EXISTS idx_historical_aggregate_trades_time
    ON historical_aggregate_trades(dataset_id, trade_time_ms, aggregate_trade_id);

CREATE TABLE IF NOT EXISTS historical_funding_rates (
    dataset_id INTEGER NOT NULL REFERENCES historical_datasets(dataset_id) ON DELETE CASCADE,
    funding_time_ms INTEGER NOT NULL,
    funding_rate REAL NOT NULL,
    mark_price REAL,
    PRIMARY KEY (dataset_id, funding_time_ms)
);

CREATE INDEX IF NOT EXISTS idx_historical_funding_rates_time
    ON historical_funding_rates(dataset_id, funding_time_ms);

CREATE TABLE IF NOT EXISTS historical_book_ticker (
    quote_id INTEGER PRIMARY KEY,
    dataset_id INTEGER NOT NULL REFERENCES historical_datasets(dataset_id) ON DELETE CASCADE,
    event_time_ms INTEGER NOT NULL,
    transaction_time_ms INTEGER,
    update_id INTEGER,
    bid_price REAL NOT NULL,
    bid_quantity REAL NOT NULL,
    ask_price REAL NOT NULL,
    ask_quantity REAL NOT NULL,
    UNIQUE (dataset_id, event_time_ms, update_id)
);

CREATE INDEX IF NOT EXISTS idx_historical_book_ticker_time
    ON historical_book_ticker(dataset_id, event_time_ms, quote_id);

CREATE TABLE IF NOT EXISTS historical_depth_snapshots (
    snapshot_id INTEGER PRIMARY KEY,
    dataset_id INTEGER NOT NULL REFERENCES historical_datasets(dataset_id) ON DELETE CASCADE,
    depth_kind TEXT NOT NULL CHECK (depth_kind IN ('l2', 'percent_bucket')),
    event_time_ms INTEGER NOT NULL,
    transaction_time_ms INTEGER,
    update_id INTEGER
);

CREATE INDEX IF NOT EXISTS idx_historical_depth_snapshots_time
    ON historical_depth_snapshots(dataset_id, event_time_ms, snapshot_id);

CREATE TABLE IF NOT EXISTS historical_depth_levels (
    snapshot_id INTEGER NOT NULL REFERENCES historical_depth_snapshots(snapshot_id) ON DELETE CASCADE,
    side TEXT NOT NULL CHECK (side IN ('bid', 'ask')),
    level_index INTEGER NOT NULL CHECK (level_index >= 0),
    price REAL,
    quantity REAL,
    notional REAL,
    distance_basis_points REAL,
    PRIMARY KEY (snapshot_id, side, level_index)
);

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
WHERE d.dataset_kind = 'traded_kline'
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
