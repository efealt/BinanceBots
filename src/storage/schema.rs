use rusqlite::Connection;

const INITIAL_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/001_initial.sql"
));
const DOWNLOAD_START_DATE_MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/migrations/002_download_start_date.sql"
));

pub(crate) fn migrate(connection: &Connection) -> Result<(), rusqlite::Error> {
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
        connection.execute_batch(INITIAL_MIGRATION)?;
        connection.execute(
            "INSERT INTO schema_migrations (version, applied_at_ms) VALUES (?1, ?2)",
            rusqlite::params![1_i64, current_time_ms()],
        )?;
    }

    if current_version < 2 {
        connection.execute_batch(DOWNLOAD_START_DATE_MIGRATION)?;
        connection.execute(
            "INSERT INTO schema_migrations (version, applied_at_ms) VALUES (?1, ?2)",
            rusqlite::params![2_i64, current_time_ms()],
        )?;
    }

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
    use super::migrate;
    use rusqlite::Connection;

    #[test]
    fn creates_backtest_and_live_storage() {
        let connection = Connection::open_in_memory().expect("open in-memory database");
        migrate(&connection).expect("apply initial migration");

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
                       'live_trades'
                   )",
                [],
                |row| row.get(0),
            )
            .expect("count storage tables");

        assert_eq!(table_count, 22);

        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("read schema version");
        assert_eq!(version, 2);
    }
}
