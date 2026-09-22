CREATE TABLE trading_runs (
    run_id INTEGER PRIMARY KEY AUTOINCREMENT,
    comparison_id TEXT,
    mode TEXT NOT NULL CHECK (mode IN ('backtest', 'paper', 'live')),
    status TEXT NOT NULL CHECK (status IN ('created', 'running', 'completed', 'failed', 'stopped')),
    strategy_id TEXT NOT NULL CHECK (length(strategy_id) > 0),
    strategy_version TEXT NOT NULL CHECK (length(strategy_version) > 0),
    strategy_params_json TEXT NOT NULL CHECK (json_valid(strategy_params_json)),
    instrument_id INTEGER NOT NULL REFERENCES market_instruments(instrument_id),
    initial_capital_decimal TEXT NOT NULL CHECK (length(initial_capital_decimal) > 0),
    run_config_json TEXT NOT NULL CHECK (json_valid(run_config_json)),
    data_source_json TEXT NOT NULL CHECK (json_valid(data_source_json)),
    execution_assumptions_json TEXT NOT NULL CHECK (json_valid(execution_assumptions_json)),
    started_at_ms INTEGER,
    ended_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    next_event_sequence INTEGER NOT NULL DEFAULT 0 CHECK (next_event_sequence >= 0),
    CHECK (comparison_id IS NULL OR length(trim(comparison_id)) > 0),
    CHECK (ended_at_ms IS NULL OR started_at_ms IS NULL OR ended_at_ms >= started_at_ms)
);

CREATE INDEX idx_trading_runs_comparison
ON trading_runs(comparison_id, run_id);

CREATE INDEX idx_trading_runs_instrument_time
ON trading_runs(instrument_id, created_at_ms, run_id);

CREATE TABLE trading_run_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL REFERENCES trading_runs(run_id) ON DELETE CASCADE,
    run_sequence INTEGER NOT NULL CHECK (run_sequence > 0),
    event_kind TEXT NOT NULL CHECK (event_kind IN (
        'run_status', 'decision', 'order_intent', 'order_state', 'fill', 'position', 'equity'
    )),
    event_time_ms INTEGER NOT NULL,
    exchange_time_ms INTEGER,
    received_at_ms INTEGER,
    persisted_at_ms INTEGER NOT NULL,
    UNIQUE (run_id, run_sequence),
    UNIQUE (event_id, run_id)
);

CREATE INDEX idx_trading_run_events_time
ON trading_run_events(run_id, event_time_ms, run_sequence);

CREATE TABLE trading_run_status_events (
    event_id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('created', 'running', 'completed', 'failed', 'stopped')),
    note TEXT,
    FOREIGN KEY (event_id, run_id)
        REFERENCES trading_run_events(event_id, run_id) ON DELETE CASCADE
);

CREATE INDEX idx_trading_run_status_events_run
ON trading_run_status_events(run_id, event_id);

CREATE TABLE trading_decisions (
    event_id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL,
    decision_type TEXT NOT NULL CHECK (length(decision_type) > 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    FOREIGN KEY (event_id, run_id)
        REFERENCES trading_run_events(event_id, run_id) ON DELETE CASCADE
);

CREATE INDEX idx_trading_decisions_run
ON trading_decisions(run_id, event_id);

CREATE TABLE trading_order_intents (
    event_id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL,
    intent_key TEXT,
    side TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    order_type TEXT NOT NULL CHECK (order_type IN (
        'market', 'limit', 'stop_market', 'stop_limit'
    )),
    time_in_force TEXT CHECK (time_in_force IS NULL OR time_in_force IN ('gtc', 'ioc', 'fok', 'gtx')),
    price_decimal TEXT,
    quantity_decimal TEXT NOT NULL CHECK (length(quantity_decimal) > 0),
    stop_price_decimal TEXT,
    reduce_only INTEGER NOT NULL DEFAULT 0 CHECK (reduce_only IN (0, 1)),
    metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json)),
    CHECK (intent_key IS NULL OR length(intent_key) > 0),
    CHECK (price_decimal IS NULL OR length(price_decimal) > 0),
    CHECK (stop_price_decimal IS NULL OR length(stop_price_decimal) > 0),
    UNIQUE (event_id, run_id),
    FOREIGN KEY (event_id, run_id)
        REFERENCES trading_run_events(event_id, run_id) ON DELETE CASCADE
);

CREATE INDEX idx_trading_order_intents_run
ON trading_order_intents(run_id, event_id);

CREATE TABLE trading_orders (
    order_id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL REFERENCES trading_runs(run_id) ON DELETE CASCADE,
    created_event_id INTEGER NOT NULL,
    intent_event_id INTEGER,
    client_order_id TEXT,
    exchange_order_id TEXT,
    side TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    order_type TEXT NOT NULL CHECK (order_type IN (
        'market', 'limit', 'stop_market', 'stop_limit'
    )),
    time_in_force TEXT CHECK (time_in_force IS NULL OR time_in_force IN ('gtc', 'ioc', 'fok', 'gtx')),
    price_decimal TEXT,
    quantity_decimal TEXT NOT NULL CHECK (length(quantity_decimal) > 0),
    stop_price_decimal TEXT,
    metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json)),
    CHECK (client_order_id IS NULL OR length(client_order_id) > 0),
    CHECK (exchange_order_id IS NULL OR length(exchange_order_id) > 0),
    CHECK (price_decimal IS NULL OR length(price_decimal) > 0),
    CHECK (stop_price_decimal IS NULL OR length(stop_price_decimal) > 0),
    UNIQUE (created_event_id),
    UNIQUE (run_id, client_order_id),
    UNIQUE (run_id, exchange_order_id),
    UNIQUE (order_id, run_id),
    FOREIGN KEY (created_event_id, run_id)
        REFERENCES trading_run_events(event_id, run_id) ON DELETE CASCADE,
    FOREIGN KEY (intent_event_id, run_id)
        REFERENCES trading_order_intents(event_id, run_id)
);

CREATE INDEX idx_trading_orders_run
ON trading_orders(run_id, order_id);

CREATE TABLE trading_order_state_events (
    event_id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL,
    order_id INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN (
        'created', 'submitted', 'accepted', 'partially_filled',
        'filled', 'cancelled', 'rejected', 'expired'
    )),
    filled_quantity_decimal TEXT NOT NULL CHECK (length(filled_quantity_decimal) > 0),
    average_fill_price_decimal TEXT,
    reject_reason TEXT,
    metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json)),
    CHECK (average_fill_price_decimal IS NULL OR length(average_fill_price_decimal) > 0),
    FOREIGN KEY (event_id, run_id)
        REFERENCES trading_run_events(event_id, run_id) ON DELETE CASCADE,
    FOREIGN KEY (order_id, run_id)
        REFERENCES trading_orders(order_id, run_id) ON DELETE CASCADE
);

CREATE INDEX idx_trading_order_state_events_order
ON trading_order_state_events(run_id, order_id, event_id);

CREATE TABLE trading_fills (
    event_id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL,
    order_id INTEGER NOT NULL,
    exchange_trade_id TEXT,
    price_decimal TEXT NOT NULL CHECK (length(price_decimal) > 0),
    quantity_decimal TEXT NOT NULL CHECK (length(quantity_decimal) > 0),
    fee_decimal TEXT,
    fee_asset TEXT,
    liquidity_role TEXT CHECK (liquidity_role IS NULL OR liquidity_role IN ('maker', 'taker')),
    metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json)),
    CHECK (exchange_trade_id IS NULL OR length(exchange_trade_id) > 0),
    CHECK (fee_decimal IS NULL OR length(fee_decimal) > 0),
    CHECK (fee_asset IS NULL OR length(fee_asset) > 0),
    FOREIGN KEY (event_id, run_id)
        REFERENCES trading_run_events(event_id, run_id) ON DELETE CASCADE,
    FOREIGN KEY (order_id, run_id)
        REFERENCES trading_orders(order_id, run_id) ON DELETE CASCADE
);

CREATE INDEX idx_trading_fills_order
ON trading_fills(run_id, order_id, event_id);

CREATE TABLE trading_position_snapshots (
    event_id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL,
    position_quantity_decimal TEXT NOT NULL CHECK (length(position_quantity_decimal) > 0),
    average_entry_price_decimal TEXT,
    mark_price_decimal TEXT,
    realized_pnl_decimal TEXT,
    unrealized_pnl_decimal TEXT,
    cash_balance_decimal TEXT,
    metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json)),
    CHECK (average_entry_price_decimal IS NULL OR length(average_entry_price_decimal) > 0),
    CHECK (mark_price_decimal IS NULL OR length(mark_price_decimal) > 0),
    CHECK (realized_pnl_decimal IS NULL OR length(realized_pnl_decimal) > 0),
    CHECK (unrealized_pnl_decimal IS NULL OR length(unrealized_pnl_decimal) > 0),
    CHECK (cash_balance_decimal IS NULL OR length(cash_balance_decimal) > 0),
    FOREIGN KEY (event_id, run_id)
        REFERENCES trading_run_events(event_id, run_id) ON DELETE CASCADE
);

CREATE INDEX idx_trading_position_snapshots_run
ON trading_position_snapshots(run_id, event_id);

CREATE TABLE trading_equity_snapshots (
    event_id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL,
    equity_decimal TEXT NOT NULL CHECK (length(equity_decimal) > 0),
    cash_balance_decimal TEXT,
    realized_pnl_decimal TEXT,
    unrealized_pnl_decimal TEXT,
    fees_paid_decimal TEXT,
    metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json)),
    CHECK (cash_balance_decimal IS NULL OR length(cash_balance_decimal) > 0),
    CHECK (realized_pnl_decimal IS NULL OR length(realized_pnl_decimal) > 0),
    CHECK (unrealized_pnl_decimal IS NULL OR length(unrealized_pnl_decimal) > 0),
    CHECK (fees_paid_decimal IS NULL OR length(fees_paid_decimal) > 0),
    FOREIGN KEY (event_id, run_id)
        REFERENCES trading_run_events(event_id, run_id) ON DELETE CASCADE
);

CREATE INDEX idx_trading_equity_snapshots_run
ON trading_equity_snapshots(run_id, event_id);
