use super::*;
use crate::{
    storage::{OrderSide, OrderType, RunStatus, StorageReader, TimeInForce},
    trading::{
        ExecutionAssumptions, LimitFillPolicy, MarketCandle, SimulatedExecution, StrategyDecision,
        StrategyOrderIntent, StrategyOutput,
    },
};
use rusqlite::{Connection, params};
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

fn temp_database(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "binance-grid-backtest-{label}-{}-{suffix}.sqlite3",
        std::process::id()
    ))
}

fn seed_dataset(path: &std::path::Path) -> (Arc<StorageReader>, i64) {
    let reader = Arc::new(StorageReader::new(path.to_path_buf()));
    reader.initialize().expect("initialize test database");
    let connection = Connection::open(path).expect("open test database");
    connection.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    connection.execute(
        "INSERT INTO market_instruments
            (venue, market_type, symbol, created_at_ms)
         VALUES ('binance', 'spot', 'BTCUSDT', 1)",
        [],
    ).unwrap();
    let instrument_id = connection.last_insert_rowid();
    connection.execute(
        "INSERT INTO historical_datasets
            (instrument_id, dataset_kind, interval, source, start_time_ms,
             end_time_ms, downloaded_at_ms, status)
         VALUES (?1, 'traded_kline', '1m', 'test', 0, 179999, 1, 'complete')",
        params![instrument_id],
    ).unwrap();
    let dataset_id = connection.last_insert_rowid();
    let candles = [
        (0_i64, 59_999_i64, 100.0, 102.0, 99.0, 101.0),
        (60_000, 119_999, 110.0, 112.0, 100.0, 111.0),
        (120_000, 179_999, 112.0, 115.0, 98.0, 114.0),
    ];
    for (open_time, close_time, open, high, low, close) in candles {
        connection.execute(
            "INSERT INTO historical_ohlcv
                (dataset_id, open_time_ms, close_time_ms, open_price, high_price,
                 low_price, close_price, base_volume)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 10.0)",
            params![dataset_id, open_time, close_time, open, high, low, close],
        ).unwrap();
    }
    (reader, dataset_id)
}

fn cleanup(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
}

struct OneShotMarket {
    emitted: bool,
}

impl Strategy for OneShotMarket {
    fn id(&self) -> &str { "test-one-shot-market" }
    fn version(&self) -> &str { "1" }
    fn parameters(&self) -> serde_json::Value { json!({}) }

    fn on_candle(&mut self, context: &StrategyContext<'_>) -> Result<StrategyOutput, String> {
        if self.emitted {
            return Ok(StrategyOutput::default());
        }
        self.emitted = true;
        Ok(StrategyOutput {
            decisions: vec![StrategyDecision {
                decision_type: "enter".into(),
                payload: json!({"signal_time_ms": context.now_ms}),
            }],
            order_intents: vec![StrategyOrderIntent {
                intent_key: Some("entry".into()),
                side: OrderSide::Buy,
                order_type: OrderType::Market,
                time_in_force: None,
                price: None,
                quantity: 1.0,
                stop_price: None,
                reduce_only: false,
                metadata: json!({}),
            }],
        })
    }
}

struct FailingStrategy;

impl Strategy for FailingStrategy {
    fn id(&self) -> &str { "test-failing" }
    fn version(&self) -> &str { "1" }
    fn parameters(&self) -> serde_json::Value { json!({}) }
    fn on_candle(&mut self, _context: &StrategyContext<'_>) -> Result<StrategyOutput, String> {
        Err("intentional failure".into())
    }
}

fn basic_config(dataset_id: i64, comparison_id: &str) -> BacktestRunConfig {
    BacktestRunConfig {
        dataset_id,
        start_time_ms: None,
        end_time_ms: None,
        comparison_id: Some(comparison_id.into()),
        initial_capital: ExactDecimal::new("1000").unwrap(),
        execution: ExecutionAssumptions::default(),
        run_config: json!({"test": true}),
    }
}

#[test]
fn market_order_fills_only_on_later_candle_and_applies_costs() {
    let path = temp_database("no-lookahead");
    let (reader, dataset_id) = seed_dataset(&path);
    let engine = BacktestEngine::new(Arc::clone(&reader));
    let mut strategy = OneShotMarket { emitted: false };
    let mut config = basic_config(dataset_id, "no-lookahead");
    config.end_time_ms = Some(119_999);
    config.execution = ExecutionAssumptions {
        fee_bps: 10.0,
        spread_bps: 10.0,
        slippage_bps: 20.0,
        latency_ms: 0,
        limit_fill_policy: LimitFillPolicy::Touch,
        partial_fill_ratio: 1.0,
    };

    let result = engine.run(&config, &mut strategy).expect("run backtest");
    assert_eq!(result.candles_processed, 2);
    let history = reader.trading_run_history(result.run_id).unwrap();
    assert_eq!(history.decisions.len(), 1);
    assert_eq!(history.decisions[0].event.event_time_ms, 59_999);
    assert_eq!(history.fills.len(), 1);
    assert_eq!(history.fills[0].event.event_time_ms, 60_000);
    assert_eq!(history.fills[0].price.as_str(), "110.275");
    assert_eq!(history.fills[0].fee.as_ref().unwrap().as_str(), "0.110275");
    assert_eq!(history.run.status, RunStatus::Completed);
    assert!((result.final_portfolio.equity - 1000.614725).abs() < 1e-9);
    assert_eq!(history.equity.last().unwrap().equity.as_str(), "1000.614725");

    cleanup(&path);
}

#[test]
fn repeated_replay_is_deterministic_for_simulated_outputs() {
    let path = temp_database("determinism");
    let (reader, dataset_id) = seed_dataset(&path);
    let engine = BacktestEngine::new(Arc::clone(&reader));
    let config = basic_config(dataset_id, "determinism");

    let first = engine.run(&config, &mut OneShotMarket { emitted: false }).unwrap();
    let second = engine.run(&config, &mut OneShotMarket { emitted: false }).unwrap();
    let a = reader.trading_run_history(first.run_id).unwrap();
    let b = reader.trading_run_history(second.run_id).unwrap();

    let fills_a: Vec<_> = a.fills.iter().map(|fill| (
        fill.event.event_time_ms,
        fill.price.as_str().to_string(),
        fill.quantity.as_str().to_string(),
        fill.fee.as_ref().map(|value| value.as_str().to_string()),
    )).collect();
    let fills_b: Vec<_> = b.fills.iter().map(|fill| (
        fill.event.event_time_ms,
        fill.price.as_str().to_string(),
        fill.quantity.as_str().to_string(),
        fill.fee.as_ref().map(|value| value.as_str().to_string()),
    )).collect();
    assert_eq!(fills_a, fills_b);

    let equity_a: Vec<_> = a.equity.iter().map(|snapshot| (
        snapshot.event.event_time_ms,
        snapshot.equity.as_str().to_string(),
        snapshot.cash_balance.as_ref().map(|value| value.as_str().to_string()),
    )).collect();
    let equity_b: Vec<_> = b.equity.iter().map(|snapshot| (
        snapshot.event.event_time_ms,
        snapshot.equity.as_str().to_string(),
        snapshot.cash_balance.as_ref().map(|value| value.as_str().to_string()),
    )).collect();
    assert_eq!(equity_a, equity_b);

    cleanup(&path);
}

#[test]
fn simulator_handles_limit_policy_latency_and_partial_fills() {
    let intent = StrategyOrderIntent {
        intent_key: None,
        side: OrderSide::Buy,
        order_type: OrderType::Limit,
        time_in_force: Some(TimeInForce::Gtc),
        price: Some(100.0),
        quantity: 2.0,
        stop_price: None,
        reduce_only: false,
        metadata: json!({}),
    };
    let candle_touch = MarketCandle {
        open_time_ms: 60_000,
        close_time_ms: 119_999,
        open: 101.0,
        high: 103.0,
        low: 100.0,
        close: 102.0,
        volume: 10.0,
    };

    let mut touch = SimulatedExecution::new(ExecutionAssumptions {
        fee_bps: 0.0,
        spread_bps: 0.0,
        slippage_bps: 0.0,
        latency_ms: 0,
        limit_fill_policy: LimitFillPolicy::Touch,
        partial_fill_ratio: 0.5,
    }).unwrap();
    touch.submit(1, 59_999, intent.clone()).unwrap();
    let first = touch.process_candle(&candle_touch).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].status, OrderStatus::PartiallyFilled);
    assert_eq!(first[0].quantity, 1.0);
    assert_eq!(first[0].cumulative_filled_quantity, 1.0);

    let candle_second = MarketCandle { open_time_ms: 120_000, close_time_ms: 179_999, ..candle_touch.clone() };
    let second = touch.process_candle(&candle_second).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].status, OrderStatus::Filled);
    assert_eq!(second[0].cumulative_filled_quantity, 2.0);

    let mut trade_through = SimulatedExecution::new(ExecutionAssumptions {
        limit_fill_policy: LimitFillPolicy::TradeThrough,
        ..ExecutionAssumptions::default()
    }).unwrap();
    trade_through.submit(2, 59_999, intent.clone()).unwrap();
    assert!(trade_through.process_candle(&candle_touch).unwrap().is_empty());

    let mut latency = SimulatedExecution::new(ExecutionAssumptions {
        latency_ms: 60_002,
        ..ExecutionAssumptions::default()
    }).unwrap();
    latency.submit(3, 59_999, intent).unwrap();
    assert!(latency.process_candle(&candle_second).unwrap().is_empty());
    let later = MarketCandle { open_time_ms: 180_000, close_time_ms: 239_999, low: 99.0, ..candle_touch };
    assert_eq!(latency.process_candle(&later).unwrap().len(), 1);
}

#[test]
fn failed_strategy_marks_run_failed_without_deleting_history() {
    let path = temp_database("failure");
    let (reader, dataset_id) = seed_dataset(&path);
    let engine = BacktestEngine::new(Arc::clone(&reader));
    let config = basic_config(dataset_id, "failure-run");
    let error = engine.run(&config, &mut FailingStrategy).unwrap_err();
    assert!(matches!(error, BacktestError::Strategy(_)));

    let linked = reader.trading_runs_by_comparison_id("failure-run").unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].status, RunStatus::Failed);
    let history = reader.trading_run_history(linked[0].run_id).unwrap();
    assert!(history.status_events.iter().any(|event| event.status == RunStatus::Running));
    assert!(history.status_events.iter().any(|event| event.status == RunStatus::Failed));

    cleanup(&path);
}
