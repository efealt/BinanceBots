pub mod data;
pub mod market;

use crate::market::MarketService;
use crate::storage::StorageReader;
use axum::{Json, Router, routing::get};
use serde::Serialize;
use std::sync::Arc;

pub fn router(market_service: Arc<MarketService>, storage_reader: Arc<StorageReader>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .merge(market::router(market_service))
        .merge(data::router(storage_reader))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}
