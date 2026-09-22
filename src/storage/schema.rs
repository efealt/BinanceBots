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
    use super::{apply_migration, migrate};
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
                       'trading_runs', 'trading_run_events', 'trading_run_status_events',
                       'trading_decisions', 'trading_order_intents', 'trading_orders',
                       'trading_order_state_events', 'trading_fills',
                       'trading_position_snapshots', 'trading_equity_snapshots'
                   )",
                [],
                |row| row.get(0),
            )
            .expect("count storage tables");

        assert_eq!(table_count, 33);

        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("read schema version");
        assert_eq!(version, 4);
    }

    #[test]
    fn rolls_back_failed_migration() {
        let mut connection = Connection::open_in_memory().expect("open in-memory database");
        migrate(&mut connection).expect("apply initial migrations");

        let result = apply_migration(
            &mut connection,
            5,
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
        assert_eq!(version, 4);
    }
}
