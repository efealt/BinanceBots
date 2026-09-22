use super::*;
use crate::{
    storage::{OrderSide, OrderType, RunStatus, StorageReader, TimeInForce},
    trading::{
        ExecutionAssumptions, GridAnchor, LimitFillPolicy, MarketCandle, SimulatedExecution,
        StaticGridConfig, StaticGridStrategy, StrategyDecision, StrategyOrderIntent,
        StrategyOutput, StrategyStartContext,
    },
};
use rusqlite::{Connection, params, types::ValueRef};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::{Path, PathBuf}, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

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
                 low_price, close_price, base_volume, quote_volume, trade_count,
                 taker_buy_base_volume, taker_buy_quote_volume)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 10.0, 1000.0, 10, 5.0, 500.0)",
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

struct StartGrid;

impl Strategy for StartGrid {
    fn id(&self) -> &str { "test-start-grid" }
    fn version(&self) -> &str { "1" }
    fn parameters(&self) -> serde_json::Value { json!({}) }

    fn on_start(
        &mut self,
        context: &StrategyStartContext<'_>,
    ) -> Result<StrategyOutput, String> {
        let previous = context
            .previous_candle
            .ok_or_else(|| "expected previous completed candle".to_string())?;
        if (previous.close - 101.0).abs() > 1e-12 {
            return Err(format!("unexpected previous close: {}", previous.close));
        }
        Ok(StrategyOutput {
            decisions: vec![StrategyDecision {
                decision_type: "initialize_grid".into(),
                payload: json!({"reference_close": previous.close}),
            }],
            order_intents: vec![StrategyOrderIntent {
                intent_key: Some("initial-grid-buy".into()),
                side: OrderSide::Buy,
                order_type: OrderType::Limit,
                time_in_force: Some(TimeInForce::Gtc),
                price: Some(105.0),
                quantity: 1.0,
                stop_price: None,
                reduce_only: false,
                metadata: json!({"source": "on_start"}),
            }],
        })
    }

    fn on_candle(&mut self, _context: &StrategyContext<'_>) -> Result<StrategyOutput, String> {
        Ok(StrategyOutput::default())
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
fn resting_start_order_can_fill_inside_first_active_candle() {
    let path = temp_database("start-grid");
    let (reader, dataset_id) = seed_dataset(&path);
    let engine = BacktestEngine::new(Arc::clone(&reader));
    let mut config = basic_config(dataset_id, "start-grid");
    config.start_time_ms = Some(60_000);
    config.end_time_ms = Some(119_999);

    let result = engine.run(&config, &mut StartGrid).expect("run start-grid backtest");
    assert_eq!(result.candles_processed, 1);

    let history = reader.trading_run_history(result.run_id).unwrap();
    assert_eq!(history.decisions.len(), 1);
    assert_eq!(history.decisions[0].event.event_time_ms, 60_000);
    assert_eq!(history.order_intents.len(), 1);
    assert_eq!(history.order_intents[0].event.event_time_ms, 60_000);
    assert_eq!(history.fills.len(), 1);
    assert_eq!(history.fills[0].event.event_time_ms, 119_999);
    assert_eq!(history.fills[0].price.as_str(), "105");

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


struct ReplenishAfterFill {
    emitted_replacement: bool,
}

impl Strategy for ReplenishAfterFill {
    fn id(&self) -> &str { "test-replenish-after-fill" }
    fn version(&self) -> &str { "1" }
    fn parameters(&self) -> serde_json::Value { json!({}) }

    fn on_start(
        &mut self,
        _context: &StrategyStartContext<'_>,
    ) -> Result<StrategyOutput, String> {
        Ok(StrategyOutput {
            decisions: vec![StrategyDecision {
                decision_type: "seed_buy".into(),
                payload: json!({}),
            }],
            order_intents: vec![StrategyOrderIntent {
                intent_key: Some("seed-buy".into()),
                side: OrderSide::Buy,
                order_type: OrderType::Limit,
                time_in_force: Some(TimeInForce::Gtc),
                price: Some(100.0),
                quantity: 1.0,
                stop_price: None,
                reduce_only: false,
                metadata: json!({"source": "on_start"}),
            }],
        })
    }

    fn on_candle(&mut self, context: &StrategyContext<'_>) -> Result<StrategyOutput, String> {
        if self.emitted_replacement || context.portfolio.position_quantity <= 0.0 {
            return Ok(StrategyOutput::default());
        }
        self.emitted_replacement = true;
        Ok(StrategyOutput {
            decisions: vec![StrategyDecision {
                decision_type: "replace_after_fill".into(),
                payload: json!({"position_quantity": context.portfolio.position_quantity}),
            }],
            order_intents: vec![StrategyOrderIntent {
                intent_key: Some("replacement-sell".into()),
                side: OrderSide::Sell,
                order_type: OrderType::Limit,
                time_in_force: Some(TimeInForce::Gtc),
                price: Some(111.0),
                quantity: 1.0,
                stop_price: None,
                reduce_only: false,
                metadata: json!({"source": "on_candle"}),
            }],
        })
    }
}

#[test]
fn static_grid_allows_multiple_preexisting_levels_to_fill_in_same_candle() {
    let path = temp_database("multi-grid");
    let (reader, dataset_id) = seed_dataset(&path);
    let engine = BacktestEngine::new(Arc::clone(&reader));
    let mut config = basic_config(dataset_id, "multi-grid");
    config.start_time_ms = Some(60_000);
    config.end_time_ms = Some(119_999);

    let mut strategy = StaticGridStrategy::new(StaticGridConfig {
        anchor: GridAnchor::PreviousClose,
        fixed_anchor_price: None,
        spacing_bps: 40.0,
        levels_per_side: 2,
        quantity_per_order: 1.0,
        time_in_force: TimeInForce::Gtc,
    }).unwrap();

    let result = engine.run(&config, &mut strategy).expect("run static grid");
    let history = reader.trading_run_history(result.run_id).unwrap();
    assert_eq!(history.order_intents.len(), 4);
    assert_eq!(history.fills.len(), 4);
    assert!(history.fills.iter().all(|fill| fill.event.event_time_ms == 119_999));
    assert_eq!(history.equity.len(), 1);

    cleanup(&path);
}

#[test]
fn replacement_created_after_candle_close_cannot_retroactively_fill_that_candle() {
    let path = temp_database("replacement-timing");
    let (reader, dataset_id) = seed_dataset(&path);
    let engine = BacktestEngine::new(Arc::clone(&reader));
    let mut config = basic_config(dataset_id, "replacement-timing");
    config.start_time_ms = Some(60_000);
    config.end_time_ms = Some(179_999);

    let mut strategy = ReplenishAfterFill { emitted_replacement: false };
    let result = engine.run(&config, &mut strategy).expect("run replacement timing");
    let history = reader.trading_run_history(result.run_id).unwrap();
    assert_eq!(history.fills.len(), 2);

    let buy = history.fills.iter().find(|fill| fill.price.as_str() == "100").unwrap();
    let replacement = history.fills.iter().find(|fill| fill.price.as_str() == "111").unwrap();
    assert_eq!(buy.event.event_time_ms, 119_999);
    assert_eq!(replacement.event.event_time_ms, 179_999);
    assert_ne!(replacement.event.event_time_ms, 119_999);

    cleanup(&path);
}

fn file_sha256(path: &Path) -> String {
    let bytes = fs::read(path).expect("read file for sha256");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn semantic_run_digest(path: &Path, run_id: i64) -> String {
    let connection = Connection::open(path).expect("open regression database for digest");
    let mut statement = connection.prepare(
        "SELECT
            CAST(e.run_sequence AS TEXT),
            e.event_kind,
            CAST(e.event_time_ms AS TEXT),
            rs.status, rs.note,
            d.decision_type, d.payload_json,
            oi.intent_key, oi.side, oi.order_type, oi.time_in_force,
            oi.price_decimal, oi.quantity_decimal, oi.stop_price_decimal,
            CAST(oi.reduce_only AS TEXT), oi.metadata_json,
            created_order.side, created_order.order_type, created_order.time_in_force,
            created_order.price_decimal, created_order.quantity_decimal,
            created_order.stop_price_decimal, created_order.metadata_json,
            os.status, os.filled_quantity_decimal, os.average_fill_price_decimal,
            os.reject_reason, os.metadata_json,
            CAST(state_order_event.run_sequence AS TEXT),
            f.price_decimal, f.quantity_decimal, f.fee_decimal, f.fee_asset,
            f.liquidity_role, f.metadata_json,
            CAST(fill_order_event.run_sequence AS TEXT),
            p.position_quantity_decimal, p.average_entry_price_decimal,
            p.mark_price_decimal, p.realized_pnl_decimal, p.unrealized_pnl_decimal,
            p.cash_balance_decimal, p.metadata_json,
            q.equity_decimal, q.cash_balance_decimal, q.realized_pnl_decimal,
            q.unrealized_pnl_decimal, q.fees_paid_decimal, q.metadata_json
         FROM trading_run_events e
         LEFT JOIN trading_run_status_events rs ON rs.event_id = e.event_id
         LEFT JOIN trading_decisions d ON d.event_id = e.event_id
         LEFT JOIN trading_order_intents oi ON oi.event_id = e.event_id
         LEFT JOIN trading_orders created_order ON created_order.created_event_id = e.event_id
         LEFT JOIN trading_order_state_events os ON os.event_id = e.event_id
         LEFT JOIN trading_orders state_order ON state_order.order_id = os.order_id
         LEFT JOIN trading_run_events state_order_event ON state_order_event.event_id = state_order.created_event_id
         LEFT JOIN trading_fills f ON f.event_id = e.event_id
         LEFT JOIN trading_orders fill_order ON fill_order.order_id = f.order_id
         LEFT JOIN trading_run_events fill_order_event ON fill_order_event.event_id = fill_order.created_event_id
         LEFT JOIN trading_position_snapshots p ON p.event_id = e.event_id
         LEFT JOIN trading_equity_snapshots q ON q.event_id = e.event_id
         WHERE e.run_id = ?1
           AND NOT (e.event_kind = 'run_status' AND rs.status = 'created')
         ORDER BY e.run_sequence"
    ).expect("prepare semantic digest query");

    let column_count = statement.column_count();
    let mut rows = statement.query(params![run_id]).expect("query semantic events");
    let mut hasher = Sha256::new();
    while let Some(row) = rows.next().expect("read semantic event") {
        for index in 0..column_count {
            match row.get_ref(index).expect("read semantic value") {
                ValueRef::Null => hasher.update(b"<NULL>"),
                ValueRef::Integer(value) => hasher.update(value.to_string().as_bytes()),
                ValueRef::Real(value) => hasher.update(value.to_string().as_bytes()),
                ValueRef::Text(value) => hasher.update(value),
                ValueRef::Blob(value) => hasher.update(value),
            }
            hasher.update([0x1f]);
        }
        hasher.update([0x1e]);
    }
    format!("{:x}", hasher.finalize())
}

fn regression_counts(path: &Path, run_id: i64) -> serde_json::Value {
    let connection = Connection::open(path).expect("open regression database for counts");
    let count = |table: &str| -> i64 {
        connection.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE run_id = ?1"),
            params![run_id],
            |row| row.get(0),
        ).expect("count regression rows")
    };
    json!({
        "decisions": count("trading_decisions"),
        "order_intents": count("trading_order_intents"),
        "orders": count("trading_orders"),
        "fills": count("trading_fills"),
        "positions": count("trading_position_snapshots"),
        "equity_snapshots": count("trading_equity_snapshots")
    })
}

#[test]
#[ignore = "full committed real-market Phase 3 regression; CI runs this explicitly"]
fn full_xagusdt_grid_regression() {
    const FIXTURE_SHA256: &str = "37afda792f5f06cee9e63eca010f6c5b850d2282fe5ecc5a686b213f832c4a7e";
    const FIRST_ACTIVE_OPEN_MS: i64 = 1_767_780_060_000;
    const LAST_CLOSE_MS: i64 = 1_789_948_799_999;
    const ACTIVE_CANDLES: usize = 369_479;

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test-data/xagusdt_1m_2026.sqlite3");
    let fixture_before = file_sha256(&fixture);
    assert_eq!(fixture_before, FIXTURE_SHA256);

    let path = std::env::var_os("XAGUSDT_REGRESSION_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let temp = temp_database("xagusdt-full-regression");
            fs::copy(&fixture, &temp).expect("copy frozen regression fixture");
            temp
        });
    assert_ne!(path, fixture, "regression must never mutate committed fixture in place");

    let reader = Arc::new(StorageReader::new(path.clone()));
    let engine = BacktestEngine::new(Arc::clone(&reader));
    let execution = ExecutionAssumptions {
        fee_bps: 4.0,
        spread_bps: 0.0,
        slippage_bps: 0.0,
        latency_ms: 0,
        limit_fill_policy: LimitFillPolicy::Touch,
        partial_fill_ratio: 1.0,
    };
    let run_config = json!({
        "purpose": "phase3_engineering_regression",
        "fixture_sha256": FIXTURE_SHA256,
        "parameters_are_test_fixture_not_trading_recommendation": true
    });

    let make_strategy = || StaticGridStrategy::new(StaticGridConfig {
        anchor: GridAnchor::PreviousClose,
        fixed_anchor_price: None,
        spacing_bps: 100.0,
        levels_per_side: 3,
        quantity_per_order: 1.0,
        time_in_force: TimeInForce::Gtc,
    }).unwrap();
    let make_config = |comparison_id: &str| BacktestRunConfig {
        dataset_id: 1,
        start_time_ms: Some(FIRST_ACTIVE_OPEN_MS),
        end_time_ms: Some(LAST_CLOSE_MS),
        comparison_id: Some(comparison_id.into()),
        initial_capital: ExactDecimal::new("100000").unwrap(),
        execution: execution.clone(),
        run_config: run_config.clone(),
    };

    let mut first_strategy = make_strategy();
    let first = engine.run(&make_config("phase3-xagusdt-regression-a"), &mut first_strategy)
        .expect("first full XAGUSDT regression run");
    let mut second_strategy = make_strategy();
    let second = engine.run(&make_config("phase3-xagusdt-regression-b"), &mut second_strategy)
        .expect("second full XAGUSDT regression run");

    assert_eq!(first.candles_processed, ACTIVE_CANDLES);
    assert_eq!(second.candles_processed, ACTIVE_CANDLES);

    let first_digest = semantic_run_digest(&path, first.run_id);
    let second_digest = semantic_run_digest(&path, second.run_id);
    assert_eq!(first_digest, second_digest, "semantic replay must be deterministic");

    let first_counts = regression_counts(&path, first.run_id);
    let second_counts = regression_counts(&path, second.run_id);
    assert_eq!(first_counts, second_counts);
    assert_eq!(first_counts["decisions"], 1);
    assert_eq!(first_counts["order_intents"], 6);
    assert_eq!(first_counts["orders"], 6);
    assert_eq!(first_counts["equity_snapshots"], ACTIVE_CANDLES as i64);

    let summary = json!({
        "fixture_sha256": FIXTURE_SHA256,
        "strategy_id": "static-grid-fixture",
        "strategy_version": "1",
        "strategy_parameters": first_strategy.parameters(),
        "execution_assumptions": execution,
        "active_candles": ACTIVE_CANDLES,
        "counts": first_counts,
        "final_portfolio": first.final_portfolio,
        "semantic_result_sha256": first_digest
    });
    println!("PHASE3_REGRESSION_SUMMARY={}", serde_json::to_string(&summary).unwrap());

    let baseline_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test-data/xagusdt_phase3_grid_baseline.json");
    if baseline_path.exists() {
        let expected: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&baseline_path).expect("read Phase 3 baseline")
        ).expect("parse Phase 3 baseline");
        assert_eq!(summary, expected, "Phase 3 real-data regression baseline changed");
    }

    assert_eq!(file_sha256(&fixture), fixture_before, "committed fixture changed during regression");

    if std::env::var_os("XAGUSDT_REGRESSION_DB").is_none() {
        cleanup(&path);
    }
}
