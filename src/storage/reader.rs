use super::StorageError;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use std::{collections::HashSet, path::PathBuf};

const MAX_CHART_POINTS: i64 = 2_000;

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

        let connection = Connection::open(&self.database_path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )?;
        super::schema::migrate(&connection)?;
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

    pub fn captures(&self) -> Result<Vec<CaptureSummary>, StorageError> {
        if !self.database_path.exists() {
            return Ok(Vec::new());
        }

        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT
                c.capture_id,
                i.venue,
                i.symbol,
                i.market_type,
                c.interval,
                c.source,
                c.started_at_ms,
                c.ended_at_ms,
                c.status,
                c.stop_reason,
                (SELECT COUNT(*) FROM live_events e WHERE e.capture_id = c.capture_id),
                (SELECT COUNT(*) FROM live_kline_updates k WHERE k.capture_id = c.capture_id),
                (SELECT COUNT(*) FROM live_book_ticker b WHERE b.capture_id = c.capture_id),
                (SELECT COUNT(*) FROM live_depth_snapshots d WHERE d.capture_id = c.capture_id),
                (SELECT COUNT(*) FROM live_trades t WHERE t.capture_id = c.capture_id)
             FROM live_capture_sessions c
             JOIN market_instruments i ON i.instrument_id = c.instrument_id
             WHERE c.status = 'completed'
             ORDER BY c.started_at_ms DESC, c.capture_id DESC",
        )?;

        let rows = statement.query_map([], map_capture_summary)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn inspect(
        &self,
        dataset_id: i64,
        page: i64,
        page_size: i64,
    ) -> Result<DatasetInspection, StorageError> {
        let connection = self.open()?;
        let summary = query_dataset_summary(&connection, dataset_id)?
            .ok_or(StorageError::DatasetNotFound(dataset_id))?;
        let mut diagnostics = query_diagnostics(&connection, dataset_id, &summary.interval)?;
        let chart_candles = query_chart_candles(&connection, dataset_id, summary.row_count)?;
        let (candle_rows, candle_page) =
            query_candle_rows(&connection, dataset_id, page, page_size)?;
        diagnostics.chart_point_count = chart_candles.len() as i64;
        diagnostics.chart_is_sampled = (chart_candles.len() as i64) < summary.row_count;

        Ok(DatasetInspection {
            summary,
            diagnostics,
            chart_candles,
            candle_rows,
            candle_page,
        })
    }

    pub fn ohlcv_series(&self, dataset_id: i64) -> Result<Vec<InspectionCandle>, StorageError> {
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

    pub fn inspect_capture(
        &self,
        capture_id: i64,
        page: i64,
        page_size: i64,
        event_type: Option<&str>,
    ) -> Result<CaptureInspection, StorageError> {
        let connection = self.open()?;
        let summary = query_capture_summary(&connection, capture_id)?
            .ok_or(StorageError::CaptureNotFound(capture_id))?;
        let mut diagnostics =
            query_capture_diagnostics(&connection, capture_id, &summary.interval)?;
        let chart_candles =
            query_capture_chart_candles(&connection, capture_id, diagnostics.unique_candle_count)?;
        let tick_quotes = query_capture_quotes(&connection, capture_id)?;
        let tick_trades = query_capture_trades(&connection, capture_id)?;
        let latest_depth = query_latest_depth(&connection, capture_id)?;
        let depth_history = query_depth_history(&connection, capture_id)?;
        let (event_rows, event_page) =
            query_capture_events(&connection, capture_id, event_type, page, page_size)?;
        diagnostics.chart_point_count = chart_candles.len() as i64;

        Ok(CaptureInspection {
            summary,
            diagnostics,
            chart_candles,
            tick_quotes,
            tick_trades,
            latest_depth,
            depth_history,
            event_rows,
            event_page,
        })
    }

    fn open(&self) -> Result<Connection, StorageError> {
        let connection =
            Connection::open_with_flags(&self.database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }

    fn open_write(&self) -> Result<Connection, StorageError> {
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
pub struct DatasetInspection {
    pub summary: DatasetSummary,
    pub diagnostics: DataDiagnostics,
    pub chart_candles: Vec<InspectionCandle>,
    pub candle_rows: Vec<InspectionCandle>,
    pub candle_page: PageInfo,
}

#[derive(Serialize, Clone)]
pub struct CaptureSummary {
    pub capture_id: i64,
    pub venue: String,
    pub symbol: String,
    pub market_type: String,
    pub interval: String,
    pub source: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub status: String,
    pub stop_reason: Option<String>,
    pub raw_event_count: i64,
    pub kline_update_count: i64,
    pub book_ticker_count: i64,
    pub depth_snapshot_count: i64,
    pub trade_count: i64,
}

#[derive(Serialize)]
pub struct CaptureInspection {
    pub summary: CaptureSummary,
    pub diagnostics: DataDiagnostics,
    pub chart_candles: Vec<InspectionCandle>,
    pub tick_quotes: Vec<TickQuote>,
    pub tick_trades: Vec<TickTrade>,
    pub latest_depth: Option<DepthSnapshot>,
    pub depth_history: Vec<DepthSnapshot>,
    pub event_rows: Vec<StoredMarketEvent>,
    pub event_page: PageInfo,
}

#[derive(Serialize)]
pub struct PageInfo {
    pub page: i64,
    pub page_size: i64,
    pub total_rows: i64,
    pub total_pages: i64,
}

#[derive(Serialize)]
pub struct TickQuote {
    pub event_time_ms: Option<i64>,
    pub received_at_ms: i64,
    pub bid_price: f64,
    pub bid_quantity: f64,
    pub ask_price: f64,
    pub ask_quantity: f64,
}

#[derive(Serialize)]
pub struct TickTrade {
    pub event_time_ms: Option<i64>,
    pub received_at_ms: i64,
    pub price: f64,
    pub quantity: f64,
    pub is_buyer_maker: bool,
}

#[derive(Serialize)]
pub struct DepthSnapshot {
    pub event_id: i64,
    pub event_time_ms: Option<i64>,
    pub received_at_ms: i64,
    pub update_id: Option<i64>,
    pub bids: Vec<DepthLevel>,
    pub asks: Vec<DepthLevel>,
}

#[derive(Serialize)]
pub struct DepthLevel {
    pub price: f64,
    pub quantity: f64,
}

#[derive(Serialize)]
pub struct StoredMarketEvent {
    pub event_id: i64,
    pub event_type: String,
    pub stream_name: String,
    pub event_time_ms: Option<i64>,
    pub received_at_ms: i64,
    pub price: Option<f64>,
    pub quantity: Option<f64>,
    pub bid_price: Option<f64>,
    pub ask_price: Option<f64>,
    pub bid_quantity: Option<f64>,
    pub ask_quantity: Option<f64>,
    pub depth_level_count: Option<i64>,
    pub is_buyer_maker: Option<bool>,
    pub raw_payload: String,
}

#[derive(Serialize)]
pub struct DataDiagnostics {
    pub expected_interval_ms: Option<i64>,
    pub first_time_ms: Option<i64>,
    pub last_time_ms: Option<i64>,
    pub coverage_ms: Option<i64>,
    pub gap_count: i64,
    pub missing_candle_count: i64,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
    pub first_close: Option<f64>,
    pub last_close: Option<f64>,
    pub total_volume: Option<f64>,
    pub chart_point_count: i64,
    pub chart_is_sampled: bool,
    pub raw_event_count: Option<i64>,
    pub kline_update_count: Option<i64>,
    pub book_ticker_count: Option<i64>,
    pub depth_snapshot_count: Option<i64>,
    pub trade_count: Option<i64>,
    #[serde(skip)]
    unique_candle_count: i64,
}

#[derive(Serialize)]
pub struct InspectionCandle {
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

fn query_dataset_summary(
    connection: &Connection,
    dataset_id: i64,
) -> Result<Option<DatasetSummary>, StorageError> {
    Ok(connection
        .query_row(
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
             WHERE d.dataset_id = ?1 AND d.dataset_kind = 'traded_kline'",
            params![dataset_id],
            map_dataset_summary,
        )
        .optional()?)
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

fn map_capture_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<CaptureSummary> {
    Ok(CaptureSummary {
        capture_id: row.get(0)?,
        venue: row.get(1)?,
        symbol: row.get(2)?,
        market_type: row.get(3)?,
        interval: row.get(4)?,
        source: row.get(5)?,
        started_at_ms: row.get(6)?,
        ended_at_ms: row.get(7)?,
        status: row.get(8)?,
        stop_reason: row.get(9)?,
        raw_event_count: row.get(10)?,
        kline_update_count: row.get(11)?,
        book_ticker_count: row.get(12)?,
        depth_snapshot_count: row.get(13)?,
        trade_count: row.get(14)?,
    })
}

fn query_diagnostics(
    connection: &Connection,
    dataset_id: i64,
    interval: &str,
) -> Result<DataDiagnostics, StorageError> {
    let (row_count, first_time_ms, last_time_ms, min_price, max_price, total_volume, first_close, last_close): (
        i64,
        Option<i64>,
        Option<i64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    ) = connection.query_row(
        "SELECT
            COUNT(*),
            MIN(open_time_ms),
            MAX(open_time_ms),
            MIN(low_price),
            MAX(high_price),
            SUM(base_volume),
            (SELECT close_price FROM historical_ohlcv WHERE dataset_id = ?1 ORDER BY open_time_ms ASC LIMIT 1),
            (SELECT close_price FROM historical_ohlcv WHERE dataset_id = ?1 ORDER BY open_time_ms DESC LIMIT 1)
         FROM historical_ohlcv
         WHERE dataset_id = ?1",
        params![dataset_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        },
    )?;

    let expected_interval_ms = interval_milliseconds(interval);
    let (gap_count, missing_candle_count) = match expected_interval_ms {
        Some(interval_ms) if row_count > 1 => connection.query_row(
            "WITH ordered AS (
                SELECT
                    open_time_ms,
                    LAG(open_time_ms) OVER (ORDER BY open_time_ms) AS previous_time_ms
                FROM historical_ohlcv
                WHERE dataset_id = ?1
            ), gaps AS (
                SELECT open_time_ms - previous_time_ms AS delta_ms
                FROM ordered
                WHERE previous_time_ms IS NOT NULL
            )
            SELECT
                COALESCE(SUM(CASE WHEN delta_ms > ?2 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN delta_ms > ?2 THEN CAST(delta_ms / ?2 AS INTEGER) - 1 ELSE 0 END), 0)
            FROM gaps",
            params![dataset_id, interval_ms],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?,
        _ => (0, 0),
    };

    Ok(DataDiagnostics {
        expected_interval_ms,
        first_time_ms,
        last_time_ms,
        coverage_ms: first_time_ms
            .zip(last_time_ms)
            .map(|(first, last)| last - first),
        gap_count,
        missing_candle_count,
        min_price,
        max_price,
        first_close,
        last_close,
        total_volume,
        chart_point_count: 0,
        chart_is_sampled: false,
        raw_event_count: None,
        kline_update_count: None,
        book_ticker_count: None,
        depth_snapshot_count: None,
        trade_count: None,
        unique_candle_count: row_count,
    })
}

fn query_chart_candles(
    connection: &Connection,
    dataset_id: i64,
    row_count: i64,
) -> Result<Vec<InspectionCandle>, StorageError> {
    let bucket_size = if row_count > MAX_CHART_POINTS {
        (row_count + MAX_CHART_POINTS - 1) / MAX_CHART_POINTS
    } else {
        1
    };
    let mut statement = connection.prepare(
        "WITH numbered AS (
            SELECT
                open_time_ms,
                close_time_ms,
                open_price,
                high_price,
                low_price,
                close_price,
                base_volume,
                quote_volume,
                trade_count,
                taker_buy_base_volume,
                taker_buy_quote_volume,
                ((ROW_NUMBER() OVER (ORDER BY open_time_ms) - 1) / ?2) AS bucket
            FROM historical_ohlcv
            WHERE dataset_id = ?1
        ), bucketed AS (
            SELECT
                bucket,
                MIN(open_time_ms) AS open_time_ms,
                MAX(close_time_ms) AS close_time_ms,
                MAX(high_price) AS high_price,
                MIN(low_price) AS low_price,
                SUM(base_volume) AS base_volume,
                SUM(quote_volume) AS quote_volume,
                SUM(trade_count) AS trade_count,
                SUM(taker_buy_base_volume) AS taker_buy_base_volume,
                SUM(taker_buy_quote_volume) AS taker_buy_quote_volume
            FROM numbered
            GROUP BY bucket
        )
        SELECT
            b.open_time_ms,
            b.close_time_ms,
            (SELECT n.open_price FROM numbered n WHERE n.bucket = b.bucket ORDER BY n.open_time_ms ASC LIMIT 1),
            b.high_price,
            b.low_price,
            (SELECT n.close_price FROM numbered n WHERE n.bucket = b.bucket ORDER BY n.open_time_ms DESC LIMIT 1),
            b.base_volume,
            b.quote_volume,
            b.trade_count,
            b.taker_buy_base_volume,
            b.taker_buy_quote_volume
        FROM bucketed b
        ORDER BY b.open_time_ms",
    )?;
    let rows = statement.query_map(params![dataset_id, bucket_size], map_candle)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn query_candle_rows(
    connection: &Connection,
    dataset_id: i64,
    page: i64,
    page_size: i64,
) -> Result<(Vec<InspectionCandle>, PageInfo), StorageError> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 500);
    let offset = page.saturating_sub(1).saturating_mul(page_size);
    let total_rows: i64 = connection.query_row(
        "SELECT COUNT(*) FROM historical_ohlcv WHERE dataset_id = ?1",
        params![dataset_id],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT
            open_time_ms, close_time_ms, open_price, high_price, low_price,
            close_price, base_volume, quote_volume, trade_count,
            taker_buy_base_volume, taker_buy_quote_volume
         FROM historical_ohlcv
         WHERE dataset_id = ?1
         ORDER BY open_time_ms DESC
         LIMIT ?2 OFFSET ?3",
    )?;
    let rows = statement.query_map(params![dataset_id, page_size, offset], map_candle)?;
    let candles = rows.collect::<Result<Vec<_>, _>>()?;
    Ok((candles, page_info(page, page_size, total_rows)))
}

fn query_capture_summary(
    connection: &Connection,
    capture_id: i64,
) -> Result<Option<CaptureSummary>, StorageError> {
    Ok(connection
        .query_row(
            "SELECT
                c.capture_id,
                i.venue,
                i.symbol,
                i.market_type,
                c.interval,
                c.source,
                c.started_at_ms,
                c.ended_at_ms,
                c.status,
                c.stop_reason,
                (SELECT COUNT(*) FROM live_events e WHERE e.capture_id = c.capture_id),
                (SELECT COUNT(*) FROM live_kline_updates k WHERE k.capture_id = c.capture_id),
                (SELECT COUNT(*) FROM live_book_ticker b WHERE b.capture_id = c.capture_id),
                (SELECT COUNT(*) FROM live_depth_snapshots d WHERE d.capture_id = c.capture_id),
                (SELECT COUNT(*) FROM live_trades t WHERE t.capture_id = c.capture_id)
             FROM live_capture_sessions c
             JOIN market_instruments i ON i.instrument_id = c.instrument_id
             WHERE c.capture_id = ?1",
            params![capture_id],
            map_capture_summary,
        )
        .optional()?)
}

fn query_capture_diagnostics(
    connection: &Connection,
    capture_id: i64,
    interval: &str,
) -> Result<DataDiagnostics, StorageError> {
    let (
        raw_event_count,
        kline_update_count,
        book_ticker_count,
        depth_snapshot_count,
        trade_count,
        unique_candle_count,
        first_time_ms,
        last_time_ms,
        min_price,
        max_price,
        total_volume,
        first_close,
        last_close,
    ): (
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        Option<i64>,
        Option<i64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    ) = connection.query_row(
        "SELECT
            (SELECT COUNT(*) FROM live_events WHERE capture_id = ?1),
            (SELECT COUNT(*) FROM live_kline_updates WHERE capture_id = ?1),
            (SELECT COUNT(*) FROM live_book_ticker WHERE capture_id = ?1),
            (SELECT COUNT(*) FROM live_depth_snapshots WHERE capture_id = ?1),
            (SELECT COUNT(*) FROM live_trades WHERE capture_id = ?1),
            (SELECT COUNT(DISTINCT open_time_ms) FROM live_kline_updates WHERE capture_id = ?1),
            (SELECT MIN(COALESCE(exchange_event_time_ms, received_at_ms)) FROM live_events WHERE capture_id = ?1),
            (SELECT MAX(COALESCE(exchange_event_time_ms, received_at_ms)) FROM live_events WHERE capture_id = ?1),
            (SELECT MIN(price) FROM (
                SELECT price FROM live_trades WHERE capture_id = ?1
                UNION ALL SELECT bid_price FROM live_book_ticker WHERE capture_id = ?1
                UNION ALL SELECT ask_price FROM live_book_ticker WHERE capture_id = ?1
            )),
            (SELECT MAX(price) FROM (
                SELECT price FROM live_trades WHERE capture_id = ?1
                UNION ALL SELECT bid_price FROM live_book_ticker WHERE capture_id = ?1
                UNION ALL SELECT ask_price FROM live_book_ticker WHERE capture_id = ?1
            )),
            (SELECT SUM(quantity) FROM live_trades WHERE capture_id = ?1),
            (SELECT price FROM live_trades WHERE capture_id = ?1 ORDER BY trade_time_ms ASC, event_id ASC LIMIT 1),
            (SELECT price FROM live_trades WHERE capture_id = ?1 ORDER BY trade_time_ms DESC, event_id DESC LIMIT 1)",
        params![capture_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
            ))
        },
    )?;

    let expected_interval_ms = interval_milliseconds(interval);
    let (gap_count, missing_candle_count) = match expected_interval_ms {
        Some(interval_ms) if unique_candle_count > 1 => connection.query_row(
            "WITH ordered AS (
                SELECT
                    open_time_ms,
                    LAG(open_time_ms) OVER (ORDER BY open_time_ms) AS previous_time_ms
                FROM (
                    SELECT DISTINCT open_time_ms
                    FROM live_kline_updates
                    WHERE capture_id = ?1
                )
            ), gaps AS (
                SELECT open_time_ms - previous_time_ms AS delta_ms
                FROM ordered
                WHERE previous_time_ms IS NOT NULL
            )
            SELECT
                COALESCE(SUM(CASE WHEN delta_ms > ?2 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN delta_ms > ?2 THEN CAST(delta_ms / ?2 AS INTEGER) - 1 ELSE 0 END), 0)
            FROM gaps",
            params![capture_id, interval_ms],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?,
        _ => (0, 0),
    };

    Ok(DataDiagnostics {
        expected_interval_ms,
        first_time_ms,
        last_time_ms,
        coverage_ms: first_time_ms
            .zip(last_time_ms)
            .map(|(first, last)| last - first),
        gap_count,
        missing_candle_count,
        min_price,
        max_price,
        first_close,
        last_close,
        total_volume,
        chart_point_count: 0,
        chart_is_sampled: unique_candle_count > MAX_CHART_POINTS,
        raw_event_count: Some(raw_event_count),
        kline_update_count: Some(kline_update_count),
        book_ticker_count: Some(book_ticker_count),
        depth_snapshot_count: Some(depth_snapshot_count),
        trade_count: Some(trade_count),
        unique_candle_count,
    })
}

fn query_capture_chart_candles(
    connection: &Connection,
    capture_id: i64,
    unique_candle_count: i64,
) -> Result<Vec<InspectionCandle>, StorageError> {
    let bucket_size = if unique_candle_count > MAX_CHART_POINTS {
        (unique_candle_count + MAX_CHART_POINTS - 1) / MAX_CHART_POINTS
    } else {
        1
    };
    let mut statement = connection.prepare(
        "WITH latest AS (
            SELECT
                open_time_ms,
                close_time_ms,
                open_price,
                high_price,
                low_price,
                close_price,
                base_volume,
                NULL AS quote_volume,
                NULL AS trade_count,
                NULL AS taker_buy_base_volume,
                NULL AS taker_buy_quote_volume,
                ROW_NUMBER() OVER (
                    PARTITION BY open_time_ms
                    ORDER BY received_at_ms DESC, event_id DESC
                ) AS row_number
            FROM live_kline_updates
            WHERE capture_id = ?1
        ), numbered AS (
            SELECT
                open_time_ms,
                close_time_ms,
                open_price,
                high_price,
                low_price,
                close_price,
                base_volume,
                quote_volume,
                trade_count,
                taker_buy_base_volume,
                taker_buy_quote_volume,
                ((ROW_NUMBER() OVER (ORDER BY open_time_ms) - 1) / ?2) AS bucket
            FROM latest
            WHERE row_number = 1
        ), bucketed AS (
            SELECT
                bucket,
                MIN(open_time_ms) AS open_time_ms,
                MAX(close_time_ms) AS close_time_ms,
                MAX(high_price) AS high_price,
                MIN(low_price) AS low_price,
                SUM(base_volume) AS base_volume,
                SUM(quote_volume) AS quote_volume,
                SUM(trade_count) AS trade_count,
                SUM(taker_buy_base_volume) AS taker_buy_base_volume,
                SUM(taker_buy_quote_volume) AS taker_buy_quote_volume
            FROM numbered
            GROUP BY bucket
        )
        SELECT
            b.open_time_ms,
            b.close_time_ms,
            (SELECT n.open_price FROM numbered n WHERE n.bucket = b.bucket ORDER BY n.open_time_ms ASC LIMIT 1),
            b.high_price,
            b.low_price,
            (SELECT n.close_price FROM numbered n WHERE n.bucket = b.bucket ORDER BY n.open_time_ms DESC LIMIT 1),
            b.base_volume,
            b.quote_volume,
            b.trade_count,
            b.taker_buy_base_volume,
            b.taker_buy_quote_volume
        FROM bucketed b
        ORDER BY b.open_time_ms",
    )?;
    let rows = statement.query_map(params![capture_id, bucket_size], map_candle)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn query_capture_quotes(
    connection: &Connection,
    capture_id: i64,
) -> Result<Vec<TickQuote>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT
            e.exchange_event_time_ms,
            b.received_at_ms,
            b.bid_price,
            b.bid_quantity,
            b.ask_price,
            b.ask_quantity
         FROM live_book_ticker b
         JOIN live_events e ON e.event_id = b.event_id
         WHERE b.capture_id = ?1
         ORDER BY b.received_at_ms, b.event_id",
    )?;
    let rows = statement.query_map(params![capture_id], |row| {
        Ok(TickQuote {
            event_time_ms: row.get(0)?,
            received_at_ms: row.get(1)?,
            bid_price: row.get(2)?,
            bid_quantity: row.get(3)?,
            ask_price: row.get(4)?,
            ask_quantity: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn query_depth_history(
    connection: &Connection,
    capture_id: i64,
) -> Result<Vec<DepthSnapshot>, StorageError> {
    let mut statement = connection.prepare("SELECT d.event_id, e.exchange_event_time_ms, d.received_at_ms, d.update_id, l.side, l.price, l.quantity FROM live_depth_snapshots d JOIN live_events e ON e.event_id = d.event_id LEFT JOIN live_depth_levels l ON l.event_id = d.event_id WHERE d.capture_id = ?1 ORDER BY d.received_at_ms, d.event_id, l.side, l.level_index")?;
    let mut rows = statement.query(params![capture_id])?;
    let mut snapshots: Vec<DepthSnapshot> = Vec::new();
    while let Some(row) = rows.next()? {
        let event_id = row.get(0)?;
        if snapshots.last().map(|s| s.event_id) != Some(event_id) {
            snapshots.push(DepthSnapshot {
                event_id,
                event_time_ms: row.get(1)?,
                received_at_ms: row.get(2)?,
                update_id: row.get(3)?,
                bids: Vec::new(),
                asks: Vec::new(),
            });
        }
        if let Some(side) = row.get::<_, Option<String>>(4)? {
            let level = DepthLevel {
                price: row.get(5)?,
                quantity: row.get(6)?,
            };
            let snapshot = snapshots.last_mut().unwrap();
            if side == "bid" {
                snapshot.bids.push(level);
            } else {
                snapshot.asks.push(level);
            }
        }
    }
    Ok(snapshots)
}

fn query_latest_depth(
    connection: &Connection,
    capture_id: i64,
) -> Result<Option<DepthSnapshot>, StorageError> {
    let snapshot = connection
        .query_row(
            "SELECT
                d.event_id,
                e.exchange_event_time_ms,
                d.received_at_ms,
                d.update_id
             FROM live_depth_snapshots d
             JOIN live_events e ON e.event_id = d.event_id
             WHERE d.capture_id = ?1
             ORDER BY d.received_at_ms DESC, d.event_id DESC
             LIMIT 1",
            params![capture_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    let Some((event_id, event_time_ms, received_at_ms, update_id)) = snapshot else {
        return Ok(None);
    };

    let mut statement = connection.prepare(
        "SELECT side, price, quantity
         FROM live_depth_levels
         WHERE event_id = ?1
         ORDER BY side, level_index",
    )?;
    let rows = statement.query_map(params![event_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            DepthLevel {
                price: row.get(1)?,
                quantity: row.get(2)?,
            },
        ))
    })?;

    let mut bids = Vec::new();
    let mut asks = Vec::new();
    for row in rows {
        let (side, level) = row?;
        if side == "bid" {
            bids.push(level);
        } else {
            asks.push(level);
        }
    }

    Ok(Some(DepthSnapshot {
        event_id,
        event_time_ms,
        received_at_ms,
        update_id,
        bids,
        asks,
    }))
}

fn query_capture_trades(
    connection: &Connection,
    capture_id: i64,
) -> Result<Vec<TickTrade>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT
            t.trade_time_ms,
            t.received_at_ms,
            t.price,
            t.quantity,
            t.is_buyer_maker
         FROM live_trades t
         WHERE t.capture_id = ?1
         ORDER BY t.received_at_ms, t.event_id",
    )?;
    let rows = statement.query_map(params![capture_id], |row| {
        let is_buyer_maker: i64 = row.get(4)?;
        Ok(TickTrade {
            event_time_ms: row.get(0)?,
            received_at_ms: row.get(1)?,
            price: row.get(2)?,
            quantity: row.get(3)?,
            is_buyer_maker: is_buyer_maker != 0,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn query_capture_events(
    connection: &Connection,
    capture_id: i64,
    event_type: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<(Vec<StoredMarketEvent>, PageInfo), StorageError> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 500);
    let offset = page.saturating_sub(1).saturating_mul(page_size);
    let total_rows: i64 = connection.query_row(
        "SELECT COUNT(*)
         FROM live_events
         WHERE capture_id = ?1
           AND (?2 IS NULL OR event_type = ?2)",
        params![capture_id, event_type],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT
            e.event_id,
            e.event_type,
            e.stream_name,
            COALESCE(e.exchange_event_time_ms, t.trade_time_ms),
            e.received_at_ms,
            t.price,
            t.quantity,
            b.bid_price,
            b.ask_price,
            b.bid_quantity,
            b.ask_quantity,
            CASE WHEN e.event_type = 'depth' THEN (
                SELECT COUNT(*) FROM live_depth_levels l WHERE l.event_id = e.event_id
            ) ELSE NULL END,
            t.is_buyer_maker,
            e.raw_payload
         FROM live_events e
         LEFT JOIN live_trades t ON t.event_id = e.event_id
         LEFT JOIN live_book_ticker b ON b.event_id = e.event_id
         WHERE e.capture_id = ?1
           AND (?2 IS NULL OR e.event_type = ?2)
         ORDER BY e.received_at_ms DESC, e.event_id DESC
         LIMIT ?3 OFFSET ?4",
    )?;
    let rows = statement.query_map(params![capture_id, event_type, page_size, offset], |row| {
        Ok(StoredMarketEvent {
            event_id: row.get(0)?,
            event_type: row.get(1)?,
            stream_name: row.get(2)?,
            event_time_ms: row.get(3)?,
            received_at_ms: row.get(4)?,
            price: row.get(5)?,
            quantity: row.get(6)?,
            bid_price: row.get(7)?,
            ask_price: row.get(8)?,
            bid_quantity: row.get(9)?,
            ask_quantity: row.get(10)?,
            depth_level_count: row.get(11)?,
            is_buyer_maker: row.get::<_, Option<i64>>(12)?.map(|value| value != 0),
            raw_payload: row.get(13)?,
        })
    })?;
    let events = rows.collect::<Result<Vec<_>, _>>()?;
    Ok((events, page_info(page, page_size, total_rows)))
}

fn map_candle(row: &rusqlite::Row<'_>) -> rusqlite::Result<InspectionCandle> {
    Ok(InspectionCandle {
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

fn page_info(page: i64, page_size: i64, total_rows: i64) -> PageInfo {
    PageInfo {
        page,
        page_size,
        total_rows,
        total_pages: if total_rows == 0 {
            0
        } else {
            (total_rows + page_size - 1) / page_size
        },
    }
}

fn interval_milliseconds(interval: &str) -> Option<i64> {
    let (number, unit) = interval.split_at(interval.len().checked_sub(1)?);
    let number: i64 = number.parse().ok()?;
    let multiplier = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        "w" => 604_800_000,
        _ => return None,
    };
    number.checked_mul(multiplier)
}
