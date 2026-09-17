use crate::market::{MarketError, MarketKey, MarketService, MarketType};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
struct MarketQuery {
    symbol: String,
    interval: String,
    market_type: Option<String>,
}

pub fn router(market_service: Arc<MarketService>) -> Router {
    Router::new()
        .route("/api/market/candles", get(candles))
        .with_state(market_service)
}

async fn candles(
    State(market_service): State<Arc<MarketService>>,
    Query(query): Query<MarketQuery>,
) -> Result<Json<crate::market::MarketSnapshot>, ApiError> {
    let market_type = MarketType::parse(query.market_type.as_deref().unwrap_or("spot"))?;
    let key = MarketKey::new(&query.symbol, &query.interval, market_type)?;
    Ok(Json(market_service.snapshot_for(key).await?))
}

struct ApiError(MarketError);

impl From<MarketError> for ApiError {
    fn from(error: MarketError) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            MarketError::InvalidSelection(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::BAD_GATEWAY,
        };
        (status, self.0.to_string()).into_response()
    }
}
