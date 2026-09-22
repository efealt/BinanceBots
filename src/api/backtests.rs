use crate::{
    backtest::{BacktestEngine, BacktestRunConfig, ReplayInterval},
    storage::{ExactDecimal, StorageError, StorageReader, TimeInForce},
    trading::{
        ExecutionAssumptions, GridAnchor, LimitFillPolicy, StaticGridConfig, StaticGridStrategy,
    },
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use thiserror::Error;

#[derive(Clone)]
struct BacktestApiState {
    storage_reader: Arc<StorageReader>,
    jobs: Arc<Mutex<HashMap<u64, BacktestJobResponse>>>,
    next_job_id: Arc<AtomicU64>,
}

pub fn router(storage_reader: Arc<StorageReader>) -> Router {
    Router::new()
        .route("/api/backtests/jobs", post(create_job))
        .route("/api/backtests/jobs/{job_id}", get(job_status))
        .route("/api/backtests/runs/{run_id}", get(run_result))
        .route("/api/backtests/runs/{run_id}/analysis", get(run_analysis))
        .with_state(Arc::new(BacktestApiState {
            storage_reader,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            next_job_id: Arc::new(AtomicU64::new(1)),
        }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StaticGridRequest {
    anchor: String,
    fixed_anchor_price: Option<f64>,
    spacing_bps: f64,
    levels_per_side: u32,
    quantity_per_order: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct CreateBacktestJobRequest {
    dataset_id: i64,
    replay_interval: String,
    start_time_ms: i64,
    end_time_ms: i64,
    initial_capital: String,
    strategy_id: String,
    grid: StaticGridRequest,
    execution: ExecutionAssumptions,
}

#[derive(Clone, Debug)]
struct ValidatedBacktestRequest {
    dataset_id: i64,
    replay_interval: ReplayInterval,
    start_time_ms: i64,
    end_time_ms: i64,
    initial_capital: ExactDecimal,
    grid_config: StaticGridConfig,
    execution: ExecutionAssumptions,
}

#[derive(Clone, Debug, Serialize)]
struct BacktestJobResponse {
    job_id: u64,
    status: String,
    progress_percent: u8,
    message: String,
    run_id: Option<i64>,
    result: Option<BacktestRunView>,
}

#[derive(Clone, Debug, Serialize)]
struct BacktestAnalysisView {
    run_id: i64,
    equity: Vec<crate::storage::TradingEquityPoint>,
    positions: Vec<crate::storage::TradingPositionPoint>,
    order_levels: Vec<crate::storage::TradingOrderLevel>,
}

#[derive(Clone, Debug, Serialize)]
struct BacktestRunView {
    run_id: i64,
    status: String,
    strategy_id: String,
    strategy_version: String,
    strategy_params: Value,
    execution_assumptions: Value,
    dataset_id: i64,
    symbol: String,
    market_type: String,
    source_interval: String,
    replay_interval: String,
    requested_start_time_ms: Option<i64>,
    requested_end_time_ms: Option<i64>,
    effective_start_time_ms: Option<i64>,
    effective_end_time_ms: Option<i64>,
    reserved_first_candle_as_preroll: bool,
    candles_processed: i64,
    initial_capital: String,
    final_equity: Option<String>,
    total_return_percent: Option<f64>,
    max_drawdown_percent: f64,
    cash_balance: Option<String>,
    realized_pnl: Option<String>,
    unrealized_pnl: Option<String>,
    fees_paid: Option<String>,
    final_position_quantity: String,
    order_count: i64,
    fill_count: i64,
    fills: Vec<crate::storage::TradingFillAudit>,
}

async fn create_job(
    State(state): State<Arc<BacktestApiState>>,
    Json(request): Json<CreateBacktestJobRequest>,
) -> Result<(StatusCode, Json<BacktestJobResponse>), BacktestApiError> {
    let validated = validate_request(request)?;
    let storage_reader = Arc::clone(&state.storage_reader);
    let dataset_id = validated.dataset_id;
    tokio::task::spawn_blocking(move || storage_reader.historical_dataset_info(dataset_id)).await??;

    let job_id = state.next_job_id.fetch_add(1, Ordering::Relaxed);
    let job = BacktestJobResponse {
        job_id,
        status: "queued".into(),
        progress_percent: 0,
        message: "Queued on Render backend".into(),
        run_id: None,
        result: None,
    };

    {
        let mut jobs = state
            .jobs
            .lock()
            .map_err(|_| BacktestApiError::Internal("backtest job state is unavailable".into()))?;
        if jobs
            .values()
            .any(|job| matches!(job.status.as_str(), "queued" | "running"))
        {
            return Err(BacktestApiError::Busy);
        }
        jobs.insert(job_id, job.clone());
    }

    let background_state = Arc::clone(&state);
    tokio::spawn(async move {
        update_job(&background_state, job_id, |job| {
            job.status = "running".into();
            job.message = "Running historical replay on Render".into();
        });

        let worker_state = Arc::clone(&background_state);
        let worker = tokio::task::spawn_blocking(move || execute_job(worker_state, job_id, validated)).await;

        match worker {
            Ok(Ok(result)) => {
                update_job(&background_state, job_id, |job| {
                    job.status = "completed".into();
                    job.progress_percent = 100;
                    job.message = "Backtest completed".into();
                    job.run_id = Some(result.run_id);
                    job.result = Some(result);
                });
            }
            Ok(Err(error)) => {
                update_job(&background_state, job_id, |job| {
                    job.status = "failed".into();
                    job.message = error;
                });
            }
            Err(error) => {
                update_job(&background_state, job_id, |job| {
                    job.status = "failed".into();
                    job.message = format!("backtest worker failed: {error}");
                });
            }
        }
    });

    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn job_status(
    State(state): State<Arc<BacktestApiState>>,
    Path(job_id): Path<u64>,
) -> Result<Json<BacktestJobResponse>, BacktestApiError> {
    let jobs = state
        .jobs
        .lock()
        .map_err(|_| BacktestApiError::Internal("backtest job state is unavailable".into()))?;
    let job = jobs
        .get(&job_id)
        .cloned()
        .ok_or(BacktestApiError::JobNotFound(job_id))?;
    Ok(Json(job))
}

async fn run_analysis(
    State(state): State<Arc<BacktestApiState>>,
    Path(run_id): Path<i64>,
) -> Result<Json<BacktestAnalysisView>, BacktestApiError> {
    if run_id <= 0 {
        return Err(BacktestApiError::Invalid("run_id must be positive".into()));
    }
    let storage_reader = Arc::clone(&state.storage_reader);
    let analysis = tokio::task::spawn_blocking(move || -> Result<BacktestAnalysisView, StorageError> {
        let run = storage_reader.trading_run(run_id)?;
        if run.mode != crate::storage::RunMode::Backtest {
            return Err(StorageError::InvalidTradingValue {
                field: "run_mode",
                value: run.mode.as_str().to_string(),
            });
        }
        Ok(BacktestAnalysisView {
            run_id,
            equity: storage_reader.trading_run_equity_points(run_id)?,
            positions: storage_reader.trading_run_position_points(run_id)?,
            order_levels: storage_reader.trading_run_order_levels(run_id)?,
        })
    }).await??;
    Ok(Json(analysis))
}

async fn run_result(
    State(state): State<Arc<BacktestApiState>>,
    Path(run_id): Path<i64>,
) -> Result<Json<BacktestRunView>, BacktestApiError> {
    if run_id <= 0 {
        return Err(BacktestApiError::Invalid("run_id must be positive".into()));
    }
    let storage_reader = Arc::clone(&state.storage_reader);
    let result = tokio::task::spawn_blocking(move || build_run_view(&storage_reader, run_id)).await??;
    Ok(Json(result))
}

fn execute_job(
    state: Arc<BacktestApiState>,
    job_id: u64,
    request: ValidatedBacktestRequest,
) -> Result<BacktestRunView, String> {
    let mut strategy = StaticGridStrategy::new(request.grid_config.clone())?;
    let config = BacktestRunConfig {
        dataset_id: request.dataset_id,
        replay_interval: request.replay_interval,
        start_time_ms: Some(request.start_time_ms),
        end_time_ms: Some(request.end_time_ms),
        comparison_id: Some(format!("ui-backtest-job-{job_id}")),
        initial_capital: request.initial_capital,
        execution: request.execution.clone(),
        run_config: json!({
            "source": "backtest_ui",
            "job_id": job_id,
            "strategy": "static_grid_fixture",
            "replay_interval": request.replay_interval.as_str()
        }),
    };
    let engine = BacktestEngine::new(Arc::clone(&state.storage_reader));
    let progress_state = Arc::clone(&state);
    let result = engine
        .run_with_progress(&config, &mut strategy, move |progress| {
            update_job(&progress_state, job_id, |job| {
                if progress > job.progress_percent {
                    job.progress_percent = progress;
                    job.message = if progress < 100 {
                        format!("Running historical replay · {progress}%")
                    } else {
                        "Finalizing persisted result".into()
                    };
                }
            });
        })
        .map_err(|error| error.to_string())?;

    build_run_view(&state.storage_reader, result.run_id).map_err(|error| error.to_string())
}

fn validate_request(request: CreateBacktestJobRequest) -> Result<ValidatedBacktestRequest, BacktestApiError> {
    if request.dataset_id <= 0 {
        return Err(BacktestApiError::Invalid("dataset_id must be positive".into()));
    }
    if request.strategy_id != "static-grid-fixture" {
        return Err(BacktestApiError::Invalid(
            "Phase 3.5 currently exposes only static-grid-fixture".into(),
        ));
    }
    let replay_interval = ReplayInterval::parse(request.replay_interval.trim())
        .ok_or_else(|| BacktestApiError::Invalid("replay_interval must be 1m, 1h, or 1d".into()))?;
    if request.start_time_ms <= 0 || request.end_time_ms <= 0 || request.start_time_ms > request.end_time_ms {
        return Err(BacktestApiError::Invalid(
            "start/end timestamps must define a valid positive range".into(),
        ));
    }
    let initial_capital = ExactDecimal::new(request.initial_capital.trim())?;
    let initial_capital_value = initial_capital
        .as_str()
        .parse::<f64>()
        .map_err(|_| BacktestApiError::Invalid("initial capital is not representable".into()))?;
    if !initial_capital_value.is_finite() || initial_capital_value <= 0.0 {
        return Err(BacktestApiError::Invalid("initial capital must be positive".into()));
    }

    let anchor = match request.grid.anchor.trim() {
        "previous_close" => GridAnchor::PreviousClose,
        "fixed" => GridAnchor::Fixed,
        _ => return Err(BacktestApiError::Invalid(
            "grid anchor must be previous_close or fixed".into(),
        )),
    };
    let grid_config = StaticGridConfig {
        anchor,
        fixed_anchor_price: request.grid.fixed_anchor_price,
        spacing_bps: request.grid.spacing_bps,
        levels_per_side: request.grid.levels_per_side,
        quantity_per_order: request.grid.quantity_per_order,
        time_in_force: TimeInForce::Gtc,
    };
    grid_config
        .validate()
        .map_err(BacktestApiError::Invalid)?;
    request
        .execution
        .validate()
        .map_err(BacktestApiError::Invalid)?;

    Ok(ValidatedBacktestRequest {
        dataset_id: request.dataset_id,
        replay_interval,
        start_time_ms: request.start_time_ms,
        end_time_ms: request.end_time_ms,
        initial_capital,
        grid_config,
        execution: request.execution,
    })
}

fn build_run_view(storage: &StorageReader, run_id: i64) -> Result<BacktestRunView, StorageError> {
    let run = storage.trading_run(run_id)?;
    let counts = storage.trading_run_counts(run_id)?;
    let equity = storage.trading_run_equity_stats(run_id)?;
    let fills = storage.trading_run_fill_audit(run_id)?;
    let latest_position = storage.trading_run_latest_position(run_id)?;

    let initial_capital = run.initial_capital.as_str().to_string();
    let initial_value = initial_capital.parse::<f64>().ok();
    let final_equity = equity.final_equity.as_ref().map(|value| value.as_str().to_string());
    let final_value = final_equity.as_deref().and_then(|value| value.parse::<f64>().ok());
    let total_return_percent = match (initial_value, final_value) {
        (Some(initial), Some(final_value)) if initial > 0.0 => Some((final_value / initial - 1.0) * 100.0),
        _ => None,
    };

    let data = &run.data_source;
    let dataset_id = json_i64(data, "dataset_id").unwrap_or_default();
    Ok(BacktestRunView {
        run_id,
        status: run.status.as_str().to_string(),
        strategy_id: run.strategy_id,
        strategy_version: run.strategy_version,
        strategy_params: run.strategy_params,
        execution_assumptions: run.execution_assumptions,
        dataset_id,
        symbol: json_string(data, "symbol"),
        market_type: json_string(data, "market_type"),
        source_interval: json_string(data, "source_interval"),
        replay_interval: json_string(data, "replay_interval"),
        requested_start_time_ms: json_i64(data, "requested_start_time_ms"),
        requested_end_time_ms: json_i64(data, "requested_end_time_ms"),
        effective_start_time_ms: json_i64(data, "effective_first_open_time_ms"),
        effective_end_time_ms: json_i64(data, "effective_last_close_time_ms"),
        reserved_first_candle_as_preroll: data
            .get("reserved_first_candle_as_preroll")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        candles_processed: equity.snapshot_count,
        initial_capital,
        final_equity,
        total_return_percent,
        max_drawdown_percent: equity.max_drawdown_percent,
        cash_balance: equity.final_cash_balance.map(|value| value.as_str().to_string()),
        realized_pnl: equity.final_realized_pnl.map(|value| value.as_str().to_string()),
        unrealized_pnl: equity.final_unrealized_pnl.map(|value| value.as_str().to_string()),
        fees_paid: equity.final_fees_paid.map(|value| value.as_str().to_string()),
        final_position_quantity: latest_position
            .map(|position| position.position_quantity.as_str().to_string())
            .unwrap_or_else(|| "0".into()),
        order_count: counts.orders,
        fill_count: counts.fills,
        fills,
    })
}

fn json_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn json_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn update_job(state: &BacktestApiState, job_id: u64, update: impl FnOnce(&mut BacktestJobResponse)) {
    if let Ok(mut jobs) = state.jobs.lock()
        && let Some(job) = jobs.get_mut(&job_id)
    {
        update(job);
    }
}

#[derive(Debug, Error)]
enum BacktestApiError {
    #[error("invalid backtest request: {0}")]
    Invalid(String),
    #[error("another backtest is already running")]
    Busy,
    #[error("backtest job {0} was not found")]
    JobNotFound(u64),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("backtest API task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("{0}")]
    Internal(String),
}

impl IntoResponse for BacktestApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::Busy => StatusCode::CONFLICT,
            Self::JobNotFound(_) | Self::Storage(StorageError::TradingRunNotFound(_)) => StatusCode::NOT_FOUND,
            Self::Storage(StorageError::DatasetNotFound(_)) => StatusCode::NOT_FOUND,
            Self::Storage(StorageError::InvalidTradingDecimal(_))
            | Self::Storage(StorageError::InvalidTradingValue { .. }) => StatusCode::BAD_REQUEST,
            Self::Storage(_) | Self::Task(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.to_string()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_supported_static_grid_request() {
        let request = CreateBacktestJobRequest {
            dataset_id: 1,
            replay_interval: "1h".into(),
            start_time_ms: 1_000,
            end_time_ms: 2_000,
            initial_capital: "100000".into(),
            strategy_id: "static-grid-fixture".into(),
            grid: StaticGridRequest {
                anchor: "previous_close".into(),
                fixed_anchor_price: None,
                spacing_bps: 100.0,
                levels_per_side: 3,
                quantity_per_order: 1.0,
            },
            execution: ExecutionAssumptions {
                fee_bps: 4.0,
                spread_bps: 0.0,
                slippage_bps: 0.0,
                latency_ms: 0,
                limit_fill_policy: LimitFillPolicy::Touch,
                partial_fill_ratio: 1.0,
            },
        };
        let validated = validate_request(request).unwrap();
        assert_eq!(validated.replay_interval, ReplayInterval::OneHour);
        assert_eq!(validated.grid_config.levels_per_side, 3);
    }
}
