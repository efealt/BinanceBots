use crate::{
    market::MarketType,
    paper::{PaperChartSnapshot, PaperError, PaperManager, PaperSnapshot, PaperStartConfig},
    storage::{ExactDecimal, StorageError, StorageReader, TimeInForce, TradingBot, TradingBotSpec},
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

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GridRequest {
    anchor: String,
    fixed_anchor_price: Option<f64>,
    spacing_bps: f64,
    levels_per_side: u32,
    quantity_per_order: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BotConfigurationRequest {
    symbol: String,
    market_type: String,
    replay_interval: String,
    initial_capital: String,
    strategy_id: String,
    grid: GridRequest,
    execution: ExecutionAssumptions,
}

#[derive(Clone, Debug, Deserialize)]
struct StartTradingRunRequest {
    mode: String,
    bot_id: i64,
    symbol: String,
    market_type: String,
    replay_interval: String,
    initial_capital: String,
    strategy_id: String,
    grid: GridRequest,
    execution: ExecutionAssumptions,
}

#[derive(Clone, Debug, Deserialize)]
struct SaveTradingBotRequest {
    bot_name: String,
    configuration: BotConfigurationRequest,
}

#[derive(Clone)]
struct TradingApiState {
    manager: Arc<PaperManager>,
    storage: Arc<StorageReader>,
}

struct ValidatedConfiguration {
    stored: BotConfigurationRequest,
    market_type: MarketType,
    replay_interval: TradingInterval,
    initial_capital: ExactDecimal,
    grid_config: StaticGridConfig,
    execution: ExecutionAssumptions,
}

#[derive(Clone, Debug, Deserialize)]
struct RunsQuery {
    limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
struct AuditQuery {
    after_sequence: Option<i64>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct RunsResponse<T> {
    runs: Vec<T>,
}

#[derive(Serialize)]
struct BotsResponse {
    bots: Vec<TradingBot>,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum TradingStreamMessage {
    Snapshot(PaperSnapshot),
    Update(PaperSnapshot),
}

pub fn router(manager: Arc<PaperManager>, storage: Arc<StorageReader>) -> Router {
    Router::new()
        .route("/api/trading/bots", post(create_bot).get(list_bots))
        .route("/api/trading/bots/{bot_id}", get(bot_snapshot).put(update_bot))
        .route("/api/trading/runs", post(start_run).get(list_runs))
        .route("/api/trading/runs/{run_id}", get(run_snapshot))
        .route("/api/trading/runs/{run_id}/chart", get(run_chart))
        .route("/api/trading/runs/{run_id}/audit", get(run_audit))
        .route("/api/trading/runs/{run_id}/stop", post(stop_run))
        .route("/api/trading/runs/{run_id}/stream", get(run_stream))
        .with_state(TradingApiState { manager, storage })
}

async fn create_bot(
    State(state): State<TradingApiState>,
    Json(request): Json<SaveTradingBotRequest>,
) -> Result<(StatusCode, Json<TradingBot>), TradingApiError> {
    let validated = validate_configuration(request.configuration)?;
    let bot = state.storage.create_trading_bot(&TradingBotSpec {
        bot_name: request.bot_name,
        config: configuration_json(&validated.stored)?,
    })?;
    Ok((StatusCode::CREATED, Json(bot)))
}

async fn update_bot(
    State(state): State<TradingApiState>,
    Path(bot_id): Path<i64>,
    Json(request): Json<SaveTradingBotRequest>,
) -> Result<Json<TradingBot>, TradingApiError> {
    validate_bot_id(bot_id)?;
    let validated = validate_configuration(request.configuration)?;
    Ok(Json(state.storage.update_trading_bot(
        bot_id,
        &TradingBotSpec {
            bot_name: request.bot_name,
            config: configuration_json(&validated.stored)?,
        },
    )?))
}

async fn bot_snapshot(
    State(state): State<TradingApiState>,
    Path(bot_id): Path<i64>,
) -> Result<Json<TradingBot>, TradingApiError> {
    validate_bot_id(bot_id)?;
    Ok(Json(state.storage.trading_bot(bot_id)?))
}

async fn list_bots(
    State(state): State<TradingApiState>,
) -> Result<Json<BotsResponse>, TradingApiError> {
    Ok(Json(BotsResponse {
        bots: state.storage.trading_bots()?,
    }))
}

async fn start_run(
    State(state): State<TradingApiState>,
    Json(request): Json<StartTradingRunRequest>,
) -> Result<(StatusCode, Json<PaperSnapshot>), TradingApiError> {
    require_phase4_paper_mode(&request.mode)?;

    let validated = validate_configuration(BotConfigurationRequest {
        symbol: request.symbol,
        market_type: request.market_type,
        replay_interval: request.replay_interval,
        initial_capital: request.initial_capital,
        strategy_id: request.strategy_id,
        grid: request.grid,
        execution: request.execution,
    })?;
    let config_json = configuration_json(&validated.stored)?;

    validate_bot_id(request.bot_id)?;
    let bot = state.storage.trading_bot(request.bot_id)?;
    if bot.config != config_json {
        return Err(TradingApiError::Invalid(
            "Live-Paper configuration differs from the saved bot; save the bot before starting it".into(),
        ));
    }
    let bot_id = request.bot_id;

    let snapshot = state.manager
        .start(PaperStartConfig {
            bot_id,
            symbol: validated.stored.symbol,
            market_type: validated.market_type,
            replay_interval: validated.replay_interval,
            initial_capital: validated.initial_capital,
            strategy_id: validated.stored.strategy_id,
            grid_config: validated.grid_config,
            execution: validated.execution,
        })
        .await?;

    Ok((StatusCode::CREATED, Json(snapshot)))
}

fn validate_configuration(
    mut request: BotConfigurationRequest,
) -> Result<ValidatedConfiguration, TradingApiError> {
    request.symbol = request.symbol.trim().to_ascii_uppercase();
    if request.symbol.is_empty() {
        return Err(TradingApiError::Invalid("symbol is required".into()));
    }

    let market_type = MarketType::parse(request.market_type.trim())?;
    request.market_type = market_type.as_str().into();

    let replay_interval = TradingInterval::parse(request.replay_interval.trim())
        .ok_or_else(|| TradingApiError::Invalid("replay_interval must be 1m, 1h, or 1d".into()))?;
    request.replay_interval = replay_interval.as_str().into();

    let initial_capital = ExactDecimal::new(request.initial_capital.trim())?;
    request.initial_capital = initial_capital.as_str().into();

    request.strategy_id = request.strategy_id.trim().to_string();
    if request.strategy_id != "static-grid-fixture" {
        return Err(TradingApiError::Invalid(
            "Live-Paper currently exposes only static-grid-fixture".into(),
        ));
    }

    request.grid.anchor = request.grid.anchor.trim().to_ascii_lowercase();
    let anchor = match request.grid.anchor.as_str() {
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

    Ok(ValidatedConfiguration {
        execution: request.execution.clone(),
        stored: request,
        market_type,
        replay_interval,
        initial_capital,
        grid_config,
    })
}

fn configuration_json(
    configuration: &BotConfigurationRequest,
) -> Result<serde_json::Value, TradingApiError> {
    serde_json::to_value(configuration).map_err(|error| TradingApiError::Invalid(error.to_string()))
}

async fn stop_run(
    State(state): State<TradingApiState>,
    Path(run_id): Path<i64>,
) -> Result<Json<PaperSnapshot>, TradingApiError> {
    validate_run_id(run_id)?;
    Ok(Json(state.manager.stop(run_id).await?))
}

async fn run_snapshot(
    State(state): State<TradingApiState>,
    Path(run_id): Path<i64>,
) -> Result<Json<PaperSnapshot>, TradingApiError> {
    validate_run_id(run_id)?;
    Ok(Json(state.manager.snapshot(run_id).await?))
}

async fn run_chart(
    State(state): State<TradingApiState>,
    Path(run_id): Path<i64>,
) -> Result<Json<PaperChartSnapshot>, TradingApiError> {
    validate_run_id(run_id)?;
    Ok(Json(state.manager.chart_snapshot(run_id).await?))
}

async fn run_audit(
    State(state): State<TradingApiState>,
    Path(run_id): Path<i64>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<crate::storage::TradingAuditPage>, TradingApiError> {
    validate_run_id(run_id)?;
    if query.after_sequence.is_some_and(|value| value < 0) {
        return Err(TradingApiError::Invalid(
            "after_sequence must be zero or positive".into(),
        ));
    }
    let limit = query.limit.unwrap_or(250).clamp(1, 500);
    Ok(Json(state.manager.audit_page(run_id, query.after_sequence, limit)?))
}

async fn list_runs(
    State(state): State<TradingApiState>,
    Query(query): Query<RunsQuery>,
) -> Result<Json<RunsResponse<crate::paper::PaperRunSummary>>, TradingApiError> {
    let limit = query.limit.unwrap_or(25).clamp(1, 100);
    Ok(Json(RunsResponse {
        runs: state.manager.list_runs(limit).await?,
    }))
}

async fn run_stream(
    websocket: WebSocketUpgrade,
    State(state): State<TradingApiState>,
    Path(run_id): Path<i64>,
) -> Result<Response, TradingApiError> {
    validate_run_id(run_id)?;
    let (initial, receiver) = state.manager.stream_bootstrap(run_id).await?;
    let stream_manager = Arc::clone(&state.manager);
    Ok(websocket
        .on_upgrade(move |socket| {
            stream_runtime(socket, run_id, stream_manager, initial, receiver)
        })
        .into_response())
}

async fn stream_runtime(
    socket: WebSocket,
    run_id: i64,
    manager: Arc<PaperManager>,
    initial: PaperSnapshot,
    mut updates: Option<tokio::sync::broadcast::Receiver<PaperSnapshot>>,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut last_revision = initial.stream_revision;
    if send_stream_message(&mut sender, TradingStreamMessage::Snapshot(initial))
        .await
        .is_err()
    {
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
                match update {
                    Ok(snapshot) => {
                        if !is_newer_revision(last_revision, snapshot.stream_revision) {
                            continue;
                        }
                        last_revision = snapshot.stream_revision;
                        if send_stream_message(&mut sender, TradingStreamMessage::Update(snapshot))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let snapshot = match manager.snapshot(run_id).await {
                            Ok(snapshot) => snapshot,
                            Err(_) => break,
                        };
                        if !is_newer_revision(last_revision, snapshot.stream_revision) {
                            continue;
                        }
                        last_revision = snapshot.stream_revision;
                        if send_stream_message(&mut sender, TradingStreamMessage::Snapshot(snapshot))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn send_stream_message(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: TradingStreamMessage,
) -> Result<(), ()> {
    let payload = serde_json::to_string(&message).map_err(|_| ())?;
    sender
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ())
}

fn is_newer_revision(last_revision: u64, candidate_revision: u64) -> bool {
    candidate_revision > last_revision
}

fn require_phase4_paper_mode(mode: &str) -> Result<(), TradingApiError> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "paper" => Ok(()),
        "live" => Err(TradingApiError::LiveLocked),
        _ => Err(TradingApiError::Invalid(
            "mode must be paper (Live-Paper) or live (Live-Real-Account)".into(),
        )),
    }
}

fn validate_bot_id(bot_id: i64) -> Result<(), TradingApiError> {
    if bot_id <= 0 {
        return Err(TradingApiError::Invalid("bot_id must be positive".into()));
    }
    Ok(())
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
            Self::Paper(PaperError::RunNotFound(_))
            | Self::Paper(PaperError::RunNotActive(_))
            | Self::Storage(StorageError::TradingBotNotFound(_)) => StatusCode::NOT_FOUND,
            Self::Paper(PaperError::Busy(_)) => StatusCode::CONFLICT,
            Self::Paper(PaperError::Invalid(_)) => StatusCode::BAD_REQUEST,
            Self::Paper(PaperError::Market(_)) | Self::Market(_) => StatusCode::BAD_GATEWAY,
            Self::Paper(_) | Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = if matches!(self, Self::LiveLocked) {
            "Live-Real-Account execution is locked; Live-Paper is the current forward execution mode.".to_string()
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
            Self::LiveLocked => formatter.write_str("Live-Real-Account execution is locked"),
            Self::Paper(error) => error.fmt(formatter),
            Self::Market(error) => error.fmt(formatter),
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{market::MarketService, storage::StorageReader};
    use std::sync::Arc;

    fn temp_database(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "binance-grid-trading-api-{label}-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ))
    }

    fn cleanup_database(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    #[test]
    fn stream_revision_gate_drops_bootstrap_duplicates_and_accepts_new_state() {
        assert!(!is_newer_revision(7, 7));
        assert!(!is_newer_revision(7, 6));
        assert!(is_newer_revision(7, 8));
        assert!(is_newer_revision(7, 10));
    }

    #[test]
    fn phase4_mode_gate_accepts_paper_and_rejects_live() {
        assert!(require_phase4_paper_mode("paper").is_ok());
        assert!(require_phase4_paper_mode(" PAPER ").is_ok());
        assert!(matches!(
            require_phase4_paper_mode("live"),
            Err(TradingApiError::LiveLocked)
        ));
        assert!(matches!(
            require_phase4_paper_mode(" LIVE "),
            Err(TradingApiError::LiveLocked)
        ));
        assert!(matches!(
            require_phase4_paper_mode("backtest"),
            Err(TradingApiError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn phase4_verification_live_start_is_rejected_before_any_validation_or_runtime_access() {
        let path = temp_database("live-lock");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let manager = PaperManager::new(
            Arc::clone(&storage),
            Arc::new(MarketService::new()),
        )
        .unwrap();

        let request = StartTradingRunRequest {
            mode: "live".into(),
            bot_id: 1,
            symbol: "THIS_DOES_NOT_NEED_TO_EXIST".into(),
            market_type: "definitely_invalid".into(),
            replay_interval: "definitely_invalid".into(),
            initial_capital: "definitely_invalid".into(),
            strategy_id: "definitely_invalid".into(),
            grid: GridRequest {
                anchor: "definitely_invalid".into(),
                fixed_anchor_price: Some(-1.0),
                spacing_bps: -1.0,
                levels_per_side: 0,
                quantity_per_order: -1.0,
            },
            execution: ExecutionAssumptions {
                fee_bps: -1.0,
                ..ExecutionAssumptions::default()
            },
        };

        let state = TradingApiState {
            manager,
            storage: Arc::clone(&storage),
        };
        let error = start_run(State(state), Json(request))
            .await
            .err()
            .expect("Live start must be server-side locked");
        assert!(matches!(error, TradingApiError::LiveLocked));
        assert!(storage.trading_runs_by_mode(crate::storage::RunMode::Live, 10).unwrap().is_empty());
        assert!(storage.trading_runs_by_mode(crate::storage::RunMode::Paper, 10).unwrap().is_empty());

        drop(storage);
        cleanup_database(&path);
    }

    fn sample_bot_configuration(spacing_bps: f64) -> BotConfigurationRequest {
        BotConfigurationRequest {
            symbol: "BTCUSDT".into(),
            market_type: "spot".into(),
            replay_interval: "1m".into(),
            initial_capital: "1000.00".into(),
            strategy_id: "static-grid-fixture".into(),
            grid: GridRequest {
                anchor: "previous_close".into(),
                fixed_anchor_price: None,
                spacing_bps,
                levels_per_side: 3,
                quantity_per_order: 1.0,
            },
            execution: ExecutionAssumptions::default(),
        }
    }

    #[tokio::test]
    async fn bot_api_saves_and_updates_without_creating_a_run() {
        let path = temp_database("bot-save");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let manager = PaperManager::new(
            Arc::clone(&storage),
            Arc::new(MarketService::new()),
        ).unwrap();
        let state = TradingApiState {
            manager,
            storage: Arc::clone(&storage),
        };

        let (status, Json(bot)) = create_bot(
            State(state.clone()),
            Json(SaveTradingBotRequest {
                bot_name: "BTC Grid".into(),
                configuration: sample_bot_configuration(25.0),
            }),
        ).await.unwrap();

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(bot.bot_name, "BTC Grid");
        assert!(storage.trading_runs_by_mode(crate::storage::RunMode::Paper, 10).unwrap().is_empty());

        let Json(updated) = update_bot(
            State(state.clone()),
            Path(bot.bot_id),
            Json(SaveTradingBotRequest {
                bot_name: "BTC Grid Revised".into(),
                configuration: sample_bot_configuration(40.0),
            }),
        ).await.unwrap();

        assert_eq!(updated.bot_id, bot.bot_id);
        assert_eq!(updated.bot_name, "BTC Grid Revised");
        assert_eq!(updated.config["grid"]["spacing_bps"], 40.0);

        let Json(listed) = list_bots(State(state)).await.unwrap();
        assert_eq!(listed.bots.len(), 1);
        assert_eq!(listed.bots[0].bot_id, bot.bot_id);

        drop(storage);
        cleanup_database(&path);
    }

}
