pub mod market;

use crate::market::MarketService;
use axum::{Json, Router, routing::get};
use serde::Serialize;
use std::sync::Arc;

pub fn router(market_service: Arc<MarketService>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .merge(market::router(market_service))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}
