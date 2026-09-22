use crate::{
    market::{Candle, FeedStatus, MarketError, MarketKey, MarketService, MarketSnapshot, MarketType},
    storage::{
        CreateOrderInput, DecisionInput, EquitySnapshotInput, EventTimes, ExactDecimal, FillInput,
        LiquidityRole, OrderIntentInput, OrderStateInput, OrderStatus, OrderType,
        PositionSnapshotInput, RunMode, RunStatus, StorageError, StorageReader,
        TradingFillAudit, TradingOrderLevel, TradingRunSpec,
    },
    trading::{
        decimal_string, ExecutionAssumptions, MarketCandle, PortfolioState,
        PortfolioView, SimulatedExecution, StaticGridConfig, StaticGridStrategy, Strategy,
        StrategyContext, StrategyOutput, StrategyStartContext, TradingInterval,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::{
    sync::{Mutex, RwLock, broadcast, watch},
    time::{Duration, MissedTickBehavior},
};

const BASE_INTERVAL_MS: i64 = 60_000;
const POLL_INTERVAL_MS: u64 = 500;
const ARMING_GRACE_MS: i64 = 120_000;
const MAX_ACTIVE_PAPER_RUNS: usize = 4;
const MAX_RECENT_RUNTIME_EVENTS: usize = 100;
const MAX_RECENT_RUNTIME_FILLS: usize = 100;
const SERVICE_RESTART_FAILURE_REASON: &str =
    "service_restart_interruption: paper runtime state and realtime chronology cannot be proven";

#[derive(Clone, Debug)]
pub struct PaperStartConfig {
    pub symbol: String,
    pub market_type: MarketType,
    pub replay_interval: TradingInterval,
    pub initial_capital: ExactDecimal,
    pub strategy_id: String,
    pub grid_config: StaticGridConfig,
    pub execution: ExecutionAssumptions,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaperOpenOrderView {
    pub order_id: i64,
    pub side: crate::storage::OrderSide,
    pub order_type: OrderType,
    pub price: Option<f64>,
    pub original_quantity: f64,
    pub remaining_quantity: f64,
    pub filled_quantity: f64,
    pub submitted_at_ms: i64,
    pub eligible_from_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaperFillView {
    pub event_time_ms: i64,
    pub order_id: i64,
    pub side: crate::storage::OrderSide,
    pub order_type: OrderType,
    pub price: f64,
    pub quantity: f64,
    pub fee: f64,
    pub status: OrderStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaperRuntimeEvent {
    pub event_time_ms: i64,
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaperBaseCandleView {
    pub open_time_ms: i64,
    pub close_time_ms: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub is_closed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaperSnapshot {
    pub run_id: i64,
    pub stream_revision: u64,
    pub comparison_id: Option<String>,
    pub mode: String,
    pub runtime_status: String,
    pub canonical_status: String,
    pub runtime_active: bool,
    pub created_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub ended_at_ms: Option<i64>,
    pub symbol: String,
    pub market_type: String,
    pub replay_interval: String,
    pub strategy_id: String,
    pub strategy_version: String,
    pub strategy_params: Value,
    pub run_config: Value,
    pub data_source: Value,
    pub execution_assumptions: Value,
    pub initial_capital: String,
    pub arming_boundary_ms: i64,
    pub feed_status: FeedStatus,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub mid_price: Option<f64>,
    pub latest_base_candle: Option<PaperBaseCandleView>,
    pub portfolio: PortfolioView,
    pub open_orders: Vec<PaperOpenOrderView>,
    pub recent_fills: Vec<PaperFillView>,
    pub recent_events: Vec<PaperRuntimeEvent>,
    pub latest_replay_candle: Option<MarketCandle>,
    pub last_base_candle_open_ms: Option<i64>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaperChartSnapshot {
    pub run_id: i64,
    pub symbol: String,
    pub market_type: String,
    pub base_interval: String,
    pub replay_interval: String,
    pub runtime_active: bool,
    pub candles: Vec<PaperBaseCandleView>,
    pub order_levels: Vec<TradingOrderLevel>,
    pub fills: Vec<TradingFillAudit>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaperRunSummary {
    pub run_id: i64,
    pub canonical_status: String,
    pub runtime_status: String,
    pub symbol: String,
    pub market_type: String,
    pub replay_interval: String,
    pub strategy_id: String,
    pub created_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub ended_at_ms: Option<i64>,
}

struct PaperRuntimeHandle {
    snapshot: Arc<RwLock<PaperSnapshot>>,
    stop_tx: watch::Sender<bool>,
    updates_tx: broadcast::Sender<PaperSnapshot>,
}

pub struct PaperManager {
    storage: Arc<StorageReader>,
    market: Arc<MarketService>,
    runtimes: Mutex<HashMap<i64, Arc<PaperRuntimeHandle>>>,
}

impl PaperManager {
    pub fn new(
        storage: Arc<StorageReader>,
        market: Arc<MarketService>,
    ) -> Result<Arc<Self>, PaperError> {
        terminate_interrupted_paper_runs(&storage, system_now_ms())?;

        Ok(Arc::new(Self {
            storage,
            market,
            runtimes: Mutex::new(HashMap::new()),
        }))
    }

    pub async fn start(self: &Arc<Self>, config: PaperStartConfig) -> Result<PaperSnapshot, PaperError> {
        validate_start_config(&config)?;

        let active_handles: Vec<_> = self.runtimes.lock().await.values().cloned().collect();
        let mut active_count = 0;
        for handle in active_handles {
            let status = handle.snapshot.read().await.runtime_status.clone();
            if matches!(status.as_str(), "arming" | "running") {
                active_count += 1;
            }
        }
        if active_count >= MAX_ACTIVE_PAPER_RUNS {
            return Err(PaperError::Busy(MAX_ACTIVE_PAPER_RUNS));
        }

        let symbol = config.symbol.trim().to_ascii_uppercase();
        let base_key = MarketKey::new(&symbol, "1m", config.market_type)?;
        let market_snapshot = self.market.snapshot_for(base_key.clone()).await?;
        let instrument_id = self
            .storage
            .ensure_market_instrument(&symbol, config.market_type.as_str())?;

        let strategy = build_strategy(&config)?;
        let strategy_id = strategy.id().to_string();
        let strategy_version = strategy.version().to_string();
        let strategy_params = strategy.parameters();
        let execution_json = serde_json::to_value(&config.execution)
            .map_err(|error| PaperError::Invalid(error.to_string()))?;
        let now = system_now_ms();
        let boundary = config.replay_interval.next_bucket_open_ms(now);

        let run = self.storage.create_trading_run(&TradingRunSpec {
            comparison_id: None,
            mode: RunMode::Paper,
            strategy_id: strategy_id.clone(),
            strategy_version: strategy_version.clone(),
            strategy_params: strategy_params.clone(),
            instrument_id,
            initial_capital: config.initial_capital.clone(),
            run_config: json!({
                "source": "trading_ui",
                "runtime": "paper",
                "arming_boundary_ms": boundary
            }),
            data_source: json!({
                "kind": "realtime_binance_public",
                "venue": "binance",
                "symbol": symbol,
                "market_type": config.market_type.as_str(),
                "source_interval": "1m",
                "replay_interval": config.replay_interval.as_str(),
                "arming_boundary_ms": boundary
            }),
            execution_assumptions: execution_json.clone(),
        })?;

        let initial_cash = config
            .initial_capital
            .as_str()
            .parse::<f64>()
            .map_err(|_| PaperError::Invalid("initial capital cannot be represented by simulator".into()))?;
        let portfolio = PortfolioState::new(initial_cash).map_err(PaperError::Simulation)?;
        let execution = SimulatedExecution::new(config.execution.clone()).map_err(PaperError::Simulation)?;
        let core = PaperRunCore {
            run_id: run.run_id,
            storage: Arc::clone(&self.storage),
            strategy,
            portfolio,
            execution,
            status: RunStatus::Created,
        };

        let initial_snapshot = PaperSnapshot {
            run_id: run.run_id,
            stream_revision: 1,
            comparison_id: run.comparison_id.clone(),
            mode: "paper".into(),
            runtime_status: "arming".into(),
            canonical_status: "created".into(),
            runtime_active: true,
            created_at_ms: run.created_at_ms,
            started_at_ms: run.started_at_ms,
            ended_at_ms: run.ended_at_ms,
            symbol: symbol.clone(),
            market_type: config.market_type.as_str().into(),
            replay_interval: config.replay_interval.as_str().into(),
            strategy_id,
            strategy_version,
            strategy_params,
            run_config: run.run_config.clone(),
            data_source: run.data_source.clone(),
            execution_assumptions: execution_json,
            initial_capital: config.initial_capital.as_str().to_string(),
            arming_boundary_ms: boundary,
            feed_status: market_snapshot.status,
            best_bid: market_snapshot.quote.best_bid,
            best_ask: market_snapshot.quote.best_ask,
            mid_price: market_snapshot.quote.mid_price,
            latest_base_candle: market_snapshot.candles.last().map(paper_base_candle),
            portfolio: core.portfolio.view(),
            open_orders: Vec::new(),
            recent_fills: Vec::new(),
            recent_events: vec![PaperRuntimeEvent {
                event_time_ms: now,
                kind: "arming".into(),
                message: format!(
                    "Waiting for clean {} boundary at {}",
                    config.replay_interval.as_str(),
                    boundary
                ),
            }],
            latest_replay_candle: None,
            last_base_candle_open_ms: None,
            updated_at_ms: now,
        };

        let (stop_tx, stop_rx) = watch::channel(false);
        let (updates_tx, _) = broadcast::channel(64);
        let handle = Arc::new(PaperRuntimeHandle {
            snapshot: Arc::new(RwLock::new(initial_snapshot.clone())),
            stop_tx,
            updates_tx,
        });
        self.runtimes.lock().await.insert(run.run_id, Arc::clone(&handle));

        let manager = Arc::clone(self);
        tokio::spawn(async move {
            manager
                .run_loop(handle, core, config, base_key, boundary, stop_rx)
                .await;
        });

        Ok(initial_snapshot)
    }

    pub async fn stop(&self, run_id: i64) -> Result<PaperSnapshot, PaperError> {
        let handle = self.runtimes.lock().await.get(&run_id).cloned();
        let Some(handle) = handle else {
            let snapshot = self.persisted_snapshot(run_id)?;
            if snapshot.canonical_status == RunStatus::Stopped.as_str() {
                return Ok(snapshot);
            }
            return Err(PaperError::RunNotActive(run_id));
        };

        let status = handle.snapshot.read().await.runtime_status.clone();
        if !matches!(status.as_str(), "arming" | "running") {
            return Ok(handle.snapshot.read().await.clone());
        }

        let _ = handle.stop_tx.send(true);
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let snapshot = handle.snapshot.read().await.clone();
            if !matches!(snapshot.runtime_status.as_str(), "arming" | "running") {
                return Ok(snapshot);
            }
        }
        Ok(handle.snapshot.read().await.clone())
    }

    pub async fn snapshot(&self, run_id: i64) -> Result<PaperSnapshot, PaperError> {
        if let Some(handle) = self.runtimes.lock().await.get(&run_id).cloned() {
            return Ok(handle.snapshot.read().await.clone());
        }
        self.persisted_snapshot(run_id)
    }

    pub async fn chart_snapshot(&self, run_id: i64) -> Result<PaperChartSnapshot, PaperError> {
        let snapshot = self.snapshot(run_id).await?;
        let order_levels = self.storage.trading_run_order_levels(run_id)?;
        let fills = self.storage.trading_run_fill_audit(run_id)?;
        let market_snapshot = if snapshot.runtime_active {
            let market_type = MarketType::parse(&snapshot.market_type)?;
            let key = MarketKey::new(&snapshot.symbol, "1m", market_type)?;
            Some(self.market.snapshot_for(key).await?)
        } else {
            None
        };

        Ok(paper_chart_snapshot(
            &snapshot,
            market_snapshot.as_ref(),
            order_levels,
            fills,
        ))
    }

    pub async fn stream_bootstrap(
        &self,
        run_id: i64,
    ) -> Result<(PaperSnapshot, Option<broadcast::Receiver<PaperSnapshot>>), PaperError> {
        let handle = self.runtimes.lock().await.get(&run_id).cloned();
        let Some(handle) = handle else {
            return Ok((self.persisted_snapshot(run_id)?, None));
        };

        // Subscribe before reading the full snapshot. Any update racing with the snapshot
        // read is queued, and the stream revision lets the client/server discard updates
        // already represented by the bootstrap snapshot.
        let receiver = handle.updates_tx.subscribe();
        let snapshot = handle.snapshot.read().await.clone();
        Ok((snapshot, Some(receiver)))
    }

    pub async fn list_runs(&self, limit: usize) -> Result<Vec<PaperRunSummary>, PaperError> {
        let runs = self.storage.trading_runs_by_mode(RunMode::Paper, limit)?;
        let handles: HashMap<i64, Arc<PaperRuntimeHandle>> = self.runtimes.lock().await.clone();
        let mut summaries = Vec::with_capacity(runs.len());
        for run in runs {
            let runtime_status = if let Some(handle) = handles.get(&run.run_id) {
                handle.snapshot.read().await.runtime_status.clone()
            } else {
                run.status.as_str().to_string()
            };
            summaries.push(PaperRunSummary {
                run_id: run.run_id,
                canonical_status: run.status.as_str().into(),
                runtime_status,
                symbol: json_string(&run.data_source, "symbol"),
                market_type: json_string(&run.data_source, "market_type"),
                replay_interval: json_string(&run.data_source, "replay_interval"),
                strategy_id: run.strategy_id,
                created_at_ms: run.created_at_ms,
                started_at_ms: run.started_at_ms,
                ended_at_ms: run.ended_at_ms,
            });
        }
        Ok(summaries)
    }

    fn persisted_snapshot(&self, run_id: i64) -> Result<PaperSnapshot, PaperError> {
        let run = self.storage.trading_run(run_id)?;
        if run.mode != RunMode::Paper {
            return Err(PaperError::RunNotFound(run_id));
        }
        let equity = self.storage.trading_run_equity_stats(run_id)?;
        let position = self.storage.trading_run_latest_position(run_id)?;
        let fills = self.storage.trading_run_fill_audit(run_id)?;
        let initial = run
            .initial_capital
            .as_str()
            .parse::<f64>()
            .map_err(|_| PaperError::Invalid("stored initial capital is not representable".into()))?;
        let cash = equity
            .final_cash_balance
            .as_ref()
            .and_then(|value| value.as_str().parse::<f64>().ok())
            .unwrap_or(initial);
        let position_quantity = position
            .as_ref()
            .and_then(|value| value.position_quantity.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let average_entry_price = position
            .as_ref()
            .and_then(|value| value.average_entry_price.as_ref())
            .and_then(|value| value.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let realized_pnl = equity
            .final_realized_pnl
            .as_ref()
            .and_then(|value| value.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let unrealized_pnl = equity
            .final_unrealized_pnl
            .as_ref()
            .and_then(|value| value.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let fees_paid = equity
            .final_fees_paid
            .as_ref()
            .and_then(|value| value.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let final_equity = equity
            .final_equity
            .as_ref()
            .and_then(|value| value.as_str().parse::<f64>().ok())
            .unwrap_or(initial);

        let recent_fill_start = fills.len().saturating_sub(MAX_RECENT_RUNTIME_FILLS);
        let recent_fills = fills
            .into_iter()
            .skip(recent_fill_start)
            .map(|fill| PaperFillView {
                event_time_ms: fill.event_time_ms,
                order_id: fill.order_id,
                side: fill.side,
                order_type: fill.order_type,
                price: fill.price.as_str().parse().unwrap_or(0.0),
                quantity: fill.quantity.as_str().parse().unwrap_or(0.0),
                fee: fill
                    .fee
                    .as_ref()
                    .and_then(|value| value.as_str().parse().ok())
                    .unwrap_or(0.0),
                status: OrderStatus::Filled,
            })
            .collect::<Vec<_>>();

        let persisted_arming_boundary_ms =
            json_i64(&run.data_source, "arming_boundary_ms").unwrap_or(run.created_at_ms);

        Ok(PaperSnapshot {
            run_id,
            stream_revision: 0,
            comparison_id: run.comparison_id,
            mode: "paper".into(),
            runtime_status: run.status.as_str().into(),
            canonical_status: run.status.as_str().into(),
            runtime_active: false,
            created_at_ms: run.created_at_ms,
            started_at_ms: run.started_at_ms,
            ended_at_ms: run.ended_at_ms,
            symbol: json_string(&run.data_source, "symbol"),
            market_type: json_string(&run.data_source, "market_type"),
            replay_interval: json_string(&run.data_source, "replay_interval"),
            strategy_id: run.strategy_id,
            strategy_version: run.strategy_version,
            strategy_params: run.strategy_params,
            run_config: run.run_config,
            data_source: run.data_source,
            execution_assumptions: run.execution_assumptions,
            initial_capital: run.initial_capital.as_str().to_string(),
            arming_boundary_ms: persisted_arming_boundary_ms,
            feed_status: FeedStatus::Loading,
            best_bid: None,
            best_ask: None,
            mid_price: None,
            latest_base_candle: None,
            portfolio: PortfolioView {
                cash,
                position_quantity,
                average_entry_price,
                realized_pnl,
                unrealized_pnl,
                fees_paid,
                equity: final_equity,
            },
            open_orders: Vec::new(),
            recent_fills,
            recent_events: vec![PaperRuntimeEvent {
                event_time_ms: run.updated_at_ms,
                kind: "persisted".into(),
                message: "Loaded persisted Paper run; runtime process is not active.".into(),
            }],
            latest_replay_candle: None,
            last_base_candle_open_ms: None,
            updated_at_ms: run.updated_at_ms,
        })
    }

    async fn run_loop(
        self: Arc<Self>,
        handle: Arc<PaperRuntimeHandle>,
        mut core: PaperRunCore,
        config: PaperStartConfig,
        base_key: MarketKey,
        boundary: i64,
        mut stop_rx: watch::Receiver<bool>,
    ) {
        let mut ticker = tokio::time::interval(Duration::from_millis(POLL_INTERVAL_MS));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut started = false;
        let mut feed_interrupted = false;
        let mut aggregator = ReplayAggregator::new(config.replay_interval, boundary);
        let bootstrap_key = match MarketKey::new(
            &base_key.symbol,
            config.replay_interval.as_str(),
            config.market_type,
        ) {
            Ok(key) => key,
            Err(error) => {
                self.finish_failed(&handle, &mut core, error.to_string()).await;
                return;
            }
        };

        loop {
            tokio::select! {
                biased;
                changed = stop_rx.changed() => {
                    if changed.is_err() || *stop_rx.borrow() {
                        let now = system_now_ms();
                        let result = core.stop(now, "user_stop");
                        match result {
                            Ok(()) => {
                                self.update_snapshot(&handle, core.portfolio.view(), core.open_orders(), |snapshot| {
                                    snapshot.runtime_status = "stopped".into();
                                    snapshot.canonical_status = "stopped".into();
                                    snapshot.runtime_active = false;
                                    snapshot.ended_at_ms = Some(now);
                                    snapshot.updated_at_ms = now;
                                    push_runtime_event(snapshot, now, "stopped", "Paper run stopped by user.");
                                }).await;
                            }
                            Err(error) => {
                                self.finish_failed(&handle, &mut core, error.to_string()).await;
                            }
                        }
                        break;
                    }
                }
                _ = ticker.tick() => {
                    let now = system_now_ms();
                    let market_snapshot = match self.market.snapshot_for(base_key.clone()).await {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            self.finish_failed(&handle, &mut core, format!("market feed unavailable: {error}")).await;
                            break;
                        }
                    };

                    self.update_snapshot(&handle, core.portfolio.view(), core.open_orders(), |snapshot| {
                        apply_market_snapshot(snapshot, &market_snapshot);
                        snapshot.updated_at_ms = now;
                    }).await;

                    if *stop_rx.borrow() {
                        continue;
                    }

                    if market_snapshot.status != FeedStatus::Live {
                        if !feed_interrupted {
                            feed_interrupted = true;
                            self.update_snapshot(&handle, core.portfolio.view(), core.open_orders(), |snapshot| {
                                snapshot.updated_at_ms = now;
                                push_runtime_event(
                                    snapshot,
                                    now,
                                    "feed_paused",
                                    "Paper strategy clock paused while Binance market feed is not live.",
                                );
                            }).await;
                        }
                        continue;
                    }

                    if !started {
                        if now < boundary {
                            continue;
                        }
                        match self.previous_replay_candle(&bootstrap_key, boundary, config.replay_interval).await {
                            Ok(Some(previous)) => {
                                match core.start(boundary, &previous) {
                                    Ok(()) => {
                                        started = true;
                                        self.update_snapshot(&handle, core.portfolio.view(), core.open_orders(), |snapshot| {
                                            snapshot.runtime_status = "running".into();
                                            snapshot.canonical_status = "running".into();
                                            snapshot.started_at_ms = Some(boundary);
                                            snapshot.updated_at_ms = now;
                                            push_runtime_event(
                                                snapshot,
                                                now,
                                                "running",
                                                &format!("Paper strategy started at clean {} boundary.", config.replay_interval.as_str()),
                                            );
                                        }).await;
                                    }
                                    Err(error) => {
                                        self.finish_failed(&handle, &mut core, error.to_string()).await;
                                        break;
                                    }
                                }
                            }
                            Ok(None) => {
                                if now > boundary.saturating_add(ARMING_GRACE_MS) {
                                    self.finish_failed(
                                        &handle,
                                        &mut core,
                                        "previous completed replay candle was unavailable after arming boundary".into(),
                                    ).await;
                                    break;
                                }
                                continue;
                            }
                            Err(error) => {
                                self.finish_failed(&handle, &mut core, error.to_string()).await;
                                break;
                            }
                        }
                    }

                    let expected = aggregator.expected_base_open_ms();
                    let candidates = match completed_base_candles_from_snapshot(
                        &market_snapshot.candles,
                        expected,
                    ) {
                        Ok(candidates) => candidates,
                        Err(error) => {
                            self.finish_failed(&handle, &mut core, error).await;
                            return;
                        }
                    };

                    if feed_interrupted {
                        feed_interrupted = false;
                        self.update_snapshot(&handle, core.portfolio.view(), core.open_orders(), |snapshot| {
                            snapshot.updated_at_ms = now;
                            push_runtime_event(
                                snapshot,
                                now,
                                "feed_resumed",
                                "Binance market feed resumed with continuous 1m chronology proven from the expected candle.",
                            );
                        }).await;
                    }

                    for candle in candidates {
                        if *stop_rx.borrow() {
                            break;
                        }

                        let completed = match aggregator.push(&candle) {
                            Ok(completed) => completed,
                            Err(error) => {
                                self.finish_failed(&handle, &mut core, error).await;
                                return;
                            }
                        };

                        self.update_snapshot(&handle, core.portfolio.view(), core.open_orders(), |snapshot| {
                            snapshot.last_base_candle_open_ms = Some(candle.open_time);
                            snapshot.updated_at_ms = system_now_ms();
                        }).await;

                        if let Some(replay_candle) = completed {
                            if *stop_rx.borrow() {
                                break;
                            }

                            match core.process_candle(&replay_candle) {
                                Ok(fills) => {
                                    self.update_snapshot(&handle, core.portfolio.view(), core.open_orders(), |snapshot| {
                                        snapshot.latest_replay_candle = Some(replay_candle.clone());
                                        snapshot.updated_at_ms = system_now_ms();
                                        for fill in fills {
                                            snapshot.recent_fills.push(PaperFillView {
                                                event_time_ms: fill.event_time_ms,
                                                order_id: fill.order_id,
                                                side: fill.side,
                                                order_type: fill.order_type,
                                                price: fill.price,
                                                quantity: fill.quantity,
                                                fee: fill.fee,
                                                status: fill.status,
                                            });
                                            if snapshot.recent_fills.len() > MAX_RECENT_RUNTIME_FILLS {
                                                snapshot.recent_fills.remove(0);
                                            }
                                            push_runtime_event(
                                                snapshot,
                                                fill.event_time_ms,
                                                "fill",
                                                &format!("{:?} fill · {} @ {}", fill.side, fill.quantity, fill.price),
                                            );
                                        }
                                        push_runtime_event(
                                            snapshot,
                                            replay_candle.close_time_ms,
                                            "candle",
                                            &format!("Completed {} strategy candle.", config.replay_interval.as_str()),
                                        );
                                    }).await;
                                }
                                Err(error) => {
                                    self.finish_failed(&handle, &mut core, error.to_string()).await;
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    async fn previous_replay_candle(
        &self,
        key: &MarketKey,
        boundary: i64,
        interval: TradingInterval,
    ) -> Result<Option<MarketCandle>, PaperError> {
        let snapshot = self.market.snapshot_for(key.clone()).await?;
        Ok(previous_completed_replay_candle(
            &snapshot.candles,
            boundary,
            interval,
        ))
    }

    async fn update_snapshot(
        &self,
        handle: &PaperRuntimeHandle,
        portfolio: PortfolioView,
        open_orders: Vec<PaperOpenOrderView>,
        update: impl FnOnce(&mut PaperSnapshot),
    ) {
        let mut snapshot = handle.snapshot.write().await;
        snapshot.portfolio = portfolio;
        snapshot.open_orders = open_orders;
        update(&mut snapshot);
        snapshot.stream_revision = snapshot.stream_revision.saturating_add(1);
        let cloned = snapshot.clone();
        drop(snapshot);
        let _ = handle.updates_tx.send(cloned);
    }

    async fn finish_failed(
        &self,
        handle: &PaperRuntimeHandle,
        core: &mut PaperRunCore,
        reason: String,
    ) {
        let now = system_now_ms();
        let _ = core.fail(now, &reason);
        let portfolio = core.portfolio.view();
        let open_orders = core.open_orders();
        self.update_snapshot(handle, portfolio, open_orders, |snapshot| {
            snapshot.runtime_status = "failed".into();
            snapshot.canonical_status = "failed".into();
            snapshot.runtime_active = false;
            snapshot.ended_at_ms = Some(now);
            snapshot.updated_at_ms = now;
            push_runtime_event(snapshot, now, "failed", &reason);
        }).await;
    }
}

fn terminate_interrupted_paper_runs(
    storage: &StorageReader,
    event_time_ms: i64,
) -> Result<Vec<i64>, PaperError> {
    let interrupted = storage.unfinished_trading_runs(RunMode::Paper)?;
    let mut failed_run_ids = Vec::with_capacity(interrupted.len());

    for run in interrupted {
        storage.set_trading_run_status(
            run.run_id,
            RunStatus::Failed,
            EventTimes::new(event_time_ms),
            Some(SERVICE_RESTART_FAILURE_REASON),
        )?;
        failed_run_ids.push(run.run_id);
    }

    Ok(failed_run_ids)
}

struct PaperRunCore {
    run_id: i64,
    storage: Arc<StorageReader>,
    strategy: Box<dyn Strategy + Send>,
    portfolio: PortfolioState,
    execution: SimulatedExecution,
    status: RunStatus,
}

impl PaperRunCore {
    fn start(&mut self, start_time_ms: i64, previous: &MarketCandle) -> Result<(), PaperError> {
        if self.status != RunStatus::Created {
            return Err(PaperError::RunNotActive(self.run_id));
        }
        self.storage.set_trading_run_status(
            self.run_id,
            RunStatus::Running,
            paper_times(start_time_ms),
            None,
        )?;
        self.status = RunStatus::Running;
        let context = StrategyStartContext {
            now_ms: start_time_ms,
            previous_candle: Some(previous),
            portfolio: self.portfolio.view(),
        };
        let output = self.strategy.on_start(&context).map_err(PaperError::Strategy)?;
        self.persist_strategy_output(start_time_ms, output)
    }

    fn process_candle(
        &mut self,
        candle: &MarketCandle,
    ) -> Result<Vec<crate::trading::SimulatedFill>, PaperError> {
        if self.status != RunStatus::Running {
            return Err(PaperError::RunNotActive(self.run_id));
        }

        let fills = self
            .execution
            .process_candle(candle)
            .map_err(PaperError::Simulation)?;

        for fill in &fills {
            self.storage.record_fill(&FillInput {
                order_id: fill.order_id,
                times: paper_times(fill.event_time_ms),
                exchange_trade_id: None,
                price: exact(fill.price)?,
                quantity: exact(fill.quantity)?,
                fee: Some(exact(fill.fee)?),
                fee_asset: Some("QUOTE".into()),
                liquidity_role: Some(match fill.order_type {
                    OrderType::Market => LiquidityRole::Taker,
                    OrderType::Limit => LiquidityRole::Maker,
                    _ => LiquidityRole::Taker,
                }),
                metadata: json!({"simulated": true, "mode": "paper"}),
            })?;

            self.portfolio
                .apply_fill(fill.side, fill.quantity, fill.price, fill.fee)
                .map_err(PaperError::Simulation)?;

            self.storage.record_order_state(&OrderStateInput {
                order_id: fill.order_id,
                times: paper_times(fill.event_time_ms),
                status: fill.status,
                filled_quantity: exact(fill.cumulative_filled_quantity)?,
                average_fill_price: Some(exact(fill.average_fill_price)?),
                reject_reason: None,
                metadata: json!({"simulated": true, "mode": "paper"}),
            })?;

            self.portfolio.mark(fill.price).map_err(PaperError::Simulation)?;
            let view = self.portfolio.view();
            self.storage.record_position_snapshot(&PositionSnapshotInput {
                run_id: self.run_id,
                times: paper_times(fill.event_time_ms),
                position_quantity: exact(view.position_quantity)?,
                average_entry_price: optional_positive_exact(view.average_entry_price)?,
                mark_price: optional_positive_exact(fill.price)?,
                realized_pnl: Some(exact(view.realized_pnl)?),
                unrealized_pnl: Some(exact(view.unrealized_pnl)?),
                cash_balance: Some(exact(view.cash)?),
                metadata: json!({"simulated": true, "mode": "paper"}),
            })?;
        }

        self.portfolio.mark(candle.close).map_err(PaperError::Simulation)?;
        let context = StrategyContext {
            now_ms: candle.close_time_ms,
            candle,
            portfolio: self.portfolio.view(),
        };
        let output = self.strategy.on_candle(&context).map_err(PaperError::Strategy)?;
        self.persist_strategy_output(candle.close_time_ms, output)?;

        let view = self.portfolio.view();
        self.storage.record_equity_snapshot(&EquitySnapshotInput {
            run_id: self.run_id,
            times: paper_times(candle.close_time_ms),
            equity: exact(view.equity)?,
            cash_balance: Some(exact(view.cash)?),
            realized_pnl: Some(exact(view.realized_pnl)?),
            unrealized_pnl: Some(exact(view.unrealized_pnl)?),
            fees_paid: Some(exact(view.fees_paid)?),
            metadata: json!({
                "mark_price": decimal_string(candle.close).map_err(PaperError::Simulation)?,
                "mode": "paper"
            }),
        })?;

        Ok(fills)
    }

    fn persist_strategy_output(
        &mut self,
        event_time_ms: i64,
        output: StrategyOutput,
    ) -> Result<(), PaperError> {
        for decision in output.decisions {
            self.storage.record_decision(&DecisionInput {
                run_id: self.run_id,
                times: paper_times(event_time_ms),
                decision_type: decision.decision_type,
                payload: decision.payload,
            })?;
        }

        for intent in output.order_intents {
            validate_intent(&intent)?;
            let persisted_intent = self.storage.record_order_intent(&OrderIntentInput {
                run_id: self.run_id,
                times: paper_times(event_time_ms),
                intent_key: intent.intent_key.clone(),
                side: intent.side,
                order_type: intent.order_type,
                time_in_force: intent.time_in_force,
                price: intent.price.map(exact).transpose()?,
                quantity: exact(intent.quantity)?,
                stop_price: intent.stop_price.map(exact).transpose()?,
                reduce_only: intent.reduce_only,
                metadata: intent.metadata.clone(),
            })?;
            let order = self.storage.create_trading_order(&CreateOrderInput {
                run_id: self.run_id,
                times: paper_times(event_time_ms),
                intent_event_id: Some(persisted_intent.event.event_id),
                client_order_id: None,
                exchange_order_id: None,
                side: intent.side,
                order_type: intent.order_type,
                time_in_force: intent.time_in_force,
                price: intent.price.map(exact).transpose()?,
                quantity: exact(intent.quantity)?,
                stop_price: intent.stop_price.map(exact).transpose()?,
                metadata: intent.metadata.clone(),
            })?;
            self.storage.record_order_state(&OrderStateInput {
                order_id: order.order_id,
                times: paper_times(event_time_ms),
                status: OrderStatus::Accepted,
                filled_quantity: ExactDecimal::zero(),
                average_fill_price: None,
                reject_reason: None,
                metadata: json!({"simulated": true, "mode": "paper"}),
            })?;
            self.execution
                .submit(order.order_id, event_time_ms, intent)
                .map_err(PaperError::Simulation)?;
        }
        Ok(())
    }

    fn open_orders(&self) -> Vec<PaperOpenOrderView> {
        self.execution
            .pending_orders()
            .iter()
            .map(|pending| PaperOpenOrderView {
                order_id: pending.order_id,
                side: pending.intent.side,
                order_type: pending.intent.order_type,
                price: pending.intent.price,
                original_quantity: pending.original_quantity,
                remaining_quantity: pending.remaining_quantity,
                filled_quantity: pending.filled_quantity,
                submitted_at_ms: pending.submitted_at_ms,
                eligible_from_ms: pending.eligible_from_ms,
            })
            .collect()
    }

    fn stop(&mut self, event_time_ms: i64, reason: &str) -> Result<(), PaperError> {
        if self.status == RunStatus::Stopped {
            return Ok(());
        }
        if matches!(self.status, RunStatus::Completed | RunStatus::Failed) {
            return Ok(());
        }

        self.expire_pending(event_time_ms, reason)?;
        let run = self.storage.trading_run(self.run_id)?;
        if matches!(run.status, RunStatus::Created | RunStatus::Running) {
            self.storage.set_trading_run_status(
                self.run_id,
                RunStatus::Stopped,
                EventTimes::new(event_time_ms),
                Some(reason),
            )?;
            self.status = RunStatus::Stopped;
        } else {
            self.status = run.status;
        }
        Ok(())
    }

    fn fail(&mut self, event_time_ms: i64, reason: &str) -> Result<(), PaperError> {
        if matches!(self.status, RunStatus::Completed | RunStatus::Failed | RunStatus::Stopped) {
            return Ok(());
        }

        self.expire_pending(event_time_ms, reason)?;
        let run = self.storage.trading_run(self.run_id)?;
        if matches!(run.status, RunStatus::Created | RunStatus::Running) {
            self.storage.set_trading_run_status(
                self.run_id,
                RunStatus::Failed,
                EventTimes::new(event_time_ms),
                Some(reason),
            )?;
            self.status = RunStatus::Failed;
        } else {
            self.status = run.status;
        }
        Ok(())
    }

    fn expire_pending(&mut self, event_time_ms: i64, reason: &str) -> Result<(), PaperError> {
        for pending in self.execution.expire_all() {
            let filled = pending.original_quantity - pending.remaining_quantity;
            self.storage.record_order_state(&OrderStateInput {
                order_id: pending.order_id,
                times: EventTimes::new(event_time_ms),
                status: OrderStatus::Expired,
                filled_quantity: exact(filled)?,
                average_fill_price: if pending.filled_quantity > 0.0 {
                    Some(exact(pending.filled_notional / pending.filled_quantity)?)
                } else {
                    None
                },
                reject_reason: None,
                metadata: json!({"simulated": true, "mode": "paper", "reason": reason}),
            })?;
        }
        Ok(())
    }
}

fn previous_completed_replay_candle(
    candles: &[Candle],
    boundary_ms: i64,
    interval: TradingInterval,
) -> Option<MarketCandle> {
    let target_open = boundary_ms.saturating_sub(interval.duration_ms());
    candles
        .iter()
        .rev()
        .find(|candle| {
            candle.is_closed
                && candle.open_time == target_open
                && candle.close_time < boundary_ms
        })
        .map(market_candle)
}

fn completed_base_candles_from_snapshot(
    candles: &[Candle],
    expected_open_ms: i64,
) -> Result<Vec<Candle>, String> {
    let mut previous_open = None;
    let mut next_expected = expected_open_ms;
    let mut fresh = Vec::new();

    for candle in candles {
        if candle.open_time.rem_euclid(BASE_INTERVAL_MS) != 0 {
            return Err(format!(
                "market_data_integrity: 1m candle open time is not UTC-minute aligned: {}",
                candle.open_time
            ));
        }
        if let Some(previous) = previous_open
            && candle.open_time <= previous
        {
            return Err(format!(
                "market_data_integrity: duplicate/out-of-order 1m candles in backend market snapshot: {} after {}",
                candle.open_time, previous
            ));
        }
        previous_open = Some(candle.open_time);

        // Older candles are rolling-snapshot history and are never replayed again.
        if candle.open_time < expected_open_ms {
            continue;
        }

        if candle.open_time > next_expected {
            return Err(format!(
                "market_data_gap: expected 1m candle at {}, but backend snapshot advanced to {}",
                next_expected, candle.open_time
            ));
        }

        // The expected candle exists but is still forming. Continuity is intact, so
        // wait for its completed form rather than advancing the strategy clock.
        if !candle.is_closed {
            break;
        }

        fresh.push(candle.clone());
        next_expected = next_expected.saturating_add(BASE_INTERVAL_MS);
    }

    Ok(fresh)
}

#[derive(Debug)]
struct ReplayAggregator {
    interval: TradingInterval,
    expected_base_open_ms: i64,
    current: Option<MarketCandle>,
}

impl ReplayAggregator {
    fn new(interval: TradingInterval, start_boundary_ms: i64) -> Self {
        Self {
            interval,
            expected_base_open_ms: start_boundary_ms,
            current: None,
        }
    }

    fn expected_base_open_ms(&self) -> i64 {
        self.expected_base_open_ms
    }

    fn push(&mut self, candle: &Candle) -> Result<Option<MarketCandle>, String> {
        if !candle.is_closed {
            return Err("Paper aggregator accepts completed 1m candles only".into());
        }
        if candle.open_time != self.expected_base_open_ms {
            return Err(format!(
                "duplicate/out-of-order/gap candle: expected {}, received {}",
                self.expected_base_open_ms,
                candle.open_time
            ));
        }
        if candle.open_time.rem_euclid(BASE_INTERVAL_MS) != 0 {
            return Err("1m candle open time is not aligned to a UTC minute".into());
        }

        let bucket_open = self.interval.bucket_open_ms(candle.open_time);
        match self.current.as_mut() {
            Some(current) if current.open_time_ms == bucket_open => {
                current.close_time_ms = candle.close_time;
                current.high = current.high.max(candle.high);
                current.low = current.low.min(candle.low);
                current.close = candle.close;
                current.volume += candle.volume;
            }
            Some(_) => {
                return Err("new replay bucket arrived before prior bucket completed".into());
            }
            None => {
                self.current = Some(MarketCandle {
                    open_time_ms: bucket_open,
                    close_time_ms: candle.close_time,
                    open: candle.open,
                    high: candle.high,
                    low: candle.low,
                    close: candle.close,
                    volume: candle.volume,
                });
            }
        }

        self.expected_base_open_ms = self.expected_base_open_ms.saturating_add(BASE_INTERVAL_MS);
        let last_base_open = bucket_open
            .saturating_add(self.interval.duration_ms())
            .saturating_sub(BASE_INTERVAL_MS);
        if candle.open_time == last_base_open {
            return Ok(self.current.take());
        }
        Ok(None)
    }
}

fn build_strategy(config: &PaperStartConfig) -> Result<Box<dyn Strategy + Send>, PaperError> {
    match config.strategy_id.as_str() {
        "static-grid-fixture" => Ok(Box::new(
            StaticGridStrategy::new(config.grid_config.clone()).map_err(PaperError::Invalid)?,
        )),
        other => Err(PaperError::Invalid(format!(
            "unsupported Paper strategy: {other}"
        ))),
    }
}

fn validate_start_config(config: &PaperStartConfig) -> Result<(), PaperError> {
    if config.strategy_id != "static-grid-fixture" {
        return Err(PaperError::Invalid(
            "Phase 4 currently exposes only static-grid-fixture".into(),
        ));
    }
    let capital = config
        .initial_capital
        .as_str()
        .parse::<f64>()
        .map_err(|_| PaperError::Invalid("initial capital is not representable".into()))?;
    if !capital.is_finite() || capital <= 0.0 {
        return Err(PaperError::Invalid("initial capital must be positive".into()));
    }
    config.execution.validate().map_err(PaperError::Invalid)?;
    config.grid_config.validate().map_err(PaperError::Invalid)?;
    Ok(())
}

fn validate_intent(intent: &crate::trading::StrategyOrderIntent) -> Result<(), PaperError> {
    if !intent.quantity.is_finite() || intent.quantity <= 0.0 {
        return Err(PaperError::Simulation(
            "order quantity must be finite and positive".into(),
        ));
    }
    if let Some(price) = intent.price
        && (!price.is_finite() || price <= 0.0)
    {
        return Err(PaperError::Simulation(
            "order price must be finite and positive".into(),
        ));
    }
    Ok(())
}

fn exact(value: f64) -> Result<ExactDecimal, PaperError> {
    let value = decimal_string(value).map_err(PaperError::Simulation)?;
    Ok(ExactDecimal::new(value)?)
}

fn optional_positive_exact(value: f64) -> Result<Option<ExactDecimal>, PaperError> {
    if value > 0.0 {
        Ok(Some(exact(value)?))
    } else {
        Ok(None)
    }
}

fn paper_base_candle(candle: &Candle) -> PaperBaseCandleView {
    PaperBaseCandleView {
        open_time_ms: candle.open_time,
        close_time_ms: candle.close_time,
        open: candle.open,
        high: candle.high,
        low: candle.low,
        close: candle.close,
        volume: candle.volume,
        is_closed: candle.is_closed,
    }
}

fn paper_chart_snapshot(
    snapshot: &PaperSnapshot,
    market: Option<&MarketSnapshot>,
    order_levels: Vec<TradingOrderLevel>,
    fills: Vec<TradingFillAudit>,
) -> PaperChartSnapshot {
    PaperChartSnapshot {
        run_id: snapshot.run_id,
        symbol: snapshot.symbol.clone(),
        market_type: snapshot.market_type.clone(),
        base_interval: "1m".into(),
        replay_interval: snapshot.replay_interval.clone(),
        runtime_active: snapshot.runtime_active,
        candles: market
            .map(|market| market.candles.iter().map(paper_base_candle).collect())
            .unwrap_or_default(),
        order_levels,
        fills,
        updated_at_ms: snapshot.updated_at_ms,
    }
}

fn apply_market_snapshot(snapshot: &mut PaperSnapshot, market: &MarketSnapshot) {
    snapshot.feed_status = market.status;
    snapshot.best_bid = market.quote.best_bid;
    snapshot.best_ask = market.quote.best_ask;
    snapshot.mid_price = market.quote.mid_price;
    snapshot.latest_base_candle = market.candles.last().map(paper_base_candle);
}

fn market_candle(candle: &Candle) -> MarketCandle {
    MarketCandle {
        open_time_ms: candle.open_time,
        close_time_ms: candle.close_time,
        open: candle.open,
        high: candle.high,
        low: candle.low,
        close: candle.close,
        volume: candle.volume,
    }
}

fn paper_times(event_time_ms: i64) -> EventTimes {
    EventTimes {
        event_time_ms,
        exchange_time_ms: Some(event_time_ms),
        received_at_ms: Some(system_now_ms()),
    }
}

fn push_runtime_event(
    snapshot: &mut PaperSnapshot,
    event_time_ms: i64,
    kind: &str,
    message: &str,
) {
    snapshot.recent_events.push(PaperRuntimeEvent {
        event_time_ms,
        kind: kind.into(),
        message: message.into(),
    });
    if snapshot.recent_events.len() > MAX_RECENT_RUNTIME_EVENTS {
        let excess = snapshot.recent_events.len() - MAX_RECENT_RUNTIME_EVENTS;
        snapshot.recent_events.drain(0..excess);
    }
}

fn json_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn json_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[derive(Debug, Error)]
pub enum PaperError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Market(#[from] MarketError),
    #[error("invalid Paper configuration: {0}")]
    Invalid(String),
    #[error("Paper strategy error: {0}")]
    Strategy(String),
    #[error("Paper simulation error: {0}")]
    Simulation(String),
    #[error("Paper run {0} was not found")]
    RunNotFound(i64),
    #[error("Paper run {0} is not active")]
    RunNotActive(i64),
    #[error("Paper runtime capacity reached ({0} active runs)")]
    Busy(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(open_time: i64, open: f64, high: f64, low: f64, close: f64) -> Candle {
        Candle {
            open_time,
            close_time: open_time + 59_999,
            open,
            high,
            low,
            close,
            volume: 1.0,
            is_closed: true,
        }
    }

    #[test]
    fn interval_boundaries_are_shared_and_utc_aligned() {
        assert_eq!(TradingInterval::OneMinute.bucket_open_ms(61_234), 60_000);
        assert_eq!(TradingInterval::OneHour.bucket_open_ms(3_999_999), 3_600_000);
        assert_eq!(TradingInterval::OneDay.bucket_open_ms(90_000_000), 86_400_000);
        assert_eq!(TradingInterval::OneHour.next_bucket_open_ms(3_600_000), 7_200_000);
    }

    #[test]
    fn aggregates_completed_minutes_without_lookahead() {
        let mut aggregator = ReplayAggregator::new(TradingInterval::OneHour, 0);
        let mut completed = None;
        for minute in 0..60 {
            let open_time = minute * 60_000;
            completed = aggregator
                .push(&candle(
                    open_time,
                    100.0 + minute as f64,
                    102.0 + minute as f64,
                    99.0,
                    101.0 + minute as f64,
                ))
                .unwrap();
            if minute < 59 {
                assert!(completed.is_none());
            }
        }
        let replay = completed.expect("hour completes only after minute 59 closes");
        assert_eq!(replay.open_time_ms, 0);
        assert_eq!(replay.close_time_ms, 3_599_999);
        assert_eq!(replay.open, 100.0);
        assert_eq!(replay.close, 160.0);
        assert_eq!(replay.high, 161.0);
        assert_eq!(replay.low, 99.0);
    }

    #[test]
    fn rejects_gap_or_out_of_order_base_candles() {
        let mut aggregator = ReplayAggregator::new(TradingInterval::OneMinute, 60_000);
        let error = aggregator
            .push(&candle(120_000, 1.0, 1.0, 1.0, 1.0))
            .unwrap_err();
        assert!(error.contains("expected 60000"));
    }

    #[test]
    fn arming_boundary_is_the_next_clean_utc_interval() {
        assert_eq!(TradingInterval::OneMinute.next_bucket_open_ms(61_234), 120_000);
        assert_eq!(TradingInterval::OneHour.next_bucket_open_ms(3_600_001), 7_200_000);
        assert_eq!(TradingInterval::OneDay.next_bucket_open_ms(90_000_000), 172_800_000);
    }

    #[test]
    fn bootstrap_selects_only_the_immediately_previous_completed_replay_candle() {
        let mut older = candle(0, 100.0, 101.0, 99.0, 100.5);
        older.close_time = 3_599_999;
        let mut previous = candle(3_600_000, 101.0, 103.0, 100.0, 102.0);
        previous.close_time = 7_199_999;
        let mut current = candle(7_200_000, 102.0, 104.0, 101.0, 103.0);
        current.close_time = 10_799_999;
        current.is_closed = false;

        let selected = previous_completed_replay_candle(
            &[older, previous.clone(), current],
            7_200_000,
            TradingInterval::OneHour,
        )
        .expect("previous completed replay candle");

        assert_eq!(selected.open_time_ms, previous.open_time);
        assert_eq!(selected.close_time_ms, previous.close_time);
        assert_eq!(selected.close, previous.close);
    }

    #[test]
    fn backend_snapshot_validation_rejects_duplicate_out_of_order_and_missing_minutes() {
        let duplicate = vec![
            candle(60_000, 1.0, 1.0, 1.0, 1.0),
            candle(60_000, 1.0, 1.0, 1.0, 1.0),
        ];
        assert!(completed_base_candles_from_snapshot(&duplicate, 60_000)
            .err()
            .expect("duplicate snapshot must be rejected")
            .contains("duplicate/out-of-order"));

        let out_of_order = vec![
            candle(120_000, 1.0, 1.0, 1.0, 1.0),
            candle(60_000, 1.0, 1.0, 1.0, 1.0),
        ];
        assert!(completed_base_candles_from_snapshot(&out_of_order, 60_000)
            .err()
            .expect("invalid snapshot must be rejected")
            .contains("market_data_gap:"));

        let gap = vec![
            candle(60_000, 1.0, 1.0, 1.0, 1.0),
            candle(180_000, 1.0, 1.0, 1.0, 1.0),
        ];
        assert!(completed_base_candles_from_snapshot(&gap, 60_000)
            .err()
            .expect("invalid snapshot must be rejected")
            .contains("market_data_gap:"));
    }

    #[test]
    fn reconnect_accepts_only_contiguous_completed_minutes_before_current_incomplete_candle() {
        let mut current = candle(180_000, 1.0, 1.0, 1.0, 1.0);
        current.is_closed = false;
        let snapshot = vec![
            candle(0, 1.0, 1.0, 1.0, 1.0),
            candle(60_000, 1.0, 1.0, 1.0, 1.0),
            candle(120_000, 1.0, 1.0, 1.0, 1.0),
            current,
        ];

        let recovered = completed_base_candles_from_snapshot(&snapshot, 60_000).unwrap();
        assert_eq!(
            recovered.iter().map(|item| item.open_time).collect::<Vec<_>>(),
            vec![60_000, 120_000]
        );
    }

    #[test]
    fn reconnect_fails_if_snapshot_advanced_past_missing_expected_minute() {
        let mut later_current = candle(120_000, 1.0, 1.0, 1.0, 1.0);
        later_current.is_closed = false;
        let snapshot = vec![
            candle(0, 1.0, 1.0, 1.0, 1.0),
            later_current,
        ];

        let error = completed_base_candles_from_snapshot(&snapshot, 60_000)
            .err()
            .expect("missing expected minute must fail");
        assert!(error.starts_with("market_data_gap:"));
        assert!(error.contains("expected 1m candle at 60000"));
        assert!(error.contains("advanced to 120000"));
    }

    #[test]
    fn feed_gap_failure_reason_is_persisted_for_later_comparison() {
        let path = temp_database("feed-gap-reason");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let mut core = test_core(Arc::clone(&storage), "feedgapreason");

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();

        let reason = "market_data_gap: expected 1m candle at 60000, but backend snapshot advanced to 120000";
        core.fail(120_000, reason).unwrap();

        let history = storage.trading_run_history(core.run_id).unwrap();
        assert_eq!(history.run.status, RunStatus::Failed);
        assert_eq!(
            history.status_events.last().and_then(|event| event.note.as_deref()),
            Some(reason)
        );

        drop(core);
        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn stale_snapshot_history_is_ignored_before_the_strategy_clock() {
        let candles = vec![
            candle(0, 1.0, 1.0, 1.0, 1.0),
            candle(60_000, 1.0, 1.0, 1.0, 1.0),
            candle(120_000, 1.0, 1.0, 1.0, 1.0),
        ];
        let fresh = completed_base_candles_from_snapshot(&candles, 120_000).unwrap();
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].open_time, 120_000);
    }

    #[test]
    fn paper_and_backtest_aggregation_use_the_same_utc_bucket_rules() {
        for interval in [
            TradingInterval::OneMinute,
            TradingInterval::OneHour,
            TradingInterval::OneDay,
        ] {
            let minute_count = (interval.duration_ms() / BASE_INTERVAL_MS) as usize;
            let source: Vec<crate::storage::OhlcvCandle> = (0..minute_count)
                .map(|index| {
                    let open_time = index as i64 * BASE_INTERVAL_MS;
                    let open = 100.0 + index as f64 * 0.01;
                    crate::storage::OhlcvCandle {
                        open_time_ms: open_time,
                        close_time_ms: open_time + BASE_INTERVAL_MS - 1,
                        open_price: open,
                        high_price: open + 2.0,
                        low_price: open - 1.0,
                        close_price: open + 0.5,
                        base_volume: 1.0 + index as f64 * 0.001,
                        quote_volume: None,
                        trade_count: None,
                        taker_buy_base_volume: None,
                        taker_buy_quote_volume: None,
                    }
                })
                .collect();

            let backtest = crate::backtest::aggregate_candles(&source, interval).unwrap();
            assert_eq!(backtest.len(), 1);

            let mut paper_aggregator = ReplayAggregator::new(interval, 0);
            let mut paper_completed = None;
            for item in &source {
                let live = Candle {
                    open_time: item.open_time_ms,
                    close_time: item.close_time_ms,
                    open: item.open_price,
                    high: item.high_price,
                    low: item.low_price,
                    close: item.close_price,
                    volume: item.base_volume,
                    is_closed: true,
                };
                if let Some(completed) = paper_aggregator.push(&live).unwrap() {
                    paper_completed = Some(completed);
                }
            }

            let paper = paper_completed.expect("paper replay bucket completes");
            let historical = &backtest[0];
            assert_eq!(paper.open_time_ms, historical.open_time_ms);
            assert_eq!(paper.close_time_ms, historical.close_time_ms);
            assert!((paper.open - historical.open_price).abs() < 1e-12);
            assert!((paper.high - historical.high_price).abs() < 1e-12);
            assert!((paper.low - historical.low_price).abs() < 1e-12);
            assert!((paper.close - historical.close_price).abs() < 1e-12);
            assert!((paper.volume - historical.base_volume).abs() < 1e-9);
        }
    }

    struct TimingProbeStrategy {
        emitted_exit: bool,
    }

    impl Strategy for TimingProbeStrategy {
        fn id(&self) -> &str {
            "test-paper-timing-probe"
        }

        fn version(&self) -> &str {
            "1"
        }

        fn parameters(&self) -> Value {
            json!({})
        }

        fn on_start(
            &mut self,
            _context: &StrategyStartContext<'_>,
        ) -> Result<StrategyOutput, String> {
            Ok(StrategyOutput {
                decisions: Vec::new(),
                order_intents: vec![crate::trading::StrategyOrderIntent {
                    intent_key: Some("resting-buy".into()),
                    side: crate::storage::OrderSide::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: Some(crate::storage::TimeInForce::Gtc),
                    price: Some(100.0),
                    quantity: 1.0,
                    stop_price: None,
                    reduce_only: false,
                    metadata: json!({"phase": "on_start"}),
                }],
            })
        }

        fn on_candle(&mut self, context: &StrategyContext<'_>) -> Result<StrategyOutput, String> {
            let mut order_intents = Vec::new();
            if !self.emitted_exit {
                self.emitted_exit = true;
                order_intents.push(crate::trading::StrategyOrderIntent {
                    intent_key: Some("close-after-first-candle".into()),
                    side: crate::storage::OrderSide::Sell,
                    order_type: OrderType::Limit,
                    time_in_force: Some(crate::storage::TimeInForce::Gtc),
                    price: Some(111.0),
                    quantity: 1.0,
                    stop_price: None,
                    reduce_only: false,
                    metadata: json!({"phase": "on_candle"}),
                });
            }

            Ok(StrategyOutput {
                decisions: vec![crate::trading::StrategyDecision {
                    decision_type: "timing_probe".into(),
                    payload: json!({
                        "seen_position_quantity": context.portfolio.position_quantity,
                        "signal_time_ms": context.now_ms
                    }),
                }],
                order_intents,
            })
        }
    }

    struct MarketOrderOnStart;

    impl Strategy for MarketOrderOnStart {
        fn id(&self) -> &str {
            "test-paper-market-order"
        }

        fn version(&self) -> &str {
            "1"
        }

        fn parameters(&self) -> Value {
            json!({})
        }

        fn on_start(
            &mut self,
            _context: &StrategyStartContext<'_>,
        ) -> Result<StrategyOutput, String> {
            Ok(StrategyOutput {
                decisions: Vec::new(),
                order_intents: vec![crate::trading::StrategyOrderIntent {
                    intent_key: Some("market-entry".into()),
                    side: crate::storage::OrderSide::Buy,
                    order_type: OrderType::Market,
                    time_in_force: None,
                    price: None,
                    quantity: 2.0,
                    stop_price: None,
                    reduce_only: false,
                    metadata: json!({}),
                }],
            })
        }

        fn on_candle(&mut self, _context: &StrategyContext<'_>) -> Result<StrategyOutput, String> {
            Ok(StrategyOutput::default())
        }
    }

    struct TouchPolicyProbe;

    impl Strategy for TouchPolicyProbe {
        fn id(&self) -> &str {
            "test-paper-touch-policy"
        }

        fn version(&self) -> &str {
            "1"
        }

        fn parameters(&self) -> Value {
            json!({})
        }

        fn on_start(
            &mut self,
            _context: &StrategyStartContext<'_>,
        ) -> Result<StrategyOutput, String> {
            Ok(StrategyOutput {
                decisions: Vec::new(),
                order_intents: vec![crate::trading::StrategyOrderIntent {
                    intent_key: Some("trade-through-buy".into()),
                    side: crate::storage::OrderSide::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: Some(crate::storage::TimeInForce::Gtc),
                    price: Some(100.0),
                    quantity: 1.0,
                    stop_price: None,
                    reduce_only: false,
                    metadata: json!({}),
                }],
            })
        }

        fn on_candle(&mut self, _context: &StrategyContext<'_>) -> Result<StrategyOutput, String> {
            Ok(StrategyOutput::default())
        }
    }

    struct NoOpStrategy;

    impl Strategy for NoOpStrategy {
        fn id(&self) -> &str {
            "test-paper-noop"
        }

        fn version(&self) -> &str {
            "1"
        }

        fn parameters(&self) -> Value {
            json!({})
        }

        fn on_candle(&mut self, _context: &StrategyContext<'_>) -> Result<StrategyOutput, String> {
            Ok(StrategyOutput::default())
        }
    }

    fn temp_database(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "binance-grid-paper-{label}-{}-{}.sqlite3",
            std::process::id(),
            system_now_ms()
        ))
    }

    fn cleanup_database(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    fn test_core_with_strategy(
        storage: Arc<StorageReader>,
        label: &str,
        strategy: Box<dyn Strategy + Send>,
        assumptions: ExecutionAssumptions,
    ) -> PaperRunCore {
        let instrument_id = storage
            .ensure_market_instrument(&format!("{}USDT", label.to_ascii_uppercase()), "spot")
            .unwrap();
        let run = storage
            .create_trading_run(&TradingRunSpec {
                comparison_id: Some(format!("phase-4-3-{label}")),
                mode: RunMode::Paper,
                strategy_id: strategy.id().to_string(),
                strategy_version: strategy.version().to_string(),
                strategy_params: strategy.parameters(),
                instrument_id,
                initial_capital: ExactDecimal::new("1000").unwrap(),
                run_config: json!({"test": "phase_4_3"}),
                data_source: json!({"kind": "test"}),
                execution_assumptions: serde_json::to_value(&assumptions).unwrap(),
            })
            .unwrap();

        PaperRunCore {
            run_id: run.run_id,
            storage,
            strategy,
            portfolio: PortfolioState::new(1000.0).unwrap(),
            execution: SimulatedExecution::new(assumptions).unwrap(),
            status: RunStatus::Created,
        }
    }

    fn test_core(storage: Arc<StorageReader>, label: &str) -> PaperRunCore {
        let instrument_id = storage
            .ensure_market_instrument(&format!("{}USDT", label.to_ascii_uppercase()), "spot")
            .unwrap();
        let run = storage
            .create_trading_run(&TradingRunSpec {
                comparison_id: Some(format!("phase-4-1-{label}")),
                mode: RunMode::Paper,
                strategy_id: "test-paper-noop".into(),
                strategy_version: "1".into(),
                strategy_params: json!({}),
                instrument_id,
                initial_capital: ExactDecimal::new("1000").unwrap(),
                run_config: json!({"test": "phase_4_1"}),
                data_source: json!({"kind": "test"}),
                execution_assumptions: serde_json::to_value(ExecutionAssumptions::default()).unwrap(),
            })
            .unwrap();

        PaperRunCore {
            run_id: run.run_id,
            storage,
            strategy: Box::new(NoOpStrategy),
            portfolio: PortfolioState::new(1000.0).unwrap(),
            execution: SimulatedExecution::new(ExecutionAssumptions::default()).unwrap(),
            status: RunStatus::Created,
        }
    }

    #[test]
    fn paper_processes_resting_fills_before_strategy_and_never_retrofills_new_orders() {
        let path = temp_database("execution-ordering");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let mut core = test_core_with_strategy(
            Arc::clone(&storage),
            "executionordering",
            Box::new(TimingProbeStrategy { emitted_exit: false }),
            ExecutionAssumptions::default(),
        );

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 105.0,
            high: 106.0,
            low: 104.0,
            close: 105.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();

        let first = MarketCandle {
            open_time_ms: 60_000,
            close_time_ms: 119_999,
            open: 105.0,
            high: 112.0,
            low: 99.0,
            close: 110.0,
            volume: 1.0,
        };
        let first_fills = core.process_candle(&first).unwrap();
        assert_eq!(first_fills.len(), 1);
        assert_eq!(first_fills[0].side, crate::storage::OrderSide::Buy);
        assert!((first_fills[0].price - 100.0).abs() < 1e-12);
        assert_eq!(core.portfolio.view().position_quantity, 1.0);

        // The sell order is created from the completed first candle. The first candle
        // traded through 111, but the new order must still remain pending.
        let pending = core.open_orders();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].side, crate::storage::OrderSide::Sell);
        assert_eq!(pending[0].price, Some(111.0));
        assert_eq!(pending[0].submitted_at_ms, first.close_time_ms);

        let history_after_first = storage.trading_run_history(core.run_id).unwrap();
        assert_eq!(history_after_first.fills.len(), 1);
        assert_eq!(history_after_first.positions.len(), 1);
        assert_eq!(history_after_first.equity.len(), 1);
        assert_eq!(history_after_first.order_intents.len(), 2);
        assert_eq!(history_after_first.decisions.len(), 1);
        assert_eq!(
            history_after_first.decisions[0]
                .payload
                .get("seen_position_quantity")
                .and_then(Value::as_f64),
            Some(1.0)
        );

        let fill_sequence = history_after_first.fills[0].event.run_sequence;
        let position_sequence = history_after_first.positions[0].event.run_sequence;
        let decision_sequence = history_after_first.decisions[0].event.run_sequence;
        let new_intent_sequence = history_after_first.order_intents[1].event.run_sequence;
        let equity_sequence = history_after_first.equity[0].event.run_sequence;
        assert!(fill_sequence < position_sequence);
        assert!(position_sequence < decision_sequence);
        assert!(decision_sequence < new_intent_sequence);
        assert!(new_intent_sequence < equity_sequence);
        assert!(
            history_after_first
                .events
                .windows(2)
                .all(|pair| pair[0].run_sequence < pair[1].run_sequence)
        );
        assert!(
            history_after_first.orders[0].order_id
                < history_after_first.orders[1].order_id
        );

        let second = MarketCandle {
            open_time_ms: 120_000,
            close_time_ms: 179_999,
            open: 110.0,
            high: 112.0,
            low: 109.0,
            close: 111.0,
            volume: 1.0,
        };
        let second_fills = core.process_candle(&second).unwrap();
        assert_eq!(second_fills.len(), 1);
        assert_eq!(second_fills[0].side, crate::storage::OrderSide::Sell);
        assert!((second_fills[0].price - 111.0).abs() < 1e-12);
        assert_eq!(core.portfolio.view().position_quantity, 0.0);

        drop(core);
        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn paper_execution_uses_recorded_fee_spread_slippage_latency_and_partial_fill_assumptions() {
        let path = temp_database("execution-assumptions");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let assumptions = ExecutionAssumptions {
            fee_bps: 10.0,
            spread_bps: 10.0,
            slippage_bps: 20.0,
            latency_ms: 60_000,
            limit_fill_policy: crate::trading::LimitFillPolicy::Touch,
            partial_fill_ratio: 0.5,
        };
        let mut core = test_core_with_strategy(
            Arc::clone(&storage),
            "executionassumptions",
            Box::new(MarketOrderOnStart),
            assumptions.clone(),
        );

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 100.0,
            low: 100.0,
            close: 100.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();

        let first = MarketCandle {
            open_time_ms: 60_000,
            close_time_ms: 119_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        assert!(core.process_candle(&first).unwrap().is_empty());

        let second = MarketCandle {
            open_time_ms: 120_000,
            close_time_ms: 179_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        let fills = core.process_candle(&second).unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].status, OrderStatus::PartiallyFilled);
        assert!((fills[0].quantity - 1.0).abs() < 1e-12);
        assert!((fills[0].price - 100.25).abs() < 1e-12);
        assert!((fills[0].fee - 0.10025).abs() < 1e-12);

        let view = core.portfolio.view();
        assert!((view.position_quantity - 1.0).abs() < 1e-12);
        assert!((view.fees_paid - 0.10025).abs() < 1e-12);
        assert_eq!(core.open_orders().len(), 1);
        assert!((core.open_orders()[0].remaining_quantity - 1.0).abs() < 1e-12);

        let run = storage.trading_run(core.run_id).unwrap();
        assert_eq!(
            run.execution_assumptions,
            serde_json::to_value(&assumptions).unwrap()
        );

        drop(core);
        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn paper_limit_fill_policy_uses_trade_through_not_touch_when_configured() {
        let path = temp_database("trade-through");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let assumptions = ExecutionAssumptions {
            limit_fill_policy: crate::trading::LimitFillPolicy::TradeThrough,
            ..ExecutionAssumptions::default()
        };
        let mut core = test_core_with_strategy(
            Arc::clone(&storage),
            "tradethrough",
            Box::new(TouchPolicyProbe),
            assumptions,
        );

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 101.0,
            high: 101.0,
            low: 101.0,
            close: 101.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();

        let touch_only = MarketCandle {
            open_time_ms: 60_000,
            close_time_ms: 119_999,
            open: 101.0,
            high: 102.0,
            low: 100.0,
            close: 101.0,
            volume: 1.0,
        };
        assert!(core.process_candle(&touch_only).unwrap().is_empty());
        assert_eq!(core.open_orders().len(), 1);

        let trades_through = MarketCandle {
            open_time_ms: 120_000,
            close_time_ms: 179_999,
            open: 101.0,
            high: 102.0,
            low: 99.9,
            close: 100.5,
            volume: 1.0,
        };
        let fills = core.process_candle(&trades_through).unwrap();
        assert_eq!(fills.len(), 1);
        assert!((fills[0].price - 100.0).abs() < 1e-12);
        assert!(core.open_orders().is_empty());

        drop(core);
        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn stop_is_idempotent_persists_reason_and_blocks_future_processing() {
        let path = temp_database("stop-idempotent");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let mut core = test_core(Arc::clone(&storage), "stopidempotent");

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();
        core.stop(90_000, "user_stop").unwrap();

        let after_first_stop = storage.trading_run_history(core.run_id).unwrap();
        let event_count = after_first_stop.events.len();
        assert_eq!(after_first_stop.run.status, RunStatus::Stopped);
        assert_eq!(core.status, RunStatus::Stopped);
        assert_eq!(
            after_first_stop.status_events.last().and_then(|event| event.note.as_deref()),
            Some("user_stop")
        );
        assert_eq!(
            after_first_stop
                .status_events
                .iter()
                .filter(|event| event.status == RunStatus::Stopped)
                .count(),
            1
        );

        core.stop(100_000, "user_stop").unwrap();
        let after_second_stop = storage.trading_run_history(core.run_id).unwrap();
        assert_eq!(after_second_stop.events.len(), event_count);
        assert_eq!(
            after_second_stop
                .status_events
                .iter()
                .filter(|event| event.status == RunStatus::Stopped)
                .count(),
            1
        );

        let later_candle = MarketCandle {
            open_time_ms: 120_000,
            close_time_ms: 179_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        assert!(matches!(
            core.process_candle(&later_candle),
            Err(PaperError::RunNotActive(run_id)) if run_id == core.run_id
        ));
        let after_rejected_candle = storage.trading_run_history(core.run_id).unwrap();
        assert_eq!(after_rejected_candle.events.len(), event_count);

        drop(core);
        drop(storage);
        cleanup_database(&path);
    }

    #[tokio::test]
    async fn repeated_manager_stop_returns_persisted_stopped_run_without_runtime_handle() {
        let path = temp_database("manager-stop-idempotent");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let mut core = test_core(Arc::clone(&storage), "managerstop");
        let run_id = core.run_id;

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();
        core.stop(90_000, "user_stop").unwrap();
        drop(core);

        let manager = PaperManager {
            storage: Arc::clone(&storage),
            market: Arc::new(MarketService::new()),
            runtimes: Mutex::new(HashMap::new()),
        };
        let snapshot = manager.stop(run_id).await.unwrap();
        assert_eq!(snapshot.canonical_status, "stopped");
        assert_eq!(snapshot.runtime_status, "stopped");

        let history = storage.trading_run_history(run_id).unwrap();
        assert_eq!(
            history
                .status_events
                .iter()
                .filter(|event| event.status == RunStatus::Stopped)
                .count(),
            1
        );

        drop(storage);
        cleanup_database(&path);
    }

    #[tokio::test]
    async fn dropping_browser_subscription_does_not_signal_or_remove_backend_runtime() {
        let path = temp_database("browser-disconnect");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let mut core = test_core(Arc::clone(&storage), "browserdisconnect");
        let run_id = core.run_id;

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();

        let manager = PaperManager {
            storage: Arc::clone(&storage),
            market: Arc::new(MarketService::new()),
            runtimes: Mutex::new(HashMap::new()),
        };
        let snapshot = manager.persisted_snapshot(run_id).unwrap();
        let (stop_tx, stop_rx) = watch::channel(false);
        let (updates_tx, _) = broadcast::channel(4);
        let handle = Arc::new(PaperRuntimeHandle {
            snapshot: Arc::new(RwLock::new(snapshot)),
            stop_tx,
            updates_tx,
        });
        manager.runtimes.lock().await.insert(run_id, Arc::clone(&handle));

        let (_bootstrap, browser_receiver) = manager.stream_bootstrap(run_id).await.unwrap();
        let browser_receiver = browser_receiver.expect("active runtime receiver");
        drop(browser_receiver);

        assert!(manager.runtimes.lock().await.contains_key(&run_id));
        assert!(!*stop_rx.borrow());

        core.stop(90_000, "test_cleanup").unwrap();
        drop(handle);
        drop(manager);
        drop(core);
        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn service_restart_fails_created_and_running_paper_runs_with_persisted_reason() {
        let path = temp_database("restart-interrupted");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();

        let created = test_core(Arc::clone(&storage), "restartcreated");
        let created_run_id = created.run_id;
        drop(created);

        let mut running = test_core(Arc::clone(&storage), "restartrunning");
        let running_run_id = running.run_id;
        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        running.start(60_000, &previous).unwrap();
        drop(running);

        let failed = terminate_interrupted_paper_runs(&storage, 120_000).unwrap();
        assert_eq!(failed, vec![created_run_id, running_run_id]);

        for run_id in [created_run_id, running_run_id] {
            let history = storage.trading_run_history(run_id).unwrap();
            assert_eq!(history.run.status, RunStatus::Failed);
            assert_eq!(history.run.ended_at_ms, Some(120_000));
            assert_eq!(
                history.status_events.last().and_then(|event| event.note.as_deref()),
                Some(SERVICE_RESTART_FAILURE_REASON)
            );
        }

        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn service_restart_leaves_terminal_paper_runs_unchanged() {
        let path = temp_database("restart-terminal");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();

        let mut stopped = test_core(Arc::clone(&storage), "restartstopped");
        let run_id = stopped.run_id;
        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        stopped.start(60_000, &previous).unwrap();
        stopped.stop(90_000, "user_stop").unwrap();
        drop(stopped);

        let before = storage.trading_run_history(run_id).unwrap();
        let before_event_count = before.events.len();

        let failed = terminate_interrupted_paper_runs(&storage, 120_000).unwrap();
        assert!(failed.is_empty());

        let after = storage.trading_run_history(run_id).unwrap();
        assert_eq!(after.run.status, RunStatus::Stopped);
        assert_eq!(after.events.len(), before_event_count);
        assert_eq!(
            after.status_events.last().and_then(|event| event.note.as_deref()),
            Some("user_stop")
        );

        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn persisted_snapshot_restores_canonical_config_and_financial_state_without_browser_state() {
        let path = temp_database("snapshot-contract");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let mut core = test_core_with_strategy(
            Arc::clone(&storage),
            "snapshotcontract",
            Box::new(MarketOrderOnStart),
            ExecutionAssumptions::default(),
        );
        let run_id = core.run_id;

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();
        let candle = MarketCandle {
            open_time_ms: 60_000,
            close_time_ms: 119_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            volume: 1.0,
        };
        core.process_candle(&candle).unwrap();
        core.stop(120_000, "user_stop").unwrap();
        drop(core);

        let manager = PaperManager {
            storage: Arc::clone(&storage),
            market: Arc::new(MarketService::new()),
            runtimes: Mutex::new(HashMap::new()),
        };
        let snapshot = manager.persisted_snapshot(run_id).unwrap();

        assert_eq!(snapshot.run_id, run_id);
        assert_eq!(snapshot.mode, "paper");
        assert_eq!(snapshot.canonical_status, "stopped");
        assert!(!snapshot.runtime_active);
        assert_eq!(snapshot.started_at_ms, Some(60_000));
        assert_eq!(snapshot.ended_at_ms, Some(120_000));
        assert_eq!(snapshot.strategy_id, "test-paper-market-order");
        assert_eq!(snapshot.run_config.get("test").and_then(Value::as_str), Some("phase_4_3"));
        assert_eq!(snapshot.data_source.get("kind").and_then(Value::as_str), Some("test"));
        assert_eq!(snapshot.recent_fills.len(), 1);
        assert_eq!(snapshot.recent_fills[0].order_id, 1);
        assert!((snapshot.portfolio.position_quantity - 2.0).abs() < 1e-12);
        assert!((snapshot.portfolio.equity - 1001.0).abs() < 1e-12);
        assert_eq!(snapshot.feed_status, FeedStatus::Loading);
        assert!(snapshot.best_bid.is_none());
        assert!(snapshot.latest_base_candle.is_none());

        drop(manager);
        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn market_snapshot_fields_are_synchronized_for_ui_refresh_state() {
        let mut snapshot = PaperSnapshot {
            run_id: 1,
            stream_revision: 1,
            comparison_id: None,
            mode: "paper".into(),
            runtime_status: "running".into(),
            canonical_status: "running".into(),
            runtime_active: true,
            created_at_ms: 1,
            started_at_ms: Some(2),
            ended_at_ms: None,
            symbol: "BTCUSDT".into(),
            market_type: "spot".into(),
            replay_interval: "1m".into(),
            strategy_id: "static-grid-fixture".into(),
            strategy_version: "1".into(),
            strategy_params: json!({}),
            run_config: json!({}),
            data_source: json!({}),
            execution_assumptions: json!({}),
            initial_capital: "1000".into(),
            arming_boundary_ms: 60_000,
            feed_status: FeedStatus::Loading,
            best_bid: None,
            best_ask: None,
            mid_price: None,
            latest_base_candle: None,
            portfolio: PortfolioState::new(1000.0).unwrap().view(),
            open_orders: Vec::new(),
            recent_fills: Vec::new(),
            recent_events: Vec::new(),
            latest_replay_candle: None,
            last_base_candle_open_ms: None,
            updated_at_ms: 0,
        };

        let current = Candle {
            open_time: 120_000,
            close_time: 179_999,
            open: 100.0,
            high: 102.0,
            low: 99.0,
            close: 101.0,
            volume: 3.0,
            is_closed: false,
        };
        let market = MarketSnapshot {
            symbol: "BTCUSDT".into(),
            interval: "1m".into(),
            status: FeedStatus::Live,
            candles: vec![current.clone()],
            quote: crate::market::MarketQuote {
                update_id: Some(7),
                best_bid: Some(100.9),
                best_bid_quantity: Some(2.0),
                best_ask: Some(101.1),
                best_ask_quantity: Some(3.0),
                spread: Some(0.2),
                mid_price: Some(101.0),
            },
            order_book: crate::market::OrderBookSnapshot::default(),
            trades: Vec::new(),
        };

        apply_market_snapshot(&mut snapshot, &market);
        assert_eq!(snapshot.feed_status, FeedStatus::Live);
        assert_eq!(snapshot.best_bid, Some(100.9));
        assert_eq!(snapshot.best_ask, Some(101.1));
        assert_eq!(snapshot.mid_price, Some(101.0));
        let candle = snapshot.latest_base_candle.as_ref().expect("latest base candle");
        assert_eq!(candle.open_time_ms, 120_000);
        assert!(!candle.is_closed);
        assert_eq!(candle.close, 101.0);

        let chart = paper_chart_snapshot(&snapshot, Some(&market), Vec::new(), Vec::new());
        assert_eq!(chart.run_id, 1);
        assert_eq!(chart.base_interval, "1m");
        assert_eq!(chart.replay_interval, "1m");
        assert_eq!(chart.candles.len(), 1);
        assert_eq!(chart.candles[0].open_time_ms, 120_000);
        assert!(chart.order_levels.is_empty());
        assert!(chart.fills.is_empty());
    }

    #[tokio::test]
    async fn stream_bootstrap_is_snapshot_first_and_revisions_are_monotonic() {
        let path = temp_database("stream-bootstrap");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let mut core = test_core(Arc::clone(&storage), "streambootstrap");
        let run_id = core.run_id;

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();

        let mut snapshot = PaperSnapshot {
            run_id,
            stream_revision: 7,
            comparison_id: None,
            mode: "paper".into(),
            runtime_status: "running".into(),
            canonical_status: "running".into(),
            runtime_active: true,
            created_at_ms: 1,
            started_at_ms: Some(60_000),
            ended_at_ms: None,
            symbol: "BTCUSDT".into(),
            market_type: "spot".into(),
            replay_interval: "1m".into(),
            strategy_id: "test-paper-noop".into(),
            strategy_version: "1".into(),
            strategy_params: json!({}),
            run_config: json!({}),
            data_source: json!({}),
            execution_assumptions: json!({}),
            initial_capital: "1000".into(),
            arming_boundary_ms: 60_000,
            feed_status: FeedStatus::Live,
            best_bid: None,
            best_ask: None,
            mid_price: None,
            latest_base_candle: None,
            portfolio: core.portfolio.view(),
            open_orders: core.open_orders(),
            recent_fills: Vec::new(),
            recent_events: Vec::new(),
            latest_replay_candle: None,
            last_base_candle_open_ms: None,
            updated_at_ms: 60_000,
        };
        let (stop_tx, _stop_rx) = watch::channel(false);
        let (updates_tx, _) = broadcast::channel(8);
        let handle = Arc::new(PaperRuntimeHandle {
            snapshot: Arc::new(RwLock::new(snapshot.clone())),
            stop_tx,
            updates_tx,
        });
        let manager = PaperManager {
            storage: Arc::clone(&storage),
            market: Arc::new(MarketService::new()),
            runtimes: Mutex::new(HashMap::from([(run_id, Arc::clone(&handle))])),
        };

        let (bootstrap, mut receiver) = manager.stream_bootstrap(run_id).await.unwrap();
        assert_eq!(bootstrap.stream_revision, 7);
        assert_eq!(bootstrap.runtime_status, "running");

        snapshot.portfolio.cash = 999.0;
        manager
            .update_snapshot(
                &handle,
                snapshot.portfolio.clone(),
                snapshot.open_orders.clone(),
                |next| next.updated_at_ms = 61_000,
            )
            .await;

        let update = receiver
            .as_mut()
            .expect("active runtime receiver")
            .recv()
            .await
            .unwrap();
        assert_eq!(update.stream_revision, 8);
        assert_eq!(update.updated_at_ms, 61_000);
        assert_eq!(update.portfolio.cash, 999.0);

        core.stop(90_000, "test_cleanup").unwrap();
        drop(manager);
        drop(core);
        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn paper_core_uses_canonical_run_lifecycle() {
        let path = temp_database("lifecycle");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let mut core = test_core(Arc::clone(&storage), "lifecycle");

        assert_eq!(storage.trading_run(core.run_id).unwrap().status, RunStatus::Created);

        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 1.0,
        };
        core.start(60_000, &previous).unwrap();
        assert_eq!(storage.trading_run(core.run_id).unwrap().status, RunStatus::Running);

        core.stop(120_000, "phase_4_1_test_stop").unwrap();
        let history = storage.trading_run_history(core.run_id).unwrap();
        assert_eq!(history.run.status, RunStatus::Stopped);
        assert_eq!(
            history.status_events.iter().map(|event| event.status).collect::<Vec<_>>(),
            vec![RunStatus::Created, RunStatus::Running, RunStatus::Stopped]
        );

        drop(core);
        drop(storage);
        cleanup_database(&path);
    }

    #[test]
    fn paper_run_cores_keep_state_isolated_by_run() {
        let path = temp_database("isolation");
        let storage = Arc::new(StorageReader::new(path.clone()));
        storage.initialize().unwrap();
        let mut first = test_core(Arc::clone(&storage), "first");
        let second = test_core(Arc::clone(&storage), "second");

        assert_ne!(first.run_id, second.run_id);
        first
            .portfolio
            .apply_fill(crate::storage::OrderSide::Buy, 2.0, 100.0, 1.0)
            .unwrap();

        assert_eq!(first.portfolio.view().position_quantity, 2.0);
        assert_eq!(second.portfolio.view().position_quantity, 0.0);
        assert_eq!(second.portfolio.view().cash, 1000.0);

        drop(first);
        drop(second);
        drop(storage);
        cleanup_database(&path);
    }
}
