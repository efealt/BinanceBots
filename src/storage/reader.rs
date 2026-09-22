use super::StorageError;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use std::{collections::HashSet, path::PathBuf};

#[derive(Clone)]
pub struct StorageReader {
    database_path: PathBuf,
}

impl StorageReader {
    pub fn new(database_path: PathBuf) -> Self {
        Self { database_path }
    }

    pub fn initialize(&self) -> Result<(), StorageError> {
        if let Some(parent) = self.database_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let mut connection = Connection::open(&self.database_path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )?;
        super::schema::migrate(&mut connection)?;
        Ok(())
    }

    pub fn data_downloads(&self) -> Result<Vec<DataDownload>, StorageError> {
        if !self.database_path.exists() {
            return Ok(Vec::new());
        }

        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT
                download_id,
                provider,
                symbol,
                market_type,
                name,
                interval,
                requested_start_time_ms,
                created_at_ms,
                last_downloaded_at_ms,
                data_start_time_ms,
                data_end_time_ms,
                status
             FROM data_downloads
             ORDER BY created_at_ms DESC, download_id DESC",
        )?;
        let rows = statement.query_map([], map_data_download)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn create_data_download(
        &self,
        spec: &DataDownloadSpec,
    ) -> Result<DataDownload, StorageError> {
        self.initialize()?;
        let connection = Connection::open(&self.database_path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;

        let existing: Option<i64> = connection
            .query_row(
                "SELECT download_id
                 FROM data_downloads
                 WHERE provider = ?1 AND symbol = ?2 AND market_type = ?3 AND interval = ?4",
                params![spec.provider, spec.symbol, spec.market_type, spec.interval],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            return Err(StorageError::DataDownloadAlreadyExists {
                symbol: spec.symbol.clone(),
                market_type: spec.market_type.clone(),
                interval: spec.interval.clone(),
            });
        }

        let created_at_ms = super::now_ms();
        connection.execute(
            "INSERT INTO data_downloads
                (provider, symbol, market_type, name, interval, requested_start_time_ms, created_at_ms, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'not_started')",
            params![
                spec.provider,
                spec.symbol,
                spec.market_type,
                spec.name,
                spec.interval,
                spec.requested_start_time_ms,
                created_at_ms,
            ],
        )?;

        let download_id = connection.last_insert_rowid();
        Ok(DataDownload {
            download_id,
            provider: spec.provider.clone(),
            symbol: spec.symbol.clone(),
            market_type: spec.market_type.clone(),
            name: spec.name.clone(),
            interval: spec.interval.clone(),
            requested_start_time_ms: Some(spec.requested_start_time_ms),
            created_at_ms,
            last_downloaded_at_ms: None,
            data_start_time_ms: None,
            data_end_time_ms: None,
            status: "not_started".into(),
        })
    }

    pub fn update_data_download_start_date(
        &self,
        download_id: i64,
        requested_start_time_ms: i64,
    ) -> Result<DataDownload, StorageError> {
        self.initialize()?;
        let connection = self.open_write()?;
        let status: Option<String> = connection
            .query_row(
                "SELECT status FROM data_downloads WHERE download_id = ?1",
                params![download_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(status) = status else {
            return Err(StorageError::DataDownloadNotFound(download_id));
        };
        if status == "downloading" {
            return Err(StorageError::DataDownloadAlreadyRunning(download_id));
        }

        connection.execute(
            "UPDATE data_downloads
             SET requested_start_time_ms = ?1
             WHERE download_id = ?2",
            params![requested_start_time_ms, download_id],
        )?;

        connection
            .query_row(
                "SELECT
                    download_id,
                    provider,
                    symbol,
                    market_type,
                    name,
                    interval,
                    requested_start_time_ms,
                    created_at_ms,
                    last_downloaded_at_ms,
                    data_start_time_ms,
                    data_end_time_ms,
                    status
                 FROM data_downloads
                 WHERE download_id = ?1",
                params![download_id],
                map_data_download,
            )
            .map_err(StorageError::from)
    }

    pub fn prepare_archive_download(
        &self,
        download_id: i64,
        requested_start_time_ms: Option<i64>,
    ) -> Result<DownloadRunPreparation, StorageError> {
        self.initialize()?;
        let connection = self.open_write()?;
        let definition = connection
            .query_row(
                "SELECT provider, symbol, market_type, interval, requested_start_time_ms
                 FROM data_downloads WHERE download_id = ?1",
                params![download_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(StorageError::DataDownloadNotFound(download_id))?;

        let saved_start = match (definition.4, requested_start_time_ms) {
            (Some(saved), Some(requested)) if saved != requested => {
                return Err(StorageError::DataDownloadStartDateImmutable);
            }
            (Some(saved), _) => saved,
            (None, Some(requested)) => {
                connection.execute(
                    "UPDATE data_downloads
                     SET requested_start_time_ms = ?1
                     WHERE download_id = ?2",
                    params![requested, download_id],
                )?;
                requested
            }
            (None, None) => return Err(StorageError::DataDownloadStartDateRequired),
        };

        connection.execute(
            "UPDATE data_downloads SET status = 'downloading' WHERE download_id = ?1",
            params![download_id],
        )?;

        let mut statement = connection.prepare(
            "SELECT hi.source_url
             FROM historical_imports hi
             JOIN historical_datasets ds ON ds.dataset_id = hi.dataset_id
             JOIN market_instruments i ON i.instrument_id = ds.instrument_id
             WHERE i.venue = 'binance'
               AND i.symbol = ?1
               AND i.market_type = ?2
               AND ds.dataset_kind = 'traded_kline'
               AND ds.interval = ?3
               AND ds.source = 'binance_public_data'
               AND hi.status = 'imported'",
        )?;
        let imported_urls = statement
            .query_map(params![definition.1, definition.2, definition.3], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<HashSet<_>, _>>()?;

        Ok(DownloadRunPreparation {
            download_id,
            provider: definition.0,
            symbol: definition.1,
            market_type: definition.2,
            interval: definition.3,
            requested_start_time_ms: saved_start,
            imported_urls,
        })
    }

    pub fn import_historical_klines(
        &self,
        preparation: &DownloadRunPreparation,
        source_url: &str,
        archive_kind: &str,
        checksum_sha256: &str,
        rows: &[HistoricalKline],
    ) -> Result<(), StorageError> {
        let first = rows
            .first()
            .expect("archive rows are validated before storage");
        let last = rows
            .last()
            .expect("archive rows are validated before storage");
        let now_ms = super::now_ms();
        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let instrument_id = ensure_instrument(
            &transaction,
            &preparation.symbol,
            &preparation.market_type,
            now_ms,
        )?;
        let dataset_id = ensure_kline_dataset(
            &transaction,
            instrument_id,
            &preparation.interval,
            first.open_time_ms,
            last.open_time_ms,
            now_ms,
        )?;

        {
            let mut statement = transaction.prepare(
                "INSERT INTO historical_ohlcv (
                    dataset_id, open_time_ms, close_time_ms, open_price, high_price, low_price,
                    close_price, base_volume, quote_volume, trade_count,
                    taker_buy_base_volume, taker_buy_quote_volume
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(dataset_id, open_time_ms) DO UPDATE SET
                    close_time_ms = excluded.close_time_ms,
                    open_price = excluded.open_price,
                    high_price = excluded.high_price,
                    low_price = excluded.low_price,
                    close_price = excluded.close_price,
                    base_volume = excluded.base_volume,
                    quote_volume = excluded.quote_volume,
                    trade_count = excluded.trade_count,
                    taker_buy_base_volume = excluded.taker_buy_base_volume,
                    taker_buy_quote_volume = excluded.taker_buy_quote_volume",
            )?;
            for row in rows {
                statement.execute(params![
                    dataset_id,
                    row.open_time_ms,
                    row.close_time_ms,
                    row.open_price,
                    row.high_price,
                    row.low_price,
                    row.close_price,
                    row.base_volume,
                    row.quote_volume,
                    row.trade_count,
                    row.taker_buy_base_volume,
                    row.taker_buy_quote_volume,
                ])?;
            }
        }

        transaction.execute(
            "INSERT INTO historical_imports (
                dataset_id, archive_kind, source_url, checksum_sha256,
                start_time_ms, end_time_ms, downloaded_at_ms, imported_at_ms, row_count, status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, 'imported')
             ON CONFLICT(dataset_id, source_url) DO UPDATE SET
                checksum_sha256 = excluded.checksum_sha256,
                start_time_ms = excluded.start_time_ms,
                end_time_ms = excluded.end_time_ms,
                downloaded_at_ms = excluded.downloaded_at_ms,
                imported_at_ms = excluded.imported_at_ms,
                row_count = excluded.row_count,
                status = 'imported',
                failure_reason = NULL",
            params![
                dataset_id,
                archive_kind,
                source_url,
                checksum_sha256,
                first.open_time_ms,
                last.open_time_ms,
                now_ms,
                i64::try_from(rows.len()).unwrap_or(i64::MAX),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn record_archive_failure(
        &self,
        preparation: &DownloadRunPreparation,
        source_url: &str,
        archive_kind: &str,
        start_time_ms: i64,
        end_time_ms: i64,
        reason: &str,
    ) -> Result<(), StorageError> {
        let now_ms = super::now_ms();
        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let instrument_id = ensure_instrument(
            &transaction,
            &preparation.symbol,
            &preparation.market_type,
            now_ms,
        )?;
        let dataset_id = ensure_kline_dataset(
            &transaction,
            instrument_id,
            &preparation.interval,
            start_time_ms,
            end_time_ms,
            now_ms,
        )?;
        transaction.execute(
            "INSERT INTO historical_imports (
                dataset_id, archive_kind, source_url, start_time_ms, end_time_ms,
                downloaded_at_ms, row_count, status, failure_reason
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 'failed', ?7)
             ON CONFLICT(dataset_id, source_url) DO UPDATE SET
                downloaded_at_ms = excluded.downloaded_at_ms,
                status = 'failed',
                failure_reason = excluded.failure_reason",
            params![
                dataset_id,
                archive_kind,
                source_url,
                start_time_ms,
                end_time_ms,
                now_ms,
                reason,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn finish_archive_download(
        &self,
        preparation: &DownloadRunPreparation,
        status: &str,
    ) -> Result<(), StorageError> {
        let connection = self.open_write()?;
        let (data_start_time_ms, data_end_time_ms): (Option<i64>, Option<i64>) = connection
            .query_row(
                "SELECT MIN(h.open_time_ms), MAX(h.open_time_ms)
                 FROM historical_ohlcv h
                 JOIN historical_datasets ds ON ds.dataset_id = h.dataset_id
                 JOIN market_instruments i ON i.instrument_id = ds.instrument_id
                 WHERE i.venue = 'binance'
                   AND i.symbol = ?1
                   AND i.market_type = ?2
                   AND ds.dataset_kind = 'traded_kline'
                   AND ds.interval = ?3
                   AND ds.source = 'binance_public_data'",
                params![
                    preparation.symbol,
                    preparation.market_type,
                    preparation.interval,
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
        connection.execute(
            "UPDATE data_downloads
             SET last_downloaded_at_ms = ?1,
                 data_start_time_ms = ?2,
                 data_end_time_ms = ?3,
                 status = ?4
             WHERE download_id = ?5",
            params![
                super::now_ms(),
                data_start_time_ms,
                data_end_time_ms,
                status,
                preparation.download_id,
            ],
        )?;
        Ok(())
    }

    pub fn catalog(&self) -> Result<Vec<DatasetSummary>, StorageError> {
        if !self.database_path.exists() {
            return Ok(Vec::new());
        }

        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT
                d.dataset_id,
                i.venue,
                i.symbol,
                i.market_type,
                d.interval,
                d.source,
                d.start_time_ms,
                d.end_time_ms,
                d.downloaded_at_ms,
                d.status,
                (SELECT COUNT(*) FROM historical_ohlcv h WHERE h.dataset_id = d.dataset_id)
             FROM historical_datasets d
             JOIN market_instruments i ON i.instrument_id = d.instrument_id
             WHERE d.dataset_kind = 'traded_kline'
             ORDER BY d.downloaded_at_ms DESC, d.dataset_id DESC",
        )?;

        let rows = statement.query_map([], map_dataset_summary)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn record_auth_event(
        &self,
        event_type: &str,
        source_ip: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<(), StorageError> {
        self.initialize()?;
        let connection = self.open_write()?;
        connection.execute(
            "INSERT INTO auth_audit_events
                (event_type, occurred_at_ms, source_ip, user_agent)
             VALUES (?1, ?2, ?3, ?4)",
            params![event_type, super::now_ms(), source_ip, user_agent],
        )?;
        Ok(())
    }

    pub fn auth_audit_events(&self, limit: usize) -> Result<Vec<AuthAuditEvent>, StorageError> {
        if !self.database_path.exists() {
            return Ok(Vec::new());
        }
        let connection = self.open()?;
        let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
        let mut statement = connection.prepare(
            "SELECT event_id, event_type, occurred_at_ms, source_ip, user_agent
             FROM auth_audit_events
             ORDER BY occurred_at_ms DESC, event_id DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit], |row| {
            Ok(AuthAuditEvent {
                event_id: row.get(0)?,
                event_type: row.get(1)?,
                occurred_at_ms: row.get(2)?,
                source_ip: row.get(3)?,
                user_agent: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn ohlcv_series(&self, dataset_id: i64) -> Result<Vec<OhlcvCandle>, StorageError> {
        let connection = self.open()?;
        let exists: Option<i64> = connection
            .query_row(
                "SELECT dataset_id FROM historical_datasets
                 WHERE dataset_id = ?1 AND dataset_kind = 'traded_kline'",
                params![dataset_id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(StorageError::DatasetNotFound(dataset_id));
        }
        let mut statement = connection.prepare(
            "SELECT
                open_time_ms, close_time_ms, open_price, high_price, low_price,
                close_price, base_volume, quote_volume, trade_count,
                taker_buy_base_volume, taker_buy_quote_volume
             FROM historical_ohlcv
             WHERE dataset_id = ?1
             ORDER BY open_time_ms",
        )?;
        let rows = statement.query_map(params![dataset_id], map_candle)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub(super) fn open(&self) -> Result<Connection, StorageError> {
        let connection =
            Connection::open_with_flags(&self.database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }

    pub(super) fn open_write(&self) -> Result<Connection, StorageError> {
        let connection = Connection::open(&self.database_path)?;
        connection.busy_timeout(std::time::Duration::from_secs(10))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )?;
        Ok(connection)
    }
}

#[derive(Serialize, Clone)]
pub struct AuthAuditEvent {
    pub event_id: i64,
    pub event_type: String,
    pub occurred_at_ms: i64,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct DatasetSummary {
    pub dataset_id: i64,
    pub venue: String,
    pub symbol: String,
    pub market_type: String,
    pub interval: String,
    pub source: String,
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub downloaded_at_ms: i64,
    pub status: String,
    pub row_count: i64,
}

#[derive(Serialize, Clone)]
pub struct DataDownload {
    pub download_id: i64,
    pub provider: String,
    pub symbol: String,
    pub market_type: String,
    pub name: String,
    pub interval: String,
    pub requested_start_time_ms: Option<i64>,
    pub created_at_ms: i64,
    pub last_downloaded_at_ms: Option<i64>,
    pub data_start_time_ms: Option<i64>,
    pub data_end_time_ms: Option<i64>,
    pub status: String,
}

pub struct DataDownloadSpec {
    pub provider: String,
    pub symbol: String,
    pub market_type: String,
    pub name: String,
    pub interval: String,
    pub requested_start_time_ms: i64,
}

pub struct DownloadRunPreparation {
    pub download_id: i64,
    pub provider: String,
    pub symbol: String,
    pub market_type: String,
    pub interval: String,
    pub requested_start_time_ms: i64,
    pub imported_urls: HashSet<String>,
}

pub struct HistoricalKline {
    pub open_time_ms: i64,
    pub close_time_ms: i64,
    pub open_price: f64,
    pub high_price: f64,
    pub low_price: f64,
    pub close_price: f64,
    pub base_volume: f64,
    pub quote_volume: f64,
    pub trade_count: i64,
    pub taker_buy_base_volume: f64,
    pub taker_buy_quote_volume: f64,
}

#[derive(Serialize)]
pub struct OhlcvCandle {
    pub open_time_ms: i64,
    pub close_time_ms: i64,
    pub open_price: f64,
    pub high_price: f64,
    pub low_price: f64,
    pub close_price: f64,
    pub base_volume: f64,
    pub quote_volume: Option<f64>,
    pub trade_count: Option<i64>,
    pub taker_buy_base_volume: Option<f64>,
    pub taker_buy_quote_volume: Option<f64>,
}

fn map_dataset_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<DatasetSummary> {
    Ok(DatasetSummary {
        dataset_id: row.get(0)?,
        venue: row.get(1)?,
        symbol: row.get(2)?,
        market_type: row.get(3)?,
        interval: row.get(4)?,
        source: row.get(5)?,
        start_time_ms: row.get(6)?,
        end_time_ms: row.get(7)?,
        downloaded_at_ms: row.get(8)?,
        status: row.get(9)?,
        row_count: row.get(10)?,
    })
}

fn map_data_download(row: &rusqlite::Row<'_>) -> rusqlite::Result<DataDownload> {
    Ok(DataDownload {
        download_id: row.get(0)?,
        provider: row.get(1)?,
        symbol: row.get(2)?,
        market_type: row.get(3)?,
        name: row.get(4)?,
        interval: row.get(5)?,
        requested_start_time_ms: row.get(6)?,
        created_at_ms: row.get(7)?,
        last_downloaded_at_ms: row.get(8)?,
        data_start_time_ms: row.get(9)?,
        data_end_time_ms: row.get(10)?,
        status: row.get(11)?,
    })
}

fn ensure_instrument(
    transaction: &rusqlite::Transaction<'_>,
    symbol: &str,
    market_type: &str,
    now_ms: i64,
) -> Result<i64, StorageError> {
    transaction.execute(
        "INSERT INTO market_instruments (venue, market_type, symbol, contract_type, created_at_ms)
         VALUES ('binance', ?1, ?2, ?3, ?4)
         ON CONFLICT(venue, market_type, symbol) DO NOTHING",
        params![
            market_type,
            symbol,
            if market_type == "usd_m_perpetual" {
                Some("perpetual")
            } else {
                None
            },
            now_ms,
        ],
    )?;
    Ok(transaction.query_row(
        "SELECT instrument_id FROM market_instruments
         WHERE venue = 'binance' AND market_type = ?1 AND symbol = ?2",
        params![market_type, symbol],
        |row| row.get(0),
    )?)
}

fn ensure_kline_dataset(
    transaction: &rusqlite::Transaction<'_>,
    instrument_id: i64,
    interval: &str,
    start_time_ms: i64,
    end_time_ms: i64,
    now_ms: i64,
) -> Result<i64, StorageError> {
    transaction.execute(
        "INSERT INTO historical_datasets (
            instrument_id, dataset_kind, interval, source,
            start_time_ms, end_time_ms, downloaded_at_ms, status
         ) VALUES (?1, 'traded_kline', ?2, 'binance_public_data', ?3, ?4, ?5, 'complete')
         ON CONFLICT(instrument_id, dataset_kind, interval, source) DO UPDATE SET
            start_time_ms = MIN(historical_datasets.start_time_ms, excluded.start_time_ms),
            end_time_ms = MAX(historical_datasets.end_time_ms, excluded.end_time_ms),
            downloaded_at_ms = excluded.downloaded_at_ms,
            status = 'complete'",
        params![instrument_id, interval, start_time_ms, end_time_ms, now_ms],
    )?;
    Ok(transaction.query_row(
        "SELECT dataset_id FROM historical_datasets
         WHERE instrument_id = ?1
           AND dataset_kind = 'traded_kline'
           AND interval = ?2
           AND source = 'binance_public_data'",
        params![instrument_id, interval],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::{DataDownloadSpec, StorageReader};
    use crate::storage::StorageError;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn persists_and_lists_data_download_definitions() {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before Unix epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "binance-grid-data-download-test-{}-{unique_suffix}.sqlite3",
            std::process::id()
        ));
        let reader = StorageReader::new(database_path.clone());
        let spec = DataDownloadSpec {
            provider: "binance".into(),
            symbol: "XAGUSDT".into(),
            market_type: "usd_m_perpetual".into(),
            name: "Silver futures 1m".into(),
            interval: "1m".into(),
            requested_start_time_ms: 1_785_542_400_000,
        };

        let saved = reader
            .create_data_download(&spec)
            .expect("save data download definition");
        assert_eq!(saved.symbol, "XAGUSDT");
        assert_eq!(saved.status, "not_started");

        let entries = reader.data_downloads().expect("list data downloads");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].download_id, saved.download_id);
        assert_eq!(entries[0].market_type, "usd_m_perpetual");
        assert_eq!(entries[0].requested_start_time_ms, Some(1_785_542_400_000));
        assert!(entries[0].last_downloaded_at_ms.is_none());
        assert!(entries[0].data_start_time_ms.is_none());
        assert!(entries[0].data_end_time_ms.is_none());

        let updated = reader
            .update_data_download_start_date(saved.download_id, 1_767_196_800_000)
            .expect("update data download start date");
        assert_eq!(updated.requested_start_time_ms, Some(1_767_196_800_000));

        let duplicate = match reader.create_data_download(&spec) {
            Ok(_) => panic!("duplicate data download definition should fail"),
            Err(error) => error,
        };
        assert!(matches!(
            duplicate,
            StorageError::DataDownloadAlreadyExists { .. }
        ));

        let _ = std::fs::remove_file(&database_path);
        let _ = std::fs::remove_file(database_path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(database_path.with_extension("sqlite3-shm"));
    }
}

fn map_candle(row: &rusqlite::Row<'_>) -> rusqlite::Result<OhlcvCandle> {
    Ok(OhlcvCandle {
        open_time_ms: row.get(0)?,
        close_time_ms: row.get(1)?,
        open_price: row.get(2)?,
        high_price: row.get(3)?,
        low_price: row.get(4)?,
        close_price: row.get(5)?,
        base_volume: row.get(6)?,
        quote_volume: row.get(7)?,
        trade_count: row.get(8)?,
        taker_buy_base_volume: row.get(9)?,
        taker_buy_quote_volume: row.get(10)?,
    })
}
