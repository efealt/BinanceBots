use crate::storage::{DownloadRunPreparation, HistoricalKline, StorageError, StorageReader};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use reqwest::{Client, StatusCode};
use sha2::{Digest, Sha256};
use std::{io::Cursor, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{sync::Mutex, time::sleep};

const ARCHIVE_DELAY: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone)]
pub struct ArchiveDownloader {
    storage: Arc<StorageReader>,
    http: Client,
    run_guard: Arc<Mutex<()>>,
}

impl ArchiveDownloader {
    pub fn new(storage: Arc<StorageReader>) -> Result<Self, ArchiveDownloadError> {
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent("binance-grid-research-archive-importer/1.0")
            .build()?;
        Ok(Self {
            storage,
            http,
            run_guard: Arc::new(Mutex::new(())),
        })
    }

    pub async fn run(
        &self,
        download_id: i64,
        requested_start_time_ms: Option<i64>,
    ) -> Result<ArchiveDownloadResult, ArchiveDownloadError> {
        let _guard = self
            .run_guard
            .try_lock()
            .map_err(|_| ArchiveDownloadError::AlreadyRunning)?;
        let storage = Arc::clone(&self.storage);
        let preparation = tokio::task::spawn_blocking(move || {
            storage.prepare_archive_download(download_id, requested_start_time_ms)
        })
        .await??;

        if preparation.provider != "binance" || preparation.interval != "1m" {
            self.finish(&preparation, "failed").await?;
            return Err(ArchiveDownloadError::UnsupportedEntry);
        }

        let archive_plan = plan_archives(&preparation)?;
        let missing = archive_plan
            .iter()
            .filter(|archive| !preparation.imported_urls.contains(&archive.url))
            .cloned()
            .collect::<Vec<_>>();
        let already_present = archive_plan.len().saturating_sub(missing.len());
        let mut imported_archives = 0_usize;
        let mut imported_rows = 0_usize;
        let mut failures = Vec::new();

        for (index, archive) in missing.iter().cloned().enumerate() {
            match self.download_and_parse(&archive).await {
                Ok((checksum, rows)) => {
                    let storage = Arc::clone(&self.storage);
                    let preparation = preparation.clone_without_urls();
                    let source_url = archive.url.clone();
                    let archive_kind = archive.kind.as_str().to_string();
                    let row_count = rows.len();
                    tokio::task::spawn_blocking(move || {
                        storage.import_historical_klines(
                            &preparation,
                            &source_url,
                            &archive_kind,
                            &checksum,
                            &rows,
                        )
                    })
                    .await??;
                    imported_archives += 1;
                    imported_rows += row_count;
                }
                Err(error) => {
                    let reason = error.to_string();
                    let storage = Arc::clone(&self.storage);
                    let preparation = preparation.clone_without_urls();
                    let source_url = archive.url.clone();
                    let archive_kind = archive.kind.as_str().to_string();
                    let start_time_ms = archive.start_time_ms;
                    let end_time_ms = archive.end_time_ms;
                    let failure_reason = reason.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        storage.record_archive_failure(
                            &preparation,
                            &source_url,
                            &archive_kind,
                            start_time_ms,
                            end_time_ms,
                            &failure_reason,
                        )
                    })
                    .await;
                    failures.push(format!("{}: {reason}", archive.label));
                }
            }

            if index + 1 < missing.len() {
                sleep(ARCHIVE_DELAY).await;
            }
        }

        let status = if failures.is_empty() {
            "complete"
        } else if imported_archives > 0 || already_present > 0 {
            "partial"
        } else {
            "failed"
        };
        self.finish(&preparation, status).await?;
        Ok(ArchiveDownloadResult {
            status: status.into(),
            archives_imported: imported_archives,
            archives_already_present: already_present,
            rows_imported: imported_rows,
            failed_archives: failures,
        })
    }

    async fn finish(
        &self,
        preparation: &DownloadRunPreparation,
        status: &str,
    ) -> Result<(), ArchiveDownloadError> {
        let storage = Arc::clone(&self.storage);
        let preparation = preparation.clone_without_urls();
        let status = status.to_string();
        tokio::task::spawn_blocking(move || storage.finish_archive_download(&preparation, &status))
            .await??;
        Ok(())
    }

    async fn download_and_parse(
        &self,
        archive: &ArchiveSpec,
    ) -> Result<(String, Vec<HistoricalKline>), ArchiveDownloadError> {
        let archive_bytes = self.download_bytes(&archive.url).await?;
        let checksum_text = self
            .download_text(&format!("{}.CHECKSUM", archive.url))
            .await?;
        let checksum = parse_checksum(&checksum_text)?;
        let actual_checksum = format!("{:x}", Sha256::digest(&archive_bytes));
        if checksum != actual_checksum {
            return Err(ArchiveDownloadError::ChecksumMismatch {
                expected: checksum,
                actual: actual_checksum,
            });
        }
        let rows = parse_klines(&archive_bytes, archive.start_time_ms, archive.end_time_ms)?;
        Ok((actual_checksum, rows))
    }

    async fn download_text(&self, url: &str) -> Result<String, ArchiveDownloadError> {
        let bytes = self.download_bytes(url).await?;
        String::from_utf8(bytes).map_err(|_| ArchiveDownloadError::InvalidChecksum)
    }

    async fn download_bytes(&self, url: &str) -> Result<Vec<u8>, ArchiveDownloadError> {
        let mut last_error = None;
        for attempt in 0..3 {
            match self.http.get(url).send().await {
                Ok(response) if response.status().is_success() => {
                    if response
                        .content_length()
                        .is_some_and(|length| length > MAX_ARCHIVE_BYTES)
                    {
                        return Err(ArchiveDownloadError::ArchiveTooLarge);
                    }
                    let bytes = response.bytes().await?;
                    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
                        return Err(ArchiveDownloadError::ArchiveTooLarge);
                    }
                    return Ok(bytes.to_vec());
                }
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::NOT_FOUND || !status.is_server_error() {
                        return Err(ArchiveDownloadError::HttpStatus(status));
                    }
                    last_error = Some(ArchiveDownloadError::HttpStatus(status));
                }
                Err(error) => last_error = Some(ArchiveDownloadError::Request(error)),
            }
            if attempt < 2 {
                sleep(Duration::from_secs(2 + attempt)).await;
            }
        }
        Err(last_error.expect("retry loop records an error"))
    }
}

#[derive(serde::Serialize)]
pub struct ArchiveDownloadResult {
    pub status: String,
    pub archives_imported: usize,
    pub archives_already_present: usize,
    pub rows_imported: usize,
    pub failed_archives: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ArchiveDownloadError {
    #[error("another archive import is already running")]
    AlreadyRunning,
    #[error("only Binance 1-minute kline entries can use this archive importer")]
    UnsupportedEntry,
    #[error("archive request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("archive returned HTTP {0}")]
    HttpStatus(StatusCode),
    #[error("archive exceeds the 256 MiB safety limit")]
    ArchiveTooLarge,
    #[error("archive checksum is malformed")]
    InvalidChecksum,
    #[error("archive checksum did not match (expected {expected}, received {actual})")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("archive ZIP could not be read: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("archive CSV could not be read: {0}")]
    Csv(#[from] csv::Error),
    #[error("archive contains no valid requested 1-minute candles")]
    NoKlines,
    #[error("archive row {row} is invalid")]
    InvalidKline { row: usize },
    #[error("requested start date is invalid")]
    InvalidStartDate,
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("archive task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

#[derive(Clone)]
struct ArchiveSpec {
    kind: ArchiveKind,
    url: String,
    label: String,
    start_time_ms: i64,
    end_time_ms: i64,
}

#[derive(Clone, Copy)]
enum ArchiveKind {
    Monthly,
    Daily,
}

impl ArchiveKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Monthly => "monthly_zip",
            Self::Daily => "daily_zip",
        }
    }
}

fn plan_archives(
    preparation: &DownloadRunPreparation,
) -> Result<Vec<ArchiveSpec>, ArchiveDownloadError> {
    let start = DateTime::from_timestamp_millis(preparation.requested_start_time_ms)
        .ok_or(ArchiveDownloadError::InvalidStartDate)?
        .date_naive();
    let today = Utc::now().date_naive();
    let Some(last_available_day) = today.pred_opt() else {
        return Ok(Vec::new());
    };
    if start > last_available_day {
        return Ok(Vec::new());
    }

    let market_path = match preparation.market_type.as_str() {
        "spot" => "spot",
        "usd_m_perpetual" => "futures/um",
        _ => return Err(ArchiveDownloadError::UnsupportedEntry),
    };
    let mut cursor = start;
    let mut plan = Vec::new();
    while cursor <= last_available_day {
        let month_end = last_day_of_month(cursor)?;
        if cursor.day() == 1 && month_end <= last_available_day {
            let file = format!(
                "{}-1m-{:04}-{:02}.zip",
                preparation.symbol,
                cursor.year(),
                cursor.month()
            );
            plan.push(ArchiveSpec {
                kind: ArchiveKind::Monthly,
                url: format!(
                    "https://data.binance.vision/data/{market_path}/monthly/klines/{}/1m/{file}",
                    preparation.symbol
                ),
                label: format!("{:04}-{:02} monthly", cursor.year(), cursor.month()),
                start_time_ms: date_start_ms(cursor)?,
                end_time_ms: date_end_ms(month_end)?,
            });
            cursor = month_end
                .succ_opt()
                .ok_or(ArchiveDownloadError::InvalidStartDate)?;
        } else {
            let file = format!(
                "{}-1m-{:04}-{:02}-{:02}.zip",
                preparation.symbol,
                cursor.year(),
                cursor.month(),
                cursor.day()
            );
            plan.push(ArchiveSpec {
                kind: ArchiveKind::Daily,
                url: format!(
                    "https://data.binance.vision/data/{market_path}/daily/klines/{}/1m/{file}",
                    preparation.symbol
                ),
                label: format!(
                    "{:04}-{:02}-{:02} daily",
                    cursor.year(),
                    cursor.month(),
                    cursor.day()
                ),
                start_time_ms: date_start_ms(cursor)?,
                end_time_ms: date_end_ms(cursor)?,
            });
            cursor = cursor
                .succ_opt()
                .ok_or(ArchiveDownloadError::InvalidStartDate)?;
        }
    }
    Ok(plan)
}

fn last_day_of_month(date: NaiveDate) -> Result<NaiveDate, ArchiveDownloadError> {
    let first_next_month = if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1)
    }
    .ok_or(ArchiveDownloadError::InvalidStartDate)?;
    first_next_month
        .pred_opt()
        .ok_or(ArchiveDownloadError::InvalidStartDate)
}

fn date_start_ms(date: NaiveDate) -> Result<i64, ArchiveDownloadError> {
    date.and_hms_opt(0, 0, 0)
        .map(|value| value.and_utc().timestamp_millis())
        .ok_or(ArchiveDownloadError::InvalidStartDate)
}

fn date_end_ms(date: NaiveDate) -> Result<i64, ArchiveDownloadError> {
    date.and_hms_milli_opt(23, 59, 59, 999)
        .map(|value| value.and_utc().timestamp_millis())
        .ok_or(ArchiveDownloadError::InvalidStartDate)
}

fn parse_checksum(body: &str) -> Result<String, ArchiveDownloadError> {
    let checksum = body
        .split_whitespace()
        .next()
        .filter(|value| {
            value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
        })
        .ok_or(ArchiveDownloadError::InvalidChecksum)?;
    Ok(checksum.to_lowercase())
}

fn parse_klines(
    archive_bytes: &[u8],
    start_time_ms: i64,
    end_time_ms: i64,
) -> Result<Vec<HistoricalKline>, ArchiveDownloadError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(archive_bytes))?;
    let index = (0..archive.len())
        .find(|index| {
            archive
                .by_index(*index)
                .is_ok_and(|file| file.name().ends_with(".csv"))
        })
        .ok_or(ArchiveDownloadError::NoKlines)?;
    let file = archive.by_index(index)?;
    let mut csv_reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(file);
    let mut rows = Vec::new();
    for (row_index, record) in csv_reader.records().enumerate() {
        let record = record?;
        if record.get(0).is_some_and(|value| {
            value.eq_ignore_ascii_case("open time") || value.eq_ignore_ascii_case("open_time")
        }) {
            continue;
        }
        if record.len() < 11 {
            return Err(ArchiveDownloadError::InvalidKline { row: row_index + 1 });
        }
        let kline = HistoricalKline {
            open_time_ms: parse_field(&record, 0, row_index)?,
            open_price: parse_field(&record, 1, row_index)?,
            high_price: parse_field(&record, 2, row_index)?,
            low_price: parse_field(&record, 3, row_index)?,
            close_price: parse_field(&record, 4, row_index)?,
            base_volume: parse_field(&record, 5, row_index)?,
            close_time_ms: parse_field(&record, 6, row_index)?,
            quote_volume: parse_field(&record, 7, row_index)?,
            trade_count: parse_field(&record, 8, row_index)?,
            taker_buy_base_volume: parse_field(&record, 9, row_index)?,
            taker_buy_quote_volume: parse_field(&record, 10, row_index)?,
        };
        if kline.open_time_ms >= start_time_ms && kline.open_time_ms <= end_time_ms {
            rows.push(kline);
        }
    }
    rows.sort_by_key(|row| row.open_time_ms);
    if rows.is_empty() {
        return Err(ArchiveDownloadError::NoKlines);
    }
    Ok(rows)
}

fn parse_field<T: std::str::FromStr>(
    record: &csv::StringRecord,
    index: usize,
    row: usize,
) -> Result<T, ArchiveDownloadError> {
    record
        .get(index)
        .and_then(|value| value.parse().ok())
        .ok_or(ArchiveDownloadError::InvalidKline { row: row + 1 })
}

impl DownloadRunPreparation {
    fn clone_without_urls(&self) -> Self {
        Self {
            download_id: self.download_id,
            provider: self.provider.clone(),
            symbol: self.symbol.clone(),
            market_type: self.market_type.clone(),
            interval: self.interval.clone(),
            requested_start_time_ms: self.requested_start_time_ms,
            imported_urls: Default::default(),
        }
    }
}
