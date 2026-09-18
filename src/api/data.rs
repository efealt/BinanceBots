use crate::downloader::{ArchiveDownloadError, ArchiveDownloadResult, ArchiveDownloader};
use crate::storage::{
    CaptureInspection, CaptureSummary, DataDownload, DataDownloadSpec, DatasetInspection,
    DatasetSummary, StorageError, StorageReader,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

const DEFAULT_PAGE_SIZE: i64 = 100;
const MAX_PAGE_SIZE: i64 = 500;

#[derive(Clone)]
struct DataApiState {
    storage_reader: Arc<StorageReader>,
    archive_downloader: Arc<ArchiveDownloader>,
}

pub fn router(storage_reader: Arc<StorageReader>) -> Router {
    let archive_downloader = Arc::new(
        ArchiveDownloader::new(Arc::clone(&storage_reader))
            .expect("archive downloader HTTP client must initialize"),
    );
    Router::new()
        .route("/api/data/datasets", get(datasets))
        .route("/api/data/ohlcv", get(ohlcv))
        .route("/api/data/downloads", get(downloads).post(create_download))
        .route(
            "/api/data/downloads/{download_id}/run",
            axum::routing::post(run_download),
        )
        .route("/api/data/downloads/{download_id}", patch(update_download))
        .route("/api/data/captures", get(captures))
        .route("/api/data/inspection", get(inspection))
        .route("/api/data/capture-inspection", get(capture_inspection))
        .with_state(Arc::new(DataApiState {
            storage_reader,
            archive_downloader,
        }))
}

async fn downloads(
    State(state): State<Arc<DataApiState>>,
) -> Result<Json<DownloadCatalog>, DataApiError> {
    let storage_reader = Arc::clone(&state.storage_reader);
    let downloads = tokio::task::spawn_blocking(move || storage_reader.data_downloads()).await??;
    Ok(Json(DownloadCatalog { downloads }))
}

#[derive(Serialize)]
struct DownloadCatalog {
    downloads: Vec<DataDownload>,
}

#[derive(Deserialize)]
struct CreateDownloadRequest {
    provider: String,
    symbol: String,
    market_type: String,
    name: String,
    interval: String,
    start_date: String,
}

async fn create_download(
    State(state): State<Arc<DataApiState>>,
    Json(request): Json<CreateDownloadRequest>,
) -> Result<Json<DataDownload>, DataApiError> {
    let spec = normalize_download_request(request)?;
    let storage_reader = Arc::clone(&state.storage_reader);
    let download =
        tokio::task::spawn_blocking(move || storage_reader.create_data_download(&spec)).await??;
    Ok(Json(download))
}

#[derive(Deserialize)]
struct RunDownloadRequest {
    start_date: Option<String>,
}

#[derive(Deserialize)]
struct UpdateDownloadRequest {
    start_date: String,
}

async fn update_download(
    State(state): State<Arc<DataApiState>>,
    Path(download_id): Path<i64>,
    Json(request): Json<UpdateDownloadRequest>,
) -> Result<Json<DataDownload>, DataApiError> {
    if download_id <= 0 {
        return Err(DataApiError::InvalidQuery("download id is required".into()));
    }
    let requested_start_time_ms = parse_start_date(&request.start_date)?;
    let storage_reader = Arc::clone(&state.storage_reader);
    let download = tokio::task::spawn_blocking(move || {
        storage_reader.update_data_download_start_date(download_id, requested_start_time_ms)
    })
    .await??;
    Ok(Json(download))
}

async fn run_download(
    State(state): State<Arc<DataApiState>>,
    Path(download_id): Path<i64>,
    Json(request): Json<RunDownloadRequest>,
) -> Result<Json<ArchiveDownloadResult>, DataApiError> {
    if download_id <= 0 {
        return Err(DataApiError::InvalidQuery("download id is required".into()));
    }
    let start_date = request
        .start_date
        .as_deref()
        .map(parse_start_date)
        .transpose()?;
    let result = state
        .archive_downloader
        .run(download_id, start_date)
        .await?;
    Ok(Json(result))
}

fn normalize_download_request(
    request: CreateDownloadRequest,
) -> Result<DataDownloadSpec, DataApiError> {
    let provider = request.provider.trim().to_lowercase();
    if provider != "binance" {
        return Err(DataApiError::InvalidQuery(
            "provider must be binance".into(),
        ));
    }

    let symbol = request.symbol.trim().to_uppercase();
    if symbol.is_empty()
        || !symbol
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return Err(DataApiError::InvalidQuery(
            "symbol must be alphanumeric".into(),
        ));
    }

    let market_type = match request.market_type.trim().to_lowercase().as_str() {
        "spot" => "spot".to_string(),
        "usd_m_perpetual" => "usd_m_perpetual".to_string(),
        _ => {
            return Err(DataApiError::InvalidQuery(
                "market_type must be spot or usd_m_perpetual".into(),
            ));
        }
    };

    let interval = request.interval.trim().to_lowercase();
    if interval != "1m" {
        return Err(DataApiError::InvalidQuery(
            "this archive downloader currently supports only the 1-minute interval".into(),
        ));
    }

    let name = request.name.trim().to_string();
    if name.is_empty() {
        return Err(DataApiError::InvalidQuery("name is required".into()));
    }
    let requested_start_time_ms = parse_start_date(&request.start_date)?;

    Ok(DataDownloadSpec {
        provider,
        symbol,
        market_type,
        name,
        interval,
        requested_start_time_ms,
    })
}

fn parse_start_date(value: &str) -> Result<i64, DataApiError> {
    use chrono::NaiveDate;
    let date = NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map_err(|_| DataApiError::InvalidQuery("start_date must use YYYY-MM-DD".into()))?;
    date.and_hms_opt(0, 0, 0)
        .map(|date_time| date_time.and_utc().timestamp_millis())
        .ok_or_else(|| DataApiError::InvalidQuery("start_date is invalid".into()))
}

async fn datasets(
    State(state): State<Arc<DataApiState>>,
) -> Result<Json<DatasetCatalog>, DataApiError> {
    let storage_reader = Arc::clone(&state.storage_reader);
    let datasets = tokio::task::spawn_blocking(move || storage_reader.catalog()).await??;
    Ok(Json(DatasetCatalog { datasets }))
}

async fn ohlcv(
    State(state): State<Arc<DataApiState>>,
    Query(query): Query<OhlcvQuery>,
) -> Result<Json<OhlcvSeries>, DataApiError> {
    let dataset_id = query
        .dataset_id
        .filter(|dataset_id| *dataset_id > 0)
        .ok_or_else(|| DataApiError::InvalidQuery("dataset_id is required".into()))?;
    let storage_reader = Arc::clone(&state.storage_reader);
    let candles =
        tokio::task::spawn_blocking(move || storage_reader.ohlcv_series(dataset_id)).await??;
    Ok(Json(OhlcvSeries { candles }))
}

#[derive(Deserialize)]
struct OhlcvQuery {
    dataset_id: Option<i64>,
}

#[derive(Serialize)]
struct OhlcvSeries {
    candles: Vec<crate::storage::InspectionCandle>,
}

#[derive(Deserialize)]
struct InspectionQuery {
    dataset_id: Option<i64>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn inspection(
    State(state): State<Arc<DataApiState>>,
    Query(query): Query<InspectionQuery>,
) -> Result<Json<DatasetInspection>, DataApiError> {
    let dataset_id = query
        .dataset_id
        .filter(|dataset_id| *dataset_id > 0)
        .ok_or_else(|| DataApiError::InvalidQuery("dataset_id is required".into()))?;
    let (page, page_size) = page_request(query.page, query.page_size);
    let storage_reader = Arc::clone(&state.storage_reader);
    let inspection =
        tokio::task::spawn_blocking(move || storage_reader.inspect(dataset_id, page, page_size))
            .await??;
    Ok(Json(inspection))
}

#[derive(Serialize)]
struct DatasetCatalog {
    datasets: Vec<DatasetSummary>,
}

async fn captures(
    State(state): State<Arc<DataApiState>>,
) -> Result<Json<CaptureCatalog>, DataApiError> {
    let storage_reader = Arc::clone(&state.storage_reader);
    let captures = tokio::task::spawn_blocking(move || storage_reader.captures()).await??;
    Ok(Json(CaptureCatalog { captures }))
}

#[derive(Serialize)]
struct CaptureCatalog {
    captures: Vec<CaptureSummary>,
}

async fn capture_inspection(
    State(state): State<Arc<DataApiState>>,
    Query(query): Query<CaptureInspectionQuery>,
) -> Result<Json<CaptureInspection>, DataApiError> {
    let capture_id = query
        .capture_id
        .filter(|capture_id| *capture_id > 0)
        .ok_or_else(|| DataApiError::InvalidQuery("capture_id is required".into()))?;
    let event_type = query.event_type.filter(|event_type| event_type != "all");
    if let Some(event_type) = event_type.as_deref()
        && !matches!(event_type, "trade" | "book_ticker" | "depth" | "kline")
    {
        return Err(DataApiError::InvalidQuery(
            "event_type must be all, trade, book_ticker, depth, or kline".into(),
        ));
    }
    let (page, page_size) = page_request(query.page, query.page_size);
    let storage_reader = Arc::clone(&state.storage_reader);
    let inspection = tokio::task::spawn_blocking(move || {
        storage_reader.inspect_capture(capture_id, page, page_size, event_type.as_deref())
    })
    .await??;
    Ok(Json(inspection))
}

#[derive(Deserialize)]
struct CaptureInspectionQuery {
    capture_id: Option<i64>,
    page: Option<i64>,
    page_size: Option<i64>,
    event_type: Option<String>,
}

fn page_request(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    (
        page.unwrap_or(1).max(1),
        page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE),
    )
}

#[derive(Debug, Error)]
enum DataApiError {
    #[error("invalid data inspection query: {0}")]
    InvalidQuery(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Archive(#[from] ArchiveDownloadError),
    #[error("data inspection task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

impl IntoResponse for DataApiError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::InvalidQuery(_) => StatusCode::BAD_REQUEST,
            Self::Storage(StorageError::DatasetNotFound(_))
            | Self::Storage(StorageError::CaptureNotFound(_))
            | Self::Storage(StorageError::DataDownloadNotFound(_)) => StatusCode::NOT_FOUND,
            Self::Storage(StorageError::DataDownloadAlreadyExists { .. }) => StatusCode::CONFLICT,
            Self::Storage(StorageError::DataDownloadAlreadyRunning(_)) => StatusCode::CONFLICT,
            Self::Storage(StorageError::DataDownloadStartDateImmutable)
            | Self::Archive(ArchiveDownloadError::AlreadyRunning) => StatusCode::CONFLICT,
            Self::Storage(StorageError::DataDownloadStartDateRequired)
            | Self::Archive(ArchiveDownloadError::UnsupportedEntry)
            | Self::Archive(ArchiveDownloadError::InvalidStartDate) => StatusCode::BAD_REQUEST,
            Self::Storage(_) | Self::Archive(_) | Self::Task(_) => StatusCode::BAD_GATEWAY,
        };
        (status, self.to_string()).into_response()
    }
}
