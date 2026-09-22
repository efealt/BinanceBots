use crate::{
    storage::{
        CreateOrderInput, DecisionInput, EquitySnapshotInput, EventTimes, ExactDecimal, FillInput,
        HistoricalDatasetInfo, LiquidityRole, OrderIntentInput, OrderStateInput, OrderStatus,
        PositionSnapshotInput, RunMode, RunStatus, StorageError, StorageReader, TradingRunSpec,
    },
    trading::{
        decimal_string, ExecutionAssumptions, MarketCandle, PortfolioState, PortfolioView,
        SimulatedExecution, Strategy, StrategyContext,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct BacktestRunConfig {
    pub dataset_id: i64,
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
    pub candles_processed: usize,
    pub final_portfolio: PortfolioView,
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
        validate_config(config)?;

        let dataset = self.storage.historical_dataset_info(config.dataset_id)?;
        let candles = self.storage.ohlcv_series_between(
            config.dataset_id,
            config.start_time_ms,
            config.end_time_ms,
        )?;
        if candles.is_empty() {
            return Err(BacktestError::InvalidConfig(
                "selected dataset/range contains no completed candles".into(),
            ));
        }
        validate_candle_order(&candles)?;

        let first = &candles[0];
        let last = candles.last().expect("non-empty candles");
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
                "interval": dataset.interval,
                "source": dataset.source,
                "requested_start_time_ms": config.start_time_ms,
                "requested_end_time_ms": config.end_time_ms,
                "first_open_time_ms": first.open_time_ms,
                "last_close_time_ms": last.close_time_ms
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
            &dataset,
            &candles,
            strategy,
            &mut portfolio,
            &mut execution,
            &mut failure_time,
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
                Ok(BacktestRunResult {
                    run_id: run.run_id,
                    dataset_id: config.dataset_id,
                    candles_processed: candles.len(),
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

    fn execute<S: Strategy>(
        &self,
        run_id: i64,
        _dataset: &HistoricalDatasetInfo,
        candles: &[crate::storage::OhlcvCandle],
        strategy: &mut S,
        portfolio: &mut PortfolioState,
        execution: &mut SimulatedExecution,
        failure_time: &mut i64,
    ) -> Result<(), BacktestError> {
        for candle in candles {
            *failure_time = candle.close_time_ms;
            let market_candle = MarketCandle {
                open_time_ms: candle.open_time_ms,
                close_time_ms: candle.close_time_ms,
                open: candle.open_price,
                high: candle.high_price,
                low: candle.low_price,
                close: candle.close_price,
                volume: candle.base_volume,
            };

            let fills = execution
                .process_candle(&market_candle)
                .map_err(BacktestError::Simulation)?;
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
            let output = strategy
                .on_candle(&context)
                .map_err(BacktestError::Strategy)?;

            for decision in output.decisions {
                self.storage.record_decision(&DecisionInput {
                    run_id,
                    times: EventTimes::new(market_candle.close_time_ms),
                    decision_type: decision.decision_type,
                    payload: decision.payload,
                })?;
            }

            for intent in output.order_intents {
                validate_intent(&intent)?;
                let persisted_intent = self.storage.record_order_intent(&OrderIntentInput {
                    run_id,
                    times: EventTimes::new(market_candle.close_time_ms),
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
                    times: EventTimes::new(market_candle.close_time_ms),
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
                    times: EventTimes::new(market_candle.close_time_ms),
                    status: OrderStatus::Accepted,
                    filled_quantity: ExactDecimal::zero(),
                    average_fill_price: None,
                    reject_reason: None,
                    metadata: json!({"simulated": true}),
                })?;
                execution
                    .submit(order.order_id, market_candle.close_time_ms, intent)
                    .map_err(BacktestError::Simulation)?;
            }

            let view = portfolio.view();
            self.storage.record_equity_snapshot(&EquitySnapshotInput {
                run_id,
                times: EventTimes::new(market_candle.close_time_ms),
                equity: exact(view.equity)?,
                cash_balance: Some(exact(view.cash)?),
                realized_pnl: Some(exact(view.realized_pnl)?),
                unrealized_pnl: Some(exact(view.unrealized_pnl)?),
                fees_paid: Some(exact(view.fees_paid)?),
                metadata: json!({"mark_price": decimal_string(market_candle.close).map_err(BacktestError::Simulation)?}),
            })?;
        }
        Ok(())
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
