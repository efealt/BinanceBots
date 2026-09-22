use crate::{
    market::MarketType,
    paper::{PaperError, PaperManager, PaperSnapshot, PaperStartConfig},
    storage::{ExactDecimal, TimeInForce},
    trading::{ExecutionAssumptions, GridAnchor, StaticGridConfig, TradingInterval},
};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Deserialize)]
struct GridRequest {
    anchor: String,
    fixed_anchor_price: Option<f64>,
    spacing_bps: f64,
    levels_per_side: u32,
    quantity_per_order: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct StartTradingRunRequest {
    mode: String,
    symbol: String,
    market_type: String,
    replay_interval: String,
    initial_capital: String,
    strategy_id: String,
    grid: GridRequest,
    execution: ExecutionAssumptions,
}

#[derive(Clone, Debug, Deserialize)]
struct RunsQuery {
    limit: Option<usize>,
}

#[derive(Serialize)]
struct RunsResponse<T> {
    runs: Vec<T>,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum TradingStreamMessage {
    Snapshot(PaperSnapshot),
}

pub fn router(manager: Arc<PaperManager>) -> Router {
    Router::new()
        .route("/api/trading/runs", post(start_run).get(list_runs))
        .route("/api/trading/runs/{run_id}", get(run_snapshot))
        .route("/api/trading/runs/{run_id}/stop", post(stop_run))
        .route("/api/trading/runs/{run_id}/stream", get(run_stream))
        .with_state(manager)
}

async fn start_run(
    State(manager): State<Arc<PaperManager>>,
    Json(request): Json<StartTradingRunRequest>,
) -> Result<(StatusCode, Json<PaperSnapshot>), TradingApiError> {
    match request.mode.trim().to_ascii_lowercase().as_str() {
        "paper" => {}
        "live" => return Err(TradingApiError::LiveLocked),
        _ => return Err(TradingApiError::Invalid("mode must be paper or live".into())),
    }

    let market_type = MarketType::parse(request.market_type.trim())?;
    let replay_interval = TradingInterval::parse(request.replay_interval.trim())
        .ok_or_else(|| TradingApiError::Invalid("replay_interval must be 1m, 1h, or 1d".into()))?;
    let initial_capital = ExactDecimal::new(request.initial_capital.trim())?;
    let anchor = match request.grid.anchor.trim() {
        "previous_close" => GridAnchor::PreviousClose,
        "fixed" => GridAnchor::Fixed,
        _ => {
            return Err(TradingApiError::Invalid(
                "grid anchor must be previous_close or fixed".into(),
            ));
        }
    };

    let grid_config = StaticGridConfig {
        anchor,
        fixed_anchor_price: request.grid.fixed_anchor_price,
        spacing_bps: request.grid.spacing_bps,
        levels_per_side: request.grid.levels_per_side,
        quantity_per_order: request.grid.quantity_per_order,
        time_in_force: TimeInForce::Gtc,
    };
    grid_config.validate().map_err(TradingApiError::Invalid)?;
    request.execution.validate().map_err(TradingApiError::Invalid)?;

    let snapshot = manager
        .start(PaperStartConfig {
            symbol: request.symbol,
            market_type,
            replay_interval,
            initial_capital,
            strategy_id: request.strategy_id,
            grid_config,
            execution: request.execution,
        })
        .await?;

    Ok((StatusCode::CREATED, Json(snapshot)))
}

async fn stop_run(
    State(manager): State<Arc<PaperManager>>,
    Path(run_id): Path<i64>,
) -> Result<Json<PaperSnapshot>, TradingApiError> {
    validate_run_id(run_id)?;
    Ok(Json(manager.stop(run_id).await?))
}

async fn run_snapshot(
    State(manager): State<Arc<PaperManager>>,
    Path(run_id): Path<i64>,
) -> Result<Json<PaperSnapshot>, TradingApiError> {
    validate_run_id(run_id)?;
    Ok(Json(manager.snapshot(run_id).await?))
}

async fn list_runs(
    State(manager): State<Arc<PaperManager>>,
    Query(query): Query<RunsQuery>,
) -> Result<Json<RunsResponse<crate::paper::PaperRunSummary>>, TradingApiError> {
    let limit = query.limit.unwrap_or(25).clamp(1, 100);
    Ok(Json(RunsResponse {
        runs: manager.list_runs(limit).await?,
    }))
}

async fn run_stream(
    websocket: WebSocketUpgrade,
    State(manager): State<Arc<PaperManager>>,
    Path(run_id): Path<i64>,
) -> Result<Response, TradingApiError> {
    validate_run_id(run_id)?;
    let initial = manager.snapshot(run_id).await?;
    let receiver = manager.subscribe(run_id).await?;
    Ok(websocket
        .on_upgrade(move |socket| stream_runtime(socket, initial, receiver))
        .into_response())
}

async fn stream_runtime(
    socket: WebSocket,
    initial: PaperSnapshot,
    mut updates: Option<tokio::sync::broadcast::Receiver<PaperSnapshot>>,
) {
    let (mut sender, mut receiver) = socket.split();
    let payload = match serde_json::to_string(&TradingStreamMessage::Snapshot(initial)) {
        Ok(payload) => payload,
        Err(_) => return,
    };
    if sender.send(Message::Text(payload.into())).await.is_err() {
        return;
    }

    let Some(mut updates) = updates.take() else {
        let _ = sender.send(Message::Close(None)).await;
        return;
    };

    loop {
        tokio::select! {
            client = receiver.next() => {
                match client {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            update = updates.recv() => {
                let snapshot = match update {
                    Ok(snapshot) => snapshot,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                let payload = match serde_json::to_string(&TradingStreamMessage::Snapshot(snapshot)) {
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

fn validate_run_id(run_id: i64) -> Result<(), TradingApiError> {
    if run_id <= 0 {
        return Err(TradingApiError::Invalid("run_id must be positive".into()));
    }
    Ok(())
}

#[derive(Debug)]
enum TradingApiError {
    Invalid(String),
    LiveLocked,
    Paper(PaperError),
    Market(crate::market::MarketError),
    Storage(crate::storage::StorageError),
}

impl From<PaperError> for TradingApiError {
    fn from(error: PaperError) -> Self {
        Self::Paper(error)
    }
}

impl From<crate::market::MarketError> for TradingApiError {
    fn from(error: crate::market::MarketError) -> Self {
        Self::Market(error)
    }
}

impl From<crate::storage::StorageError> for TradingApiError {
    fn from(error: crate::storage::StorageError) -> Self {
        Self::Storage(error)
    }
}

impl IntoResponse for TradingApiError {
    fn into_response(self) -> Response {
        let message = self.to_string();
        let status = match &self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::LiveLocked => StatusCode::LOCKED,
            Self::Paper(PaperError::RunNotFound(_)) | Self::Paper(PaperError::RunNotActive(_)) => {
                StatusCode::NOT_FOUND
            }
            Self::Paper(PaperError::Busy(_)) => StatusCode::CONFLICT,
            Self::Paper(PaperError::Invalid(_)) => StatusCode::BAD_REQUEST,
            Self::Paper(PaperError::Market(_)) | Self::Market(_) => StatusCode::BAD_GATEWAY,
            Self::Paper(_) | Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = if matches!(self, Self::LiveLocked) {
            "Live execution is locked until Phase 8; Phase 4 supports Paper only.".to_string()
        } else {
            message
        };
        (status, body).into_response()
    }
}

impl std::fmt::Display for TradingApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::LiveLocked => formatter.write_str("Live mode is locked"),
            Self::Paper(error) => error.fmt(formatter),
            Self::Market(error) => error.fmt(formatter),
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}
