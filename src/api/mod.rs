pub mod backtests;
pub mod data;
pub mod market;
pub mod security;

use crate::market::MarketService;
use crate::storage::StorageReader;
use axum::{Json, Router};
use serde::Serialize;
use std::sync::Arc;

pub fn protected_router(
    market_service: Arc<MarketService>,
    storage_reader: Arc<StorageReader>,
) -> Router {
    Router::new()
        .merge(market::router(market_service))
        .merge(data::router(Arc::clone(&storage_reader)))
        .merge(backtests::router(Arc::clone(&storage_reader)))
        .merge(security::router(storage_reader))
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
}
