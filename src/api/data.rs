use crate::downloader::{
    ArchiveDownloadError, ArchiveDownloadProgress, ArchiveDownloadResult, ArchiveDownloader,
};
use crate::storage::{
    DataDownload, DataDownloadSpec, DatasetSummary, OhlcvCandle, StorageError, StorageReader,
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
        .route(
            "/api/data/downloads/{download_id}/progress",
            get(download_progress),
        )
        .route("/api/data/downloads/{download_id}", patch(update_download))
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

async fn download_progress(
    State(state): State<Arc<DataApiState>>,
    Path(download_id): Path<i64>,
) -> Result<Json<Option<ArchiveDownloadProgress>>, DataApiError> {
    if download_id <= 0 {
        return Err(DataApiError::InvalidQuery("download id is required".into()));
    }
    Ok(Json(state.archive_downloader.progress(download_id).await))
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
    candles: Vec<OhlcvCandle>,
}

#[derive(Serialize)]
struct DatasetCatalog {
    datasets: Vec<DatasetSummary>,
}

#[derive(Debug, Error)]
enum DataApiError {
    #[error("invalid data query: {0}")]
    InvalidQuery(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Archive(#[from] ArchiveDownloadError),
    #[error("data API task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

impl IntoResponse for DataApiError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::InvalidQuery(_) => StatusCode::BAD_REQUEST,
            Self::Storage(StorageError::DatasetNotFound(_))
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
