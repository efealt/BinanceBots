use super::*;
use crate::storage::StorageReader;
use rusqlite::{Connection, params};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_database(label: &str) -> std::path::PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "binance-grid-{label}-{}-{suffix}.sqlite3",
        std::process::id()
    ))
}

fn create_instrument(path: &std::path::Path) -> i64 {
    let connection = Connection::open(path).expect("open test database");
    connection.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    connection.execute(
        "INSERT INTO market_instruments
            (venue, market_type, symbol, created_at_ms)
         VALUES ('binance', 'spot', 'BTCUSDT', 1)",
        [],
    ).unwrap();
    connection.last_insert_rowid()
}

fn cleanup(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
}

fn spec(mode: RunMode, instrument_id: i64) -> TradingRunSpec {
    TradingRunSpec {
        comparison_id: Some("paper-replay-001".into()),
        mode,
        strategy_id: "test-grid".into(),
        strategy_version: "1".into(),
        strategy_params: json!({"step": "0.01"}),
        instrument_id,
        initial_capital: ExactDecimal::new("10000.00000000").unwrap(),
        run_config: json!({"currency": "USDT"}),
        data_source: json!({"source": "test"}),
        execution_assumptions: json!({"fees": "0.0004"}),
    }
}

#[test]
fn exact_decimal_is_canonical_without_precision_loss() {
    assert_eq!(
        ExactDecimal::new("000123.450000000000000001").unwrap().as_str(),
        "123.450000000000000001"
    );
    assert_eq!(ExactDecimal::new("-000.0100").unwrap().as_str(), "-0.01");
    assert_eq!(ExactDecimal::new("-0.000").unwrap().as_str(), "0");
    assert!(ExactDecimal::new("1e-8").is_err());
    assert!(ExactDecimal::new(" 1.0").is_err());
}

#[test]
fn persists_reconstructs_and_links_all_run_modes() {
    let path = temp_database("trading-runs");
    let reader = StorageReader::new(path.clone());
    reader.initialize().unwrap();
    let instrument_id = create_instrument(&path);

    let backtest = reader.create_trading_run(&spec(RunMode::Backtest, instrument_id)).unwrap();
    let paper = reader.create_trading_run(&spec(RunMode::Paper, instrument_id)).unwrap();
    let live = reader.create_trading_run(&spec(RunMode::Live, instrument_id)).unwrap();

    reader.set_trading_run_status(
        backtest.run_id,
        RunStatus::Running,
        EventTimes { event_time_ms: 1_000, exchange_time_ms: None, received_at_ms: Some(1_010) },
        None,
    ).unwrap();

    let decision = reader.record_decision(&DecisionInput {
        run_id: backtest.run_id,
        times: EventTimes { event_time_ms: 2_000, exchange_time_ms: Some(2_001), received_at_ms: Some(2_005) },
        decision_type: "grid_buy".into(),
        payload: json!({"level": 1}),
    }).unwrap();

    let intent = reader.record_order_intent(&OrderIntentInput {
        run_id: backtest.run_id,
        times: EventTimes::new(2_100),
        intent_key: Some("buy-1".into()),
        side: OrderSide::Buy,
        order_type: OrderType::Limit,
        time_in_force: Some(TimeInForce::Gtc),
        price: Some(ExactDecimal::new("60000.123456789012345678").unwrap()),
        quantity: ExactDecimal::new("0.00100000").unwrap(),
        stop_price: None,
        reduce_only: false,
        metadata: json!({}),
    }).unwrap();

    let order = reader.create_trading_order(&CreateOrderInput {
        run_id: backtest.run_id,
        times: EventTimes::new(2_200),
        intent_event_id: Some(intent.event.event_id),
        client_order_id: Some("paper-order-1".into()),
        exchange_order_id: None,
        side: OrderSide::Buy,
        order_type: OrderType::Limit,
        time_in_force: Some(TimeInForce::Gtc),
        price: Some(ExactDecimal::new("60000.123456789012345678").unwrap()),
        quantity: ExactDecimal::new("0.001").unwrap(),
        stop_price: None,
        metadata: json!({}),
    }).unwrap();

    reader.record_order_state(&OrderStateInput {
        order_id: order.order_id,
        times: EventTimes::new(2_250),
        status: OrderStatus::Accepted,
        filled_quantity: ExactDecimal::zero(),
        average_fill_price: None,
        reject_reason: None,
        metadata: json!({}),
    }).unwrap();

    reader.record_fill(&FillInput {
        order_id: order.order_id,
        times: EventTimes { event_time_ms: 2_300, exchange_time_ms: Some(2_301), received_at_ms: Some(2_309) },
        exchange_trade_id: None,
        price: ExactDecimal::new("60000.123456789012345678").unwrap(),
        quantity: ExactDecimal::new("0.001").unwrap(),
        fee: Some(ExactDecimal::new("0.024000049382715").unwrap()),
        fee_asset: Some("USDT".into()),
        liquidity_role: Some(LiquidityRole::Maker),
        metadata: json!({}),
    }).unwrap();

    reader.record_order_state(&OrderStateInput {
        order_id: order.order_id,
        times: EventTimes::new(2_310),
        status: OrderStatus::Filled,
        filled_quantity: ExactDecimal::new("0.001").unwrap(),
        average_fill_price: Some(ExactDecimal::new("60000.123456789012345678").unwrap()),
        reject_reason: None,
        metadata: json!({}),
    }).unwrap();

    reader.record_position_snapshot(&PositionSnapshotInput {
        run_id: backtest.run_id,
        times: EventTimes::new(2_320),
        position_quantity: ExactDecimal::new("0.001").unwrap(),
        average_entry_price: Some(ExactDecimal::new("60000.123456789012345678").unwrap()),
        mark_price: Some(ExactDecimal::new("60001.00").unwrap()),
        realized_pnl: Some(ExactDecimal::zero()),
        unrealized_pnl: Some(ExactDecimal::new("0.000876543210").unwrap()),
        cash_balance: Some(ExactDecimal::new("9939.975876493827285").unwrap()),
        metadata: json!({}),
    }).unwrap();

    reader.record_equity_snapshot(&EquitySnapshotInput {
        run_id: backtest.run_id,
        times: EventTimes::new(2_330),
        equity: ExactDecimal::new("10000.000876543210").unwrap(),
        cash_balance: Some(ExactDecimal::new("9939.975876493827285").unwrap()),
        realized_pnl: Some(ExactDecimal::zero()),
        unrealized_pnl: Some(ExactDecimal::new("0.000876543210").unwrap()),
        fees_paid: Some(ExactDecimal::new("0.024000049382715").unwrap()),
        metadata: json!({}),
    }).unwrap();

    reader.set_trading_run_status(
        backtest.run_id,
        RunStatus::Completed,
        EventTimes::new(3_000),
        Some("test complete"),
    ).unwrap();

    drop(reader);
    let reopened = StorageReader::new(path.clone());
    let history = reopened.trading_run_history(backtest.run_id).unwrap();
    assert_eq!(history.run.status, RunStatus::Completed);
    assert_eq!(history.decisions[0].event.event_time_ms, 2_000);
    assert_eq!(history.decisions[0].event.exchange_time_ms, Some(2_001));
    assert_eq!(history.decisions[0].event.received_at_ms, Some(2_005));
    assert_eq!(history.orders.len(), 1);
    assert_eq!(history.order_states.len(), 3);
    assert_eq!(history.fills.len(), 1);
    assert_eq!(history.fills[0].price.as_str(), "60000.123456789012345678");
    assert_eq!(history.positions.len(), 1);
    assert_eq!(history.equity.len(), 1);
    assert!(history.events.windows(2).all(|w| w[0].run_sequence < w[1].run_sequence));
    assert_eq!(decision.event.run_sequence, 3);

    let linked = reopened.trading_runs_by_comparison_id("paper-replay-001").unwrap();
    assert_eq!(linked.len(), 3);
    assert!(linked.iter().any(|run| run.run_id == paper.run_id));
    assert!(linked.iter().any(|run| run.run_id == live.run_id));

    let tail = reopened.trading_run_audit_page(backtest.run_id, None, 3).unwrap();
    assert_eq!(tail.events.len(), 3);
    assert_eq!(tail.total_events, history.events.len() as i64);
    assert!(tail.has_earlier);
    assert!(!tail.has_more);
    assert_eq!(tail.last_sequence, history.events.last().map(|event| event.run_sequence));

    let first_page = reopened.trading_run_audit_page(backtest.run_id, Some(0), 4).unwrap();
    assert_eq!(first_page.events.len(), 4);
    assert!(!first_page.has_earlier);
    assert!(first_page.has_more);
    let first_last = first_page.last_sequence.unwrap();
    let second_page = reopened.trading_run_audit_page(backtest.run_id, Some(first_last), 500).unwrap();
    assert_eq!(
        first_page.events.len() + second_page.events.len(),
        history.events.len()
    );
    assert!(!second_page.has_more);
    let fill_event = first_page
        .events
        .iter()
        .chain(second_page.events.iter())
        .find(|event| event.event.event_kind == RunEventKind::Fill)
        .expect("fill audit event");
    assert_eq!(fill_event.side, Some(OrderSide::Buy));
    assert_eq!(fill_event.order_type, Some(OrderType::Limit));
    assert_eq!(fill_event.price.as_ref().unwrap().as_str(), "60000.123456789012345678");
    assert_eq!(fill_event.quantity.as_ref().unwrap().as_str(), "0.001");

    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    connection.execute("DELETE FROM trading_runs WHERE run_id = ?1", params![backtest.run_id]).unwrap();
    let instrument_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM market_instruments WHERE instrument_id = ?1",
        params![instrument_id],
        |row| row.get(0),
    ).unwrap();
    let fill_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM trading_fills WHERE run_id = ?1",
        params![backtest.run_id],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(instrument_count, 1);
    assert_eq!(fill_count, 0);

    drop(connection);
    cleanup(&path);
}

#[test]
fn schema_rejects_invalid_mode_status_and_foreign_key() {
    let path = temp_database("trading-constraints");
    let reader = StorageReader::new(path.clone());
    reader.initialize().unwrap();
    let instrument_id = create_instrument(&path);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

    let insert_sql =
        "INSERT INTO trading_runs (
            mode, status, strategy_id, strategy_version, strategy_params_json,
            instrument_id, initial_capital_decimal, run_config_json,
            data_source_json, execution_assumptions_json, created_at_ms, updated_at_ms
         ) VALUES (?1, ?2, 'x', '1', '{}', ?3, '1', '{}', '{}', '{}', 1, 1)";

    assert!(connection.execute(insert_sql, params!["invalid", "created", instrument_id]).is_err());
    assert!(connection.execute(insert_sql, params!["paper", "invalid", instrument_id]).is_err());
    assert!(connection.execute(insert_sql, params!["paper", "created", 999999_i64]).is_err());

    drop(connection);
    cleanup(&path);
}
