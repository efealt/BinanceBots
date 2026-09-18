use crate::storage::{
    CaptureInspection, CaptureSummary, DataDownload, DataDownloadSpec, DatasetInspection,
    DatasetSummary, StorageError, StorageReader,
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

const DEFAULT_PAGE_SIZE: i64 = 100;
const MAX_PAGE_SIZE: i64 = 500;

pub fn router(storage_reader: Arc<StorageReader>) -> Router {
    Router::new()
        .route("/api/data/datasets", get(datasets))
        .route("/api/data/downloads", get(downloads).post(create_download))
        .route("/api/data/captures", get(captures))
        .route("/api/data/inspection", get(inspection))
        .route("/api/data/capture-inspection", get(capture_inspection))
        .with_state(storage_reader)
}

async fn downloads(
    State(storage_reader): State<Arc<StorageReader>>,
) -> Result<Json<DownloadCatalog>, DataApiError> {
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
}

async fn create_download(
    State(storage_reader): State<Arc<StorageReader>>,
    Json(request): Json<CreateDownloadRequest>,
) -> Result<Json<DataDownload>, DataApiError> {
    let spec = normalize_download_request(request)?;
    let download =
        tokio::task::spawn_blocking(move || storage_reader.create_data_download(&spec)).await??;
    Ok(Json(download))
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
    if !matches!(
        interval.as_str(),
        "1m" | "3m"
            | "5m"
            | "15m"
            | "30m"
            | "1h"
            | "2h"
            | "4h"
            | "6h"
            | "8h"
            | "12h"
            | "1d"
            | "3d"
            | "1w"
    ) {
        return Err(DataApiError::InvalidQuery(
            "interval is not a supported Binance kline interval".into(),
        ));
    }

    let name = request.name.trim().to_string();
    if name.is_empty() {
        return Err(DataApiError::InvalidQuery("name is required".into()));
    }

    Ok(DataDownloadSpec {
        provider,
        symbol,
        market_type,
        name,
        interval,
    })
}

async fn datasets(
    State(storage_reader): State<Arc<StorageReader>>,
) -> Result<Json<DatasetCatalog>, DataApiError> {
    let datasets = tokio::task::spawn_blocking(move || storage_reader.catalog()).await??;
    Ok(Json(DatasetCatalog { datasets }))
}

#[derive(Deserialize)]
struct InspectionQuery {
    dataset_id: Option<i64>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn inspection(
    State(storage_reader): State<Arc<StorageReader>>,
    Query(query): Query<InspectionQuery>,
) -> Result<Json<DatasetInspection>, DataApiError> {
    let dataset_id = query
        .dataset_id
        .filter(|dataset_id| *dataset_id > 0)
        .ok_or_else(|| DataApiError::InvalidQuery("dataset_id is required".into()))?;
    let (page, page_size) = page_request(query.page, query.page_size);
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
    State(storage_reader): State<Arc<StorageReader>>,
) -> Result<Json<CaptureCatalog>, DataApiError> {
    let captures = tokio::task::spawn_blocking(move || storage_reader.captures()).await??;
    Ok(Json(CaptureCatalog { captures }))
}

#[derive(Serialize)]
struct CaptureCatalog {
    captures: Vec<CaptureSummary>,
}

async fn capture_inspection(
    State(storage_reader): State<Arc<StorageReader>>,
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
    #[error("data inspection task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

impl IntoResponse for DataApiError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::InvalidQuery(_) => StatusCode::BAD_REQUEST,
            Self::Storage(StorageError::DatasetNotFound(_))
            | Self::Storage(StorageError::CaptureNotFound(_)) => StatusCode::NOT_FOUND,
            Self::Storage(StorageError::DataDownloadAlreadyExists { .. }) => StatusCode::CONFLICT,
            Self::Storage(_) | Self::Task(_) => StatusCode::BAD_GATEWAY,
        };
        (status, self.to_string()).into_response()
    }
}
