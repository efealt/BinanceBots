use rusqlite::Connection;

const INITIAL_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/001_initial.sql"
));
const DOWNLOAD_START_DATE_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/002_download_start_date.sql"
));
const AUTH_AUDIT_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/003_auth_audit.sql"
));
const TRADING_RUNS_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/004_trading_runs.sql"
));
const TRADING_BOTS_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/005_trading_bots.sql"
));
const CLEAN_PREBOT_TRADING_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/006_clean_prebot_trading.sql"
));
const RESET_DEVELOPMENT_TRADING_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/007_reset_development_trading_state.sql"
));

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             version INTEGER PRIMARY KEY,
             applied_at_ms INTEGER NOT NULL
         );",
    )?;

    let current_version: i64 = connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;

    if current_version < 1 {
        apply_migration(connection, 1, INITIAL_MIGRATION)?;
    }

    if current_version < 2 {
        apply_migration(connection, 2, DOWNLOAD_START_DATE_MIGRATION)?;
    }

    if current_version < 3 {
        apply_migration(connection, 3, AUTH_AUDIT_MIGRATION)?;
    }

    if current_version < 4 {
        apply_migration(connection, 4, TRADING_RUNS_MIGRATION)?;
    }

    if current_version < 5 {
        apply_migration(connection, 5, TRADING_BOTS_MIGRATION)?;
    }

    if current_version < 6 {
        apply_migration(connection, 6, CLEAN_PREBOT_TRADING_MIGRATION)?;
    }

    if current_version < 7 {
        apply_migration(connection, 7, RESET_DEVELOPMENT_TRADING_MIGRATION)?;
    }

    Ok(())
}

fn apply_migration(
    connection: &mut Connection,
    version: i64,
    migration: &str,
) -> Result<(), rusqlite::Error> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(migration)?;
    transaction.execute(
        "INSERT INTO schema_migrations (version, applied_at_ms) VALUES (?1, ?2)",
        rusqlite::params![version, current_time_ms()],
    )?;
    transaction.commit()?;
    Ok(())
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_migration, migrate, AUTH_AUDIT_MIGRATION, CLEAN_PREBOT_TRADING_MIGRATION,
        DOWNLOAD_START_DATE_MIGRATION, INITIAL_MIGRATION, RESET_DEVELOPMENT_TRADING_MIGRATION,
        TRADING_BOTS_MIGRATION, TRADING_RUNS_MIGRATION,
    };
    use rusqlite::Connection;

    #[test]
    fn creates_backtest_and_live_storage() {
        let mut connection = Connection::open_in_memory().expect("open in-memory database");
        migrate(&mut connection).expect("apply initial migration");

        let table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table'
                   AND name IN (
                       'market_instruments', 'instrument_trading_rules',
                       'futures_fee_schedules', 'futures_margin_brackets', 'data_downloads',
                       'historical_datasets', 'historical_imports', 'historical_ohlcv',
                       'historical_reference_klines', 'historical_trades',
                       'historical_aggregate_trades', 'historical_funding_rates',
                       'historical_book_ticker', 'historical_depth_snapshots',
                       'historical_depth_levels',
                       'live_capture_sessions', 'live_events', 'live_kline_updates',
                       'live_book_ticker', 'live_depth_snapshots', 'live_depth_levels',
                       'live_trades', 'auth_audit_events',
                       'trading_bots', 'trading_runs', 'trading_run_events', 'trading_run_status_events',
                       'trading_decisions', 'trading_order_intents', 'trading_orders',
                       'trading_order_state_events', 'trading_fills',
                       'trading_position_snapshots', 'trading_equity_snapshots'
                   )",
                [],
                |row| row.get(0),
            )
            .expect("count storage tables");

        assert_eq!(table_count, 34);

        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("read schema version");
        assert_eq!(version, 7);
    }

    #[test]
    fn cleanup_migration_removes_trading_state_but_preserves_downloaded_data() {
        let mut connection = Connection::open_in_memory().expect("open in-memory database");
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE schema_migrations (
                 version INTEGER PRIMARY KEY,
                 applied_at_ms INTEGER NOT NULL
             );",
        ).unwrap();

        apply_migration(&mut connection, 1, INITIAL_MIGRATION).unwrap();
        apply_migration(&mut connection, 2, DOWNLOAD_START_DATE_MIGRATION).unwrap();
        apply_migration(&mut connection, 3, AUTH_AUDIT_MIGRATION).unwrap();
        apply_migration(&mut connection, 4, TRADING_RUNS_MIGRATION).unwrap();
        apply_migration(&mut connection, 5, TRADING_BOTS_MIGRATION).unwrap();

        connection.execute(
            "INSERT INTO data_downloads
             (download_id, provider, symbol, market_type, name, interval, created_at_ms, status)
             VALUES (1, 'binance', 'BTCUSDT', 'spot', 'BTC test data', '1m', 1, 'complete')",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO market_instruments
             (instrument_id, venue, market_type, symbol, created_at_ms)
             VALUES (1, 'binance', 'spot', 'BTCUSDT', 1)",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO trading_bots
             (bot_id, bot_name, config_json, created_at_ms, updated_at_ms)
             VALUES (1, 'Old dev bot', '{}', 1, 1)",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO trading_runs
             (run_id, bot_id, mode, status, strategy_id, strategy_version,
              strategy_params_json, instrument_id, initial_capital_decimal,
              run_config_json, data_source_json, execution_assumptions_json,
              created_at_ms, updated_at_ms)
             VALUES (1, 1, 'paper', 'created', 'static-grid-fixture', '1',
                     '{}', 1, '1000', '{}', '{}', '{}', 1, 1)",
            [],
        ).unwrap();

        apply_migration(&mut connection, 6, CLEAN_PREBOT_TRADING_MIGRATION).unwrap();

        connection.execute(
            "INSERT INTO trading_bots
             (bot_id, bot_name, config_json, created_at_ms, updated_at_ms)
             VALUES (2, 'Disposable phase2 bot', '{}', 2, 2)",
            [],
        ).unwrap();
        connection.execute(
            "INSERT INTO trading_runs
             (run_id, bot_id, mode, status, strategy_id, strategy_version,
              strategy_params_json, instrument_id, initial_capital_decimal,
              run_config_json, data_source_json, execution_assumptions_json,
              created_at_ms, updated_at_ms)
             VALUES (2, 2, 'paper', 'created', 'static-grid-fixture', '1',
                     '{}', 1, '1000', '{}', '{}', '{}', 2, 2)",
            [],
        ).unwrap();

        apply_migration(&mut connection, 7, RESET_DEVELOPMENT_TRADING_MIGRATION).unwrap();

        let downloads: i64 = connection.query_row(
            "SELECT COUNT(*) FROM data_downloads WHERE download_id = 1",
            [],
            |row| row.get(0),
        ).unwrap();
        let runs: i64 = connection.query_row(
            "SELECT COUNT(*) FROM trading_runs",
            [],
            |row| row.get(0),
        ).unwrap();
        let bots: i64 = connection.query_row(
            "SELECT COUNT(*) FROM trading_bots",
            [],
            |row| row.get(0),
        ).unwrap();

        assert_eq!(downloads, 1);
        assert_eq!(runs, 0);
        assert_eq!(bots, 0);
    }

    #[test]
    fn rolls_back_failed_migration() {
        let mut connection = Connection::open_in_memory().expect("open in-memory database");
        migrate(&mut connection).expect("apply initial migrations");

        let result = apply_migration(
            &mut connection,
            8,
            "CREATE TABLE migration_probe (id INTEGER PRIMARY KEY);
             THIS IS NOT VALID SQL;",
        );
        assert!(result.is_err());

        let probe_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'migration_probe'",
                [],
                |row| row.get(0),
            )
            .expect("check rollback table");
        assert_eq!(probe_count, 0);

        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("read schema version after failed migration");
        assert_eq!(version, 7);
    }
}
