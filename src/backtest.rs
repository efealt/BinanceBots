use crate::{
    storage::{
        CreateOrderInput, DecisionInput, EquitySnapshotInput, EventTimes, ExactDecimal, FillInput,
        HistoricalDatasetInfo, LiquidityRole, OrderIntentInput, OrderStateInput, OrderStatus,
        PositionSnapshotInput, RunMode, RunStatus, StorageError, StorageReader, TradingRunSpec,
    },
    trading::{
        decimal_string, ExecutionAssumptions, MarketCandle, PortfolioState, PortfolioView,
        SimulatedExecution, Strategy, StrategyContext, StrategyOutput, StrategyStartContext,
        TradingInterval,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;
use thiserror::Error;

const EQUITY_BATCH_SIZE: usize = 2_048;

pub type ReplayInterval = TradingInterval;

#[derive(Clone, Debug)]
pub struct BacktestRunConfig {
    pub dataset_id: i64,
    pub replay_interval: ReplayInterval,
    pub start_time_ms: Option<i64>,
    pub end_time_ms: Option<i64>,
    pub comparison_id: Option<String>,
    pub initial_capital: ExactDecimal,
    pub execution: ExecutionAssumptions,
    pub run_config: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct BacktestRunResult {
    pub run_id: i64,
    pub dataset_id: i64,
    pub replay_interval: String,
    pub candles_processed: usize,
    pub effective_start_time_ms: i64,
    pub effective_end_time_ms: i64,
    pub previous_candle_open_time_ms: Option<i64>,
    pub reserved_first_candle_as_preroll: bool,
    pub final_portfolio: PortfolioView,
}

struct PreparedReplay {
    dataset: HistoricalDatasetInfo,
    candles: Vec<crate::storage::OhlcvCandle>,
    previous_candle: Option<MarketCandle>,
    reserved_first_candle_as_preroll: bool,
}

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("invalid backtest configuration: {0}")]
    InvalidConfig(String),
    #[error("strategy error: {0}")]
    Strategy(String),
    #[error("simulation error: {0}")]
    Simulation(String),
}

#[derive(Clone)]
pub struct BacktestEngine {
    storage: Arc<StorageReader>,
}

impl BacktestEngine {
    pub fn new(storage: Arc<StorageReader>) -> Self {
        Self { storage }
    }

    pub fn run<S: Strategy>(
        &self,
        config: &BacktestRunConfig,
        strategy: &mut S,
    ) -> Result<BacktestRunResult, BacktestError> {
        self.run_with_progress(config, strategy, |_| {})
    }

    pub fn run_with_progress<S: Strategy, F: FnMut(u8)>(
        &self,
        config: &BacktestRunConfig,
        strategy: &mut S,
        mut progress: F,
    ) -> Result<BacktestRunResult, BacktestError> {
        progress(0);
        validate_config(config)?;
        let prepared = self.prepare_replay(config, strategy)?;
        progress(2);

        let dataset = &prepared.dataset;
        let candles = &prepared.candles;
        let first = candles.first().expect("prepared replay has active candles");
        let last = candles.last().expect("prepared replay has active candles");
        let strategy_params = strategy.parameters();
        let execution_json = serde_json::to_value(&config.execution)
            .map_err(|error| BacktestError::InvalidConfig(error.to_string()))?;
        let run_spec = TradingRunSpec {
            comparison_id: config.comparison_id.clone(),
            mode: RunMode::Backtest,
            strategy_id: strategy.id().to_string(),
            strategy_version: strategy.version().to_string(),
            strategy_params,
            instrument_id: dataset.instrument_id,
            initial_capital: config.initial_capital.clone(),
            run_config: config.run_config.clone(),
            data_source: json!({
                "kind": "historical_dataset",
                "dataset_id": dataset.dataset_id,
                "venue": dataset.venue,
                "symbol": dataset.symbol,
                "market_type": dataset.market_type,
                "source_interval": dataset.interval,
                "replay_interval": config.replay_interval.as_str(),
                "source": dataset.source,
                "requested_start_time_ms": config.start_time_ms,
                "requested_end_time_ms": config.end_time_ms,
                "effective_first_open_time_ms": first.open_time_ms,
                "effective_last_close_time_ms": last.close_time_ms,
                "previous_candle_open_time_ms": prepared.previous_candle.as_ref().map(|candle| candle.open_time_ms),
                "reserved_first_candle_as_preroll": prepared.reserved_first_candle_as_preroll
            }),
            execution_assumptions: execution_json,
        };
        let run = self.storage.create_trading_run(&run_spec)?;
        self.storage.set_trading_run_status(
            run.run_id,
            RunStatus::Running,
            EventTimes::new(first.open_time_ms),
            None,
        )?;

        let initial_cash = config
            .initial_capital
            .as_str()
            .parse::<f64>()
            .map_err(|_| BacktestError::InvalidConfig("initial capital cannot be represented by simulator".into()))?;
        let mut portfolio = PortfolioState::new(initial_cash).map_err(BacktestError::Simulation)?;
        let mut execution =
            SimulatedExecution::new(config.execution.clone()).map_err(BacktestError::Simulation)?;
        let mut failure_time = first.open_time_ms;

        let result = self.execute(
            run.run_id,
            dataset,
            candles,
            prepared.previous_candle.as_ref(),
            strategy,
            &mut portfolio,
            &mut execution,
            &mut failure_time,
            &mut progress,
        );

        match result {
            Ok(()) => {
                let final_time = last.close_time_ms;
                for pending in execution.expire_all() {
                    let filled = pending.original_quantity - pending.remaining_quantity;
                    self.storage.record_order_state(&OrderStateInput {
                        order_id: pending.order_id,
                        times: EventTimes::new(final_time),
                        status: OrderStatus::Expired,
                        filled_quantity: exact(filled)?,
                        average_fill_price: if pending.filled_quantity > 0.0 {
                            Some(exact(pending.filled_notional / pending.filled_quantity)?)
                        } else {
                            None
                        },
                        reject_reason: None,
                        metadata: json!({"reason": "backtest_range_ended"}),
                    })?;
                }
                self.storage.set_trading_run_status(
                    run.run_id,
                    RunStatus::Completed,
                    EventTimes::new(final_time),
                    None,
                )?;
                progress(100);
                Ok(BacktestRunResult {
                    run_id: run.run_id,
                    dataset_id: config.dataset_id,
                    replay_interval: config.replay_interval.as_str().to_string(),
                    candles_processed: candles.len(),
                    effective_start_time_ms: first.open_time_ms,
                    effective_end_time_ms: last.close_time_ms,
                    previous_candle_open_time_ms: prepared.previous_candle.as_ref().map(|candle| candle.open_time_ms),
                    reserved_first_candle_as_preroll: prepared.reserved_first_candle_as_preroll,
                    final_portfolio: portfolio.view(),
                })
            }
            Err(error) => {
                let note = error.to_string();
                let _ = self.storage.set_trading_run_status(
                    run.run_id,
                    RunStatus::Failed,
                    EventTimes::new(failure_time),
                    Some(&note),
                );
                Err(error)
            }
        }
    }

    fn prepare_replay<S: Strategy>(
        &self,
        config: &BacktestRunConfig,
        strategy: &S,
    ) -> Result<PreparedReplay, BacktestError> {
        let dataset = self.storage.historical_dataset_info(config.dataset_id)?;
        validate_replay_interval(&dataset.interval, config.replay_interval)?;

        let preload_start = config
            .start_time_ms
            .map(|start| start.saturating_sub(config.replay_interval.duration_ms()));
        let source_candles = self.storage.ohlcv_series_between(
            config.dataset_id,
            preload_start,
            config.end_time_ms,
        )?;
        if source_candles.is_empty() {
            return Err(BacktestError::InvalidConfig(
                "selected dataset/range contains no completed candles".into(),
            ));
        }
        validate_candle_order(&source_candles)?;
        let replay_candles = aggregate_candles(&source_candles, config.replay_interval)?;
        if replay_candles.is_empty() {
            return Err(BacktestError::InvalidConfig(
                "selected dataset/range contains no replay candles".into(),
            ));
        }

        let requested_start = config.start_time_ms.unwrap_or(i64::MIN);
        let active_index = replay_candles
            .iter()
            .position(|candle| candle.open_time_ms >= requested_start)
            .unwrap_or(replay_candles.len());
        if active_index >= replay_candles.len() {
            return Err(BacktestError::InvalidConfig(
                "selected start time is after the available replay candles".into(),
            ));
        }

        let mut previous_candle = active_index
            .checked_sub(1)
            .and_then(|index| replay_candles.get(index))
            .map(market_candle_from_storage);
        let mut active = replay_candles[active_index..].to_vec();
        let mut reserved_first_candle_as_preroll = false;

        if previous_candle.is_none() && strategy.requires_previous_candle() {
            if active.len() < 2 {
                return Err(BacktestError::InvalidConfig(
                    "strategy requires a previous completed replay candle; selected range is too short".into(),
                ));
            }
            let pre_roll = active.remove(0);
            previous_candle = Some(market_candle_from_storage(&pre_roll));
            reserved_first_candle_as_preroll = true;
        }

        if active.is_empty() {
            return Err(BacktestError::InvalidConfig(
                "selected range contains no active replay candles after pre-roll".into(),
            ));
        }

        Ok(PreparedReplay {
            dataset,
            candles: active,
            previous_candle,
            reserved_first_candle_as_preroll,
        })
    }

    fn execute<S: Strategy>(
        &self,
        run_id: i64,
        _dataset: &HistoricalDatasetInfo,
        candles: &[crate::storage::OhlcvCandle],
        previous_candle: Option<&MarketCandle>,
        strategy: &mut S,
        portfolio: &mut PortfolioState,
        execution: &mut SimulatedExecution,
        failure_time: &mut i64,
        progress: &mut impl FnMut(u8),
    ) -> Result<(), BacktestError> {
        let first = candles.first().expect("execute receives non-empty candles");
        let start_context = StrategyStartContext {
            now_ms: first.open_time_ms,
            previous_candle,
            portfolio: portfolio.view(),
        };
        let start_output = strategy
            .on_start(&start_context)
            .map_err(BacktestError::Strategy)?;
        self.persist_strategy_output(
            run_id,
            first.open_time_ms,
            start_output,
            execution,
        )?;

        let mut equity_buffer = Vec::with_capacity(EQUITY_BATCH_SIZE);

        let mut last_progress = 2_u8;
        for (index, candle) in candles.iter().enumerate() {
            *failure_time = candle.close_time_ms;
            let market_candle = market_candle_from_storage(candle);

            let fills = match execution.process_candle(&market_candle) {
                Ok(fills) => fills,
                Err(error) => {
                    self.flush_equity_buffer(&mut equity_buffer)?;
                    return Err(BacktestError::Simulation(error));
                }
            };
            if !fills.is_empty() {
                self.flush_equity_buffer(&mut equity_buffer)?;
            }

            for fill in fills {
                self.storage.record_fill(&FillInput {
                    order_id: fill.order_id,
                    times: EventTimes::new(fill.event_time_ms),
                    exchange_trade_id: None,
                    price: exact(fill.price)?,
                    quantity: exact(fill.quantity)?,
                    fee: Some(exact(fill.fee)?),
                    fee_asset: Some("QUOTE".into()),
                    liquidity_role: Some(match fill.order_type {
                        crate::storage::OrderType::Market => LiquidityRole::Taker,
                        crate::storage::OrderType::Limit => LiquidityRole::Maker,
                        _ => LiquidityRole::Taker,
                    }),
                    metadata: json!({"simulated": true}),
                })?;

                portfolio
                    .apply_fill(fill.side, fill.quantity, fill.price, fill.fee)
                    .map_err(BacktestError::Simulation)?;

                self.storage.record_order_state(&OrderStateInput {
                    order_id: fill.order_id,
                    times: EventTimes::new(fill.event_time_ms),
                    status: fill.status,
                    filled_quantity: exact(fill.cumulative_filled_quantity)?,
                    average_fill_price: Some(exact(fill.average_fill_price)?),
                    reject_reason: None,
                    metadata: json!({"simulated": true}),
                })?;

                portfolio.mark(fill.price).map_err(BacktestError::Simulation)?;
                let view = portfolio.view();
                self.storage.record_position_snapshot(&PositionSnapshotInput {
                    run_id,
                    times: EventTimes::new(fill.event_time_ms),
                    position_quantity: exact(view.position_quantity)?,
                    average_entry_price: optional_positive_exact(view.average_entry_price)?,
                    mark_price: optional_positive_exact(fill.price)?,
                    realized_pnl: Some(exact(view.realized_pnl)?),
                    unrealized_pnl: Some(exact(view.unrealized_pnl)?),
                    cash_balance: Some(exact(view.cash)?),
                    metadata: json!({"simulated": true}),
                })?;
            }

            portfolio
                .mark(market_candle.close)
                .map_err(BacktestError::Simulation)?;
            let context = StrategyContext {
                now_ms: market_candle.close_time_ms,
                candle: &market_candle,
                portfolio: portfolio.view(),
            };
            let output = match strategy.on_candle(&context) {
                Ok(output) => output,
                Err(error) => {
                    self.flush_equity_buffer(&mut equity_buffer)?;
                    return Err(BacktestError::Strategy(error));
                }
            };

            if !output.decisions.is_empty() || !output.order_intents.is_empty() {
                self.flush_equity_buffer(&mut equity_buffer)?;
                self.persist_strategy_output(
                    run_id,
                    market_candle.close_time_ms,
                    output,
                    execution,
                )?;
            }

            let view = portfolio.view();
            equity_buffer.push(EquitySnapshotInput {
                run_id,
                times: EventTimes::new(market_candle.close_time_ms),
                equity: exact(view.equity)?,
                cash_balance: Some(exact(view.cash)?),
                realized_pnl: Some(exact(view.realized_pnl)?),
                unrealized_pnl: Some(exact(view.unrealized_pnl)?),
                fees_paid: Some(exact(view.fees_paid)?),
                metadata: json!({"mark_price": decimal_string(market_candle.close).map_err(BacktestError::Simulation)?}),
            });
            if equity_buffer.len() >= EQUITY_BATCH_SIZE {
                self.flush_equity_buffer(&mut equity_buffer)?;
            }

            let completed = index + 1;
            let next_progress = 2 + ((completed * 96) / candles.len()) as u8;
            if next_progress > last_progress {
                last_progress = next_progress;
                progress(next_progress.min(98));
            }
        }

        self.flush_equity_buffer(&mut equity_buffer)?;
        Ok(())
    }

    fn flush_equity_buffer(
        &self,
        buffer: &mut Vec<EquitySnapshotInput>,
    ) -> Result<(), BacktestError> {
        if !buffer.is_empty() {
            self.storage.record_equity_snapshots(buffer)?;
            buffer.clear();
        }
        Ok(())
    }

    fn persist_strategy_output(
        &self,
        run_id: i64,
        event_time_ms: i64,
        output: StrategyOutput,
        execution: &mut SimulatedExecution,
    ) -> Result<(), BacktestError> {
        for decision in output.decisions {
            self.storage.record_decision(&DecisionInput {
                run_id,
                times: EventTimes::new(event_time_ms),
                decision_type: decision.decision_type,
                payload: decision.payload,
            })?;
        }

        for intent in output.order_intents {
            validate_intent(&intent)?;
            let persisted_intent = self.storage.record_order_intent(&OrderIntentInput {
                run_id,
                times: EventTimes::new(event_time_ms),
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
                run_id,
                times: EventTimes::new(event_time_ms),
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
                times: EventTimes::new(event_time_ms),
                status: OrderStatus::Accepted,
                filled_quantity: ExactDecimal::zero(),
                average_fill_price: None,
                reject_reason: None,
                metadata: json!({"simulated": true}),
            })?;
            execution
                .submit(order.order_id, event_time_ms, intent)
                .map_err(BacktestError::Simulation)?;
        }
        Ok(())
    }
}

fn market_candle_from_storage(candle: &crate::storage::OhlcvCandle) -> MarketCandle {
    MarketCandle {
        open_time_ms: candle.open_time_ms,
        close_time_ms: candle.close_time_ms,
        open: candle.open_price,
        high: candle.high_price,
        low: candle.low_price,
        close: candle.close_price,
        volume: candle.base_volume,
    }
}

fn validate_replay_interval(source_interval: &str, replay_interval: ReplayInterval) -> Result<(), BacktestError> {
    let source_ms = match source_interval {
        "1m" => 60_000,
        "1h" => 3_600_000,
        "1d" => 86_400_000,
        other => {
            return Err(BacktestError::InvalidConfig(format!(
                "unsupported stored dataset interval: {other}"
            )));
        }
    };
    let replay_ms = replay_interval.duration_ms();
    if replay_ms < source_ms || replay_ms % source_ms != 0 {
        return Err(BacktestError::InvalidConfig(format!(
            "replay interval {} cannot be derived from stored interval {source_interval}",
            replay_interval.as_str()
        )));
    }
    Ok(())
}

fn aggregate_candles(
    source: &[crate::storage::OhlcvCandle],
    interval: ReplayInterval,
) -> Result<Vec<crate::storage::OhlcvCandle>, BacktestError> {
    let interval_ms = interval.duration_ms();
    let mut aggregated: Vec<crate::storage::OhlcvCandle> = Vec::new();

    for candle in source {
        let bucket_open = candle.open_time_ms.div_euclid(interval_ms) * interval_ms;
        if let Some(current) = aggregated.last_mut()
            && current.open_time_ms == bucket_open
        {
            current.close_time_ms = candle.close_time_ms;
            current.high_price = current.high_price.max(candle.high_price);
            current.low_price = current.low_price.min(candle.low_price);
            current.close_price = candle.close_price;
            current.base_volume += candle.base_volume;
            current.quote_volume = sum_optional(current.quote_volume, candle.quote_volume);
            current.trade_count = sum_optional_i64(current.trade_count, candle.trade_count);
            current.taker_buy_base_volume =
                sum_optional(current.taker_buy_base_volume, candle.taker_buy_base_volume);
            current.taker_buy_quote_volume =
                sum_optional(current.taker_buy_quote_volume, candle.taker_buy_quote_volume);
            continue;
        }

        aggregated.push(crate::storage::OhlcvCandle {
            open_time_ms: bucket_open,
            close_time_ms: candle.close_time_ms,
            open_price: candle.open_price,
            high_price: candle.high_price,
            low_price: candle.low_price,
            close_price: candle.close_price,
            base_volume: candle.base_volume,
            quote_volume: candle.quote_volume,
            trade_count: candle.trade_count,
            taker_buy_base_volume: candle.taker_buy_base_volume,
            taker_buy_quote_volume: candle.taker_buy_quote_volume,
        });
    }

    validate_candle_order(&aggregated)?;
    Ok(aggregated)
}

fn sum_optional(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        _ => None,
    }
}

fn sum_optional_i64(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        _ => None,
    }
}

fn validate_config(config: &BacktestRunConfig) -> Result<(), BacktestError> {
    if config.dataset_id <= 0 {
        return Err(BacktestError::InvalidConfig("dataset_id must be positive".into()));
    }
    if let (Some(start), Some(end)) = (config.start_time_ms, config.end_time_ms)
        && start > end
    {
        return Err(BacktestError::InvalidConfig("start_time_ms must be <= end_time_ms".into()));
    }
    config.execution.validate().map_err(BacktestError::InvalidConfig)?;
    Ok(())
}

fn validate_candle_order(candles: &[crate::storage::OhlcvCandle]) -> Result<(), BacktestError> {
    for candle in candles {
        if candle.close_time_ms < candle.open_time_ms {
            return Err(BacktestError::InvalidConfig("candle closes before it opens".into()));
        }
    }
    for pair in candles.windows(2) {
        if pair[1].open_time_ms <= pair[0].open_time_ms {
            return Err(BacktestError::InvalidConfig(
                "historical candles must be strictly ordered by open time".into(),
            ));
        }
    }
    Ok(())
}

fn validate_intent(intent: &crate::trading::StrategyOrderIntent) -> Result<(), BacktestError> {
    if !intent.quantity.is_finite() || intent.quantity <= 0.0 {
        return Err(BacktestError::Simulation("order quantity must be finite and positive".into()));
    }
    if let Some(price) = intent.price
        && (!price.is_finite() || price <= 0.0)
    {
        return Err(BacktestError::Simulation("order price must be finite and positive".into()));
    }
    Ok(())
}

fn exact(value: f64) -> Result<ExactDecimal, BacktestError> {
    let value = decimal_string(value).map_err(BacktestError::Simulation)?;
    ExactDecimal::new(value).map_err(BacktestError::Storage)
}

fn optional_positive_exact(value: f64) -> Result<Option<ExactDecimal>, BacktestError> {
    if value > 0.0 {
        Ok(Some(exact(value)?))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
