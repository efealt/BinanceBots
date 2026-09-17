use crate::market::{
    MarketError, MarketKey, MarketService, MarketSnapshot, MarketType, MarketUpdate,
};
use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tokio::time::{Duration, MissedTickBehavior};

#[derive(Deserialize)]
struct MarketQuery {
    symbol: String,
    interval: String,
    market_type: Option<String>,
}

pub fn router(market_service: Arc<MarketService>) -> Router {
    Router::new()
        .route("/api/market/candles", get(candles))
        .route("/api/market/stream", get(stream))
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

async fn stream(
    websocket: WebSocketUpgrade,
    State(market_service): State<Arc<MarketService>>,
    Query(query): Query<MarketQuery>,
) -> Result<Response, ApiError> {
    let market_type = MarketType::parse(query.market_type.as_deref().unwrap_or("spot"))?;
    let key = MarketKey::new(&query.symbol, &query.interval, market_type)?;
    let initial_snapshot = market_service.snapshot_for(key.clone()).await?;

    Ok(websocket
        .on_upgrade(move |socket| run_stream(socket, market_service, key, initial_snapshot))
        .into_response())
}

#[derive(Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum StreamMessage {
    Snapshot(MarketSnapshot),
    Update(MarketUpdate),
}

async fn run_stream(
    socket: WebSocket,
    market_service: Arc<MarketService>,
    key: MarketKey,
    initial_snapshot: MarketSnapshot,
) {
    let (mut sender, mut receiver) = socket.split();
    let initial_payload = match serde_json::to_string(&StreamMessage::Snapshot(initial_snapshot)) {
        Ok(payload) => payload,
        Err(_) => return,
    };
    if sender
        .send(Message::Text(initial_payload.into()))
        .await
        .is_err()
    {
        return;
    }

    let mut ticker = tokio::time::interval(Duration::from_millis(200));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            message = receiver.next() => {
                match message {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            _ = ticker.tick() => {
                let update = match market_service.update_for(&key).await {
                    Ok(update) => update,
                    Err(_) => break,
                };
                let payload = match serde_json::to_string(&StreamMessage::Update(update)) {
                    Ok(payload) => payload,
                    Err(_) => break,
                };
                if sender.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
        }
    }
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
