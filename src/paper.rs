use crate::{
    market::{Candle, FeedStatus, MarketError, MarketKey, MarketService, MarketType},
    storage::{
        CreateOrderInput, DecisionInput, EquitySnapshotInput, EventTimes, ExactDecimal, FillInput,
        LiquidityRole, OrderIntentInput, OrderStateInput, OrderStatus, OrderType,
        PositionSnapshotInput, RunMode, RunStatus, StorageError, StorageReader,
        TradingRunSpec,
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
pub struct PaperSnapshot {
    pub run_id: i64,
    pub mode: String,
    pub runtime_status: String,
    pub canonical_status: String,
    pub symbol: String,
    pub market_type: String,
    pub replay_interval: String,
    pub strategy_id: String,
    pub strategy_version: String,
    pub strategy_params: Value,
    pub execution_assumptions: Value,
    pub initial_capital: String,
    pub arming_boundary_ms: i64,
    pub feed_status: FeedStatus,
    pub portfolio: PortfolioView,
    pub open_orders: Vec<PaperOpenOrderView>,
    pub recent_fills: Vec<PaperFillView>,
    pub recent_events: Vec<PaperRuntimeEvent>,
    pub latest_replay_candle: Option<MarketCandle>,
    pub last_base_candle_open_ms: Option<i64>,
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
        let now = system_now_ms();
        for run in storage.unfinished_trading_runs(RunMode::Paper)? {
            storage.set_trading_run_status(
                run.run_id,
                RunStatus::Failed,
                EventTimes::new(now),
                Some("service_restart_runtime_continuity_not_provable"),
            )?;
        }

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
        };

        let initial_snapshot = PaperSnapshot {
            run_id: run.run_id,
            mode: "paper".into(),
            runtime_status: "arming".into(),
            canonical_status: "created".into(),
            symbol: symbol.clone(),
            market_type: config.market_type.as_str().into(),
            replay_interval: config.replay_interval.as_str().into(),
            strategy_id,
            strategy_version,
            strategy_params,
            execution_assumptions: execution_json,
            initial_capital: config.initial_capital.as_str().to_string(),
            arming_boundary_ms: boundary,
            feed_status: market_snapshot.status,
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
        let handle = self
            .runtimes
            .lock()
            .await
            .get(&run_id)
            .cloned()
            .ok_or(PaperError::RunNotActive(run_id))?;
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

    pub async fn subscribe(
        &self,
        run_id: i64,
    ) -> Result<Option<broadcast::Receiver<PaperSnapshot>>, PaperError> {
        let handle = self.runtimes.lock().await.get(&run_id).cloned();
        Ok(handle.map(|handle| handle.updates_tx.subscribe()))
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

        let recent_fills = fills
            .into_iter()
            .rev()
            .take(MAX_RECENT_RUNTIME_FILLS)
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

        Ok(PaperSnapshot {
            run_id,
            mode: "paper".into(),
            runtime_status: run.status.as_str().into(),
            canonical_status: run.status.as_str().into(),
            symbol: json_string(&run.data_source, "symbol"),
            market_type: json_string(&run.data_source, "market_type"),
            replay_interval: json_string(&run.data_source, "replay_interval"),
            strategy_id: run.strategy_id,
            strategy_version: run.strategy_version,
            strategy_params: run.strategy_params,
            execution_assumptions: run.execution_assumptions,
            initial_capital: run.initial_capital.as_str().to_string(),
            arming_boundary_ms: json_i64(&run.data_source, "arming_boundary_ms").unwrap_or(run.created_at_ms),
            feed_status: FeedStatus::Loading,
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
                changed = stop_rx.changed() => {
                    if changed.is_err() || *stop_rx.borrow() {
                        let now = system_now_ms();
                        let result = core.stop(now, "user_stop");
                        match result {
                            Ok(()) => {
                                self.update_snapshot(&handle, &core, |snapshot| {
                                    snapshot.runtime_status = "stopped".into();
                                    snapshot.canonical_status = "stopped".into();
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

                    self.update_snapshot(&handle, &core, |snapshot| {
                        snapshot.feed_status = market_snapshot.status;
                        snapshot.updated_at_ms = now;
                    }).await;

                    if !started {
                        if now < boundary {
                            continue;
                        }
                        match self.previous_replay_candle(&bootstrap_key, boundary, config.replay_interval).await {
                            Ok(Some(previous)) => {
                                match core.start(boundary, &previous) {
                                    Ok(()) => {
                                        started = true;
                                        self.update_snapshot(&handle, &core, |snapshot| {
                                            snapshot.runtime_status = "running".into();
                                            snapshot.canonical_status = "running".into();
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
                    let mut candidates: Vec<_> = market_snapshot
                        .candles
                        .iter()
                        .filter(|candle| candle.is_closed && candle.open_time >= expected)
                        .cloned()
                        .collect();
                    candidates.sort_by_key(|candle| candle.open_time);

                    for candle in candidates {
                        if candle.open_time > aggregator.expected_base_open_ms() {
                            self.finish_failed(
                                &handle,
                                &mut core,
                                format!(
                                    "market-data gap: expected closed 1m candle at {}, received {}",
                                    aggregator.expected_base_open_ms(),
                                    candle.open_time
                                ),
                            ).await;
                            return;
                        }
                        if candle.open_time < aggregator.expected_base_open_ms() {
                            continue;
                        }

                        let completed = match aggregator.push(&candle) {
                            Ok(completed) => completed,
                            Err(error) => {
                                self.finish_failed(&handle, &mut core, error).await;
                                return;
                            }
                        };

                        self.update_snapshot(&handle, &core, |snapshot| {
                            snapshot.last_base_candle_open_ms = Some(candle.open_time);
                            snapshot.updated_at_ms = system_now_ms();
                        }).await;

                        if let Some(replay_candle) = completed {
                            match core.process_candle(&replay_candle) {
                                Ok(fills) => {
                                    self.update_snapshot(&handle, &core, |snapshot| {
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
        let target_open = boundary.saturating_sub(interval.duration_ms());
        let snapshot = self.market.snapshot_for(key.clone()).await?;
        Ok(snapshot
            .candles
            .iter()
            .rev()
            .find(|candle| candle.is_closed && candle.open_time == target_open)
            .map(market_candle))
    }

    async fn update_snapshot(
        &self,
        handle: &PaperRuntimeHandle,
        core: &PaperRunCore,
        update: impl FnOnce(&mut PaperSnapshot),
    ) {
        let mut snapshot = handle.snapshot.write().await;
        snapshot.portfolio = core.portfolio.view();
        snapshot.open_orders = core.open_orders();
        update(&mut snapshot);
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
        self.update_snapshot(handle, core, |snapshot| {
            snapshot.runtime_status = "failed".into();
            snapshot.canonical_status = "failed".into();
            snapshot.updated_at_ms = now;
            push_runtime_event(snapshot, now, "failed", &reason);
        }).await;
    }
}

struct PaperRunCore {
    run_id: i64,
    storage: Arc<StorageReader>,
    strategy: Box<dyn Strategy + Send>,
    portfolio: PortfolioState,
    execution: SimulatedExecution,
}

impl PaperRunCore {
    fn start(&mut self, start_time_ms: i64, previous: &MarketCandle) -> Result<(), PaperError> {
        self.storage.set_trading_run_status(
            self.run_id,
            RunStatus::Running,
            paper_times(start_time_ms),
            None,
        )?;
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
        self.expire_pending(event_time_ms, reason)?;
        let run = self.storage.trading_run(self.run_id)?;
        if matches!(run.status, RunStatus::Created | RunStatus::Running) {
            self.storage.set_trading_run_status(
                self.run_id,
                RunStatus::Stopped,
                EventTimes::new(event_time_ms),
                Some(reason),
            )?;
        }
        Ok(())
    }

    fn fail(&mut self, event_time_ms: i64, reason: &str) -> Result<(), PaperError> {
        self.expire_pending(event_time_ms, reason)?;
        let run = self.storage.trading_run(self.run_id)?;
        if matches!(run.status, RunStatus::Created | RunStatus::Running) {
            self.storage.set_trading_run_status(
                self.run_id,
                RunStatus::Failed,
                EventTimes::new(event_time_ms),
                Some(reason),
            )?;
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
}
