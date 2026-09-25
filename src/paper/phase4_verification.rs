use super::*;
use crate::{
    backtest::{BacktestEngine, BacktestRunConfig},
    storage::{RunEventKind, TimeInForce},
    trading::{GridAnchor, LimitFillPolicy, StrategyDecision, StrategyOrderIntent},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{RwLock, broadcast, watch};

fn verification_database(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "binance-bots-phase4-verification-{label}-{}-{suffix}.sqlite3",
        std::process::id()
    ))
}

fn cleanup_database(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
}

fn snapshot_candle(open_time: i64, open: f64, high: f64, low: f64, close: f64) -> Candle {
    Candle {
        open_time,
        close_time: open_time + BASE_INTERVAL_MS - 1,
        open,
        high,
        low,
        close,
        volume: 1.0,
        is_closed: true,
    }
}

fn parity_candles() -> Vec<MarketCandle> {
    vec![
        MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.0,
            volume: 10.0,
        },
        MarketCandle {
            open_time_ms: 60_000,
            close_time_ms: 119_999,
            open: 100.0,
            high: 102.0,
            low: 98.0,
            close: 100.5,
            volume: 12.0,
        },
        MarketCandle {
            open_time_ms: 120_000,
            close_time_ms: 179_999,
            open: 100.5,
            high: 101.5,
            low: 99.5,
            close: 101.0,
            volume: 8.0,
        },
    ]
}

fn parity_grid() -> StaticGridConfig {
    StaticGridConfig {
        anchor: GridAnchor::PreviousClose,
        fixed_anchor_price: None,
        spacing_bps: 100.0,
        levels_per_side: 1,
        quantity_per_order: 1.0,
        time_in_force: TimeInForce::Gtc,
    }
}

fn parity_execution() -> ExecutionAssumptions {
    ExecutionAssumptions {
        fee_bps: 4.0,
        spread_bps: 0.0,
        slippage_bps: 0.0,
        latency_ms: 0,
        limit_fill_policy: LimitFillPolicy::Touch,
        partial_fill_ratio: 1.0,
    }
}

fn seed_backtest_dataset(
    path: &Path,
    candles: &[MarketCandle],
) -> (Arc<StorageReader>, i64) {
    let storage = Arc::new(StorageReader::new(path.to_path_buf()));
    storage.initialize().expect("initialize parity backtest database");
    let connection = Connection::open(path).expect("open parity backtest database");
    connection.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    connection
        .execute(
            "INSERT INTO market_instruments
                (venue, market_type, symbol, created_at_ms)
             VALUES ('binance', 'spot', 'BTCUSDT', 1)",
            [],
        )
        .unwrap();
    let instrument_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO historical_datasets
                (instrument_id, dataset_kind, interval, source, start_time_ms,
                 end_time_ms, downloaded_at_ms, status)
             VALUES (?1, 'traded_kline', '1m', 'phase4_verification', ?2, ?3, 1, 'complete')",
            params![
                instrument_id,
                candles.first().unwrap().open_time_ms,
                candles.last().unwrap().close_time_ms
            ],
        )
        .unwrap();
    let dataset_id = connection.last_insert_rowid();

    for candle in candles {
        connection
            .execute(
                "INSERT INTO historical_ohlcv
                    (dataset_id, open_time_ms, close_time_ms, open_price, high_price,
                     low_price, close_price, base_volume, quote_volume, trade_count,
                     taker_buy_base_volume, taker_buy_quote_volume)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 10, ?10, ?11)",
                params![
                    dataset_id,
                    candle.open_time_ms,
                    candle.close_time_ms,
                    candle.open,
                    candle.high,
                    candle.low,
                    candle.close,
                    candle.volume,
                    candle.close * candle.volume,
                    candle.volume * 0.5,
                    candle.close * candle.volume * 0.5
                ],
            )
            .unwrap();
    }

    (storage, dataset_id)
}

fn new_paper_core(
    path: &Path,
    strategy: Box<dyn Strategy + Send>,
    execution: ExecutionAssumptions,
) -> (Arc<StorageReader>, PaperRunCore) {
    let storage = Arc::new(StorageReader::new(path.to_path_buf()));
    storage.initialize().expect("initialize parity Paper database");
    let instrument_id = storage
        .ensure_market_instrument("BTCUSDT", "spot")
        .expect("ensure parity Paper instrument");
    let bot = storage
        .create_trading_bot(&crate::storage::TradingBotSpec {
            bot_name: "Phase 4 parity bot".into(),
            config: json!({"verification": "phase_4_8a"}),
        })
        .expect("create parity bot");
    let run = storage
        .create_trading_run(&TradingRunSpec {
            bot_id: Some(bot.bot_id),
            comparison_id: Some("phase4-parity".into()),
            mode: RunMode::Paper,
            strategy_id: strategy.id().to_string(),
            strategy_version: strategy.version().to_string(),
            strategy_params: strategy.parameters(),
            instrument_id,
            initial_capital: ExactDecimal::new("1000").unwrap(),
            run_config: json!({"verification": "phase_4_8a"}),
            data_source: json!({
                "kind": "synthetic_completed_candles",
                "symbol": "BTCUSDT",
                "market_type": "spot",
                "replay_interval": "1m",
                "start_reference_boundary_ms": 60_000
            }),
            execution_assumptions: serde_json::to_value(&execution).unwrap(),
        })
        .expect("create parity Paper run");

    let core = PaperRunCore {
        run_id: run.run_id,
        storage: Arc::clone(&storage),
        strategy,
        portfolio: PortfolioState::new(1000.0).unwrap(),
        execution: LivePaperExecution::new(execution).unwrap(),
        status: RunStatus::Created,
    };
    (storage, core)
}

fn decimal(value: &Option<ExactDecimal>) -> Option<String> {
    value.as_ref().map(|item| item.as_str().to_string())
}

#[test]
fn phase4_backtest_and_paper_are_semantically_equal_with_equivalent_live_trade_touches() {
    let candles = parity_candles();
    let backtest_path = verification_database("parity-backtest");
    let paper_path = verification_database("parity-paper");

    let (backtest_storage, dataset_id) = seed_backtest_dataset(&backtest_path, &candles);
    let engine = BacktestEngine::new(Arc::clone(&backtest_storage));
    let mut backtest_strategy = StaticGridStrategy::new(parity_grid()).unwrap();
    let backtest_result = engine
        .run(
            &BacktestRunConfig {
                dataset_id,
                replay_interval: TradingInterval::OneMinute,
                start_time_ms: Some(60_000),
                end_time_ms: Some(179_999),
                comparison_id: Some("phase4-parity".into()),
                initial_capital: ExactDecimal::new("1000").unwrap(),
                execution: parity_execution(),
                run_config: json!({"verification": "phase_4_8a"}),
            },
            &mut backtest_strategy,
        )
        .expect("run parity Backtest");
    let backtest_history = backtest_storage
        .trading_run_history(backtest_result.run_id)
        .unwrap();

    let paper_strategy = Box::new(StaticGridStrategy::new(parity_grid()).unwrap());
    let (paper_storage, mut paper_core) =
        new_paper_core(&paper_path, paper_strategy, parity_execution());
    paper_core.start(60_000, &candles[0]).expect("start parity Paper");
    let mut trade_id = 1_u64;
    for candle in &candles[1..] {
        for price in [candle.low, candle.high] {
            paper_core
                .process_trade(
                    &LiveTradeEvent {
                        trade_id,
                        event_time_ms: candle.close_time_ms,
                        price,
                        quantity: 10.0,
                    },
                    candle.close_time_ms,
                    trade_id,
                )
                .expect("process parity Paper trade");
            trade_id += 1;
        }
        paper_core
            .process_strategy_candle(candle, candle.close_time_ms, trade_id)
            .expect("process parity Paper strategy candle");
    }
    let paper_history = paper_storage.trading_run_history(paper_core.run_id).unwrap();

    let decisions = |history: &crate::storage::TradingRunHistory| {
        history
            .decisions
            .iter()
            .map(|record| {
                (
                    record.event.event_time_ms,
                    record.decision_type.clone(),
                    record.payload.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(decisions(&backtest_history), decisions(&paper_history));

    let intents = |history: &crate::storage::TradingRunHistory| {
        history
            .order_intents
            .iter()
            .map(|record| {
                (
                    record.event.event_time_ms,
                    record.intent_key.clone(),
                    record.side.as_str().to_string(),
                    record.order_type.as_str().to_string(),
                    record.time_in_force.map(|value| value.as_str().to_string()),
                    decimal(&record.price),
                    record.quantity.as_str().to_string(),
                    decimal(&record.stop_price),
                    record.reduce_only,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(intents(&backtest_history), intents(&paper_history));

    let orders = |history: &crate::storage::TradingRunHistory| {
        history
            .orders
            .iter()
            .map(|record| {
                (
                    record.order_id,
                    record.created_event.event_time_ms,
                    record.side.as_str().to_string(),
                    record.order_type.as_str().to_string(),
                    record.time_in_force.map(|value| value.as_str().to_string()),
                    decimal(&record.price),
                    record.quantity.as_str().to_string(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(orders(&backtest_history), orders(&paper_history));

    let states = |history: &crate::storage::TradingRunHistory| {
        history
            .order_states
            .iter()
            .map(|record| {
                (
                    record.event.event_time_ms,
                    record.order_id,
                    record.status.as_str().to_string(),
                    record.filled_quantity.as_str().to_string(),
                    decimal(&record.average_fill_price),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(states(&backtest_history), states(&paper_history));

    let fills = |history: &crate::storage::TradingRunHistory| {
        history
            .fills
            .iter()
            .map(|record| {
                (
                    record.event.event_time_ms,
                    record.order_id,
                    record.price.as_str().to_string(),
                    record.quantity.as_str().to_string(),
                    decimal(&record.fee),
                    record
                        .liquidity_role
                        .map(|value| value.as_str().to_string()),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(fills(&backtest_history), fills(&paper_history));
    assert_eq!(paper_history.fills.len(), 2);

    let positions = |history: &crate::storage::TradingRunHistory| {
        history
            .positions
            .iter()
            .map(|record| {
                (
                    record.event.event_time_ms,
                    record.position_quantity.as_str().to_string(),
                    decimal(&record.average_entry_price),
                    decimal(&record.mark_price),
                    decimal(&record.realized_pnl),
                    decimal(&record.unrealized_pnl),
                    decimal(&record.cash_balance),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(&backtest_history), positions(&paper_history));

    let equity_values = |records: &[crate::storage::EquitySnapshotRecord]| {
        records
            .iter()
            .map(|record| {
                (
                    record.event.event_time_ms,
                    record.equity.as_str().to_string(),
                    decimal(&record.cash_balance),
                    decimal(&record.realized_pnl),
                    decimal(&record.unrealized_pnl),
                    decimal(&record.fees_paid),
                )
            })
            .collect::<Vec<_>>()
    };
    let paper_candle_equity = paper_history
        .equity
        .iter()
        .filter(|record| {
            record.metadata.get("source").and_then(Value::as_str)
                == Some("binance_closed_candle")
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        equity_values(&backtest_history.equity),
        equity_values(&paper_candle_equity)
    );
    assert_eq!(
        paper_history.equity.len(),
        backtest_history.equity.len() + paper_history.fills.len()
    );

    let common_event_order = |history: &crate::storage::TradingRunHistory| {
        history
            .events
            .iter()
            .filter(|event| {
                event.event_kind != RunEventKind::RunStatus
                    && event.event_kind != RunEventKind::Equity
            })
            .map(|event| (event.event_kind.as_str().to_string(), event.event_time_ms))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        common_event_order(&backtest_history),
        common_event_order(&paper_history)
    );
    assert!(
        backtest_history
            .events
            .windows(2)
            .all(|pair| pair[0].run_sequence < pair[1].run_sequence)
    );
    assert!(
        paper_history
            .events
            .windows(2)
            .all(|pair| pair[0].run_sequence < pair[1].run_sequence)
    );

    let backtest_final = backtest_result.final_portfolio;
    let paper_final = paper_core.portfolio.view();
    assert_eq!(
        decimal_string(backtest_final.cash).unwrap(),
        decimal_string(paper_final.cash).unwrap()
    );
    assert_eq!(
        decimal_string(backtest_final.position_quantity).unwrap(),
        decimal_string(paper_final.position_quantity).unwrap()
    );
    assert_eq!(
        decimal_string(backtest_final.average_entry_price).unwrap(),
        decimal_string(paper_final.average_entry_price).unwrap()
    );
    assert_eq!(
        decimal_string(backtest_final.realized_pnl).unwrap(),
        decimal_string(paper_final.realized_pnl).unwrap()
    );
    assert_eq!(
        decimal_string(backtest_final.unrealized_pnl).unwrap(),
        decimal_string(paper_final.unrealized_pnl).unwrap()
    );
    assert_eq!(
        decimal_string(backtest_final.fees_paid).unwrap(),
        decimal_string(paper_final.fees_paid).unwrap()
    );
    assert_eq!(
        decimal_string(backtest_final.equity).unwrap(),
        decimal_string(paper_final.equity).unwrap()
    );

    drop(paper_core);
    drop(paper_storage);
    drop(backtest_storage);
    cleanup_database(&paper_path);
    cleanup_database(&backtest_path);
}

#[test]
fn phase4_mid_interval_start_uses_latest_completed_candle_without_pre_start_replay() {
    let mid_hour_ms = 5_821_234;
    let reference_boundary = TradingInterval::OneHour.bucket_open_ms(mid_hour_ms);
    assert_eq!(reference_boundary, 3_600_000);

    let mut previous = snapshot_candle(0, 99.0, 101.0, 98.0, 100.0);
    previous.close_time = 3_599_999;
    let mut in_progress = snapshot_candle(3_600_000, 100.0, 104.0, 99.0, 103.0);
    in_progress.close_time = 7_199_999;
    in_progress.is_closed = false;

    let bootstrap = previous_completed_replay_candle(
        &[previous.clone(), in_progress],
        reference_boundary,
        TradingInterval::OneHour,
    )
    .expect("latest completed replay candle at Start");

    assert_eq!(bootstrap.open_time_ms, previous.open_time);
    assert_eq!(bootstrap.close_time_ms, previous.close_time);
    assert_eq!(bootstrap.close, previous.close);
    assert!(bootstrap.close_time_ms < reference_boundary);

    let first_execution_minute = first_full_base_open_at_or_after(mid_hour_ms);
    assert_eq!(first_execution_minute, 5_880_000);
    assert!(first_execution_minute > mid_hour_ms);
}

#[test]
fn phase4_realtime_candle_integrity_rejects_duplicate_out_of_order_and_gap_events() {
    let mut duplicate = ReplayAggregator::new(TradingInterval::OneMinute, 60_000);
    duplicate
        .push(&snapshot_candle(60_000, 1.0, 1.0, 1.0, 1.0))
        .expect("first completed candle");
    let duplicate_error = duplicate
        .push(&snapshot_candle(60_000, 1.0, 1.0, 1.0, 1.0))
        .err()
        .expect("duplicate event must fail");
    assert!(duplicate_error.contains("expected 120000"));
    assert!(duplicate_error.contains("received 60000"));

    let mut out_of_order = ReplayAggregator::new(TradingInterval::OneMinute, 120_000);
    let out_of_order_error = out_of_order
        .push(&snapshot_candle(60_000, 1.0, 1.0, 1.0, 1.0))
        .err()
        .expect("out-of-order event must fail");
    assert!(out_of_order_error.contains("expected 120000"));
    assert!(out_of_order_error.contains("received 60000"));

    let stale = snapshot_candle(60_000, 1.0, 1.0, 1.0, 1.0);
    let expected = ReplayAggregator::new(TradingInterval::OneMinute, 120_000);
    assert!(stale.open_time < expected.expected_base_open_ms());

    let mut gap = ReplayAggregator::new(TradingInterval::OneMinute, 60_000);
    let gap_error = gap
        .push(&snapshot_candle(120_000, 1.0, 1.0, 1.0, 1.0))
        .err()
        .expect("gap event must fail");
    assert!(gap_error.contains("expected 60000"));
    assert!(gap_error.contains("received 120000"));
}

struct VerificationNoOp;

impl Strategy for VerificationNoOp {
    fn id(&self) -> &str {
        "phase4-verification-noop"
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
            decisions: vec![StrategyDecision {
                decision_type: "verification_start".into(),
                payload: json!({}),
            }],
            order_intents: Vec::<StrategyOrderIntent>::new(),
        })
    }

    fn on_candle(
        &mut self,
        _context: &StrategyContext<'_>,
    ) -> Result<StrategyOutput, String> {
        Ok(StrategyOutput::default())
    }
}

#[tokio::test]
async fn phase4_stop_is_idempotent_and_reconnect_is_snapshot_first_revision_safe() {
    let path = verification_database("stop-reconnect");
    let (storage, mut core) = new_paper_core(
        &path,
        Box::new(VerificationNoOp),
        ExecutionAssumptions::default(),
    );
    let previous = parity_candles()[0].clone();
    core.start(60_000, &previous).unwrap();

    let manager = PaperManager {
        storage: Arc::clone(&storage),
        market: Arc::new(MarketService::new()),
        runtimes: Mutex::new(HashMap::new()),
    };
    let mut snapshot = manager.persisted_snapshot(core.run_id).unwrap();
    snapshot.runtime_active = true;
    snapshot.runtime_status = "running".into();
    snapshot.canonical_status = "running".into();
    snapshot.stream_revision = 7;
    snapshot.started_at_ms = Some(60_000);

    let (stop_tx, stop_rx) = watch::channel(false);
    let (updates_tx, _) = broadcast::channel(8);
    let handle = Arc::new(PaperRuntimeHandle {
        snapshot: Arc::new(RwLock::new(snapshot.clone())),
        stop_tx,
        updates_tx,
    });
    manager
        .runtimes
        .lock()
        .await
        .insert(core.run_id, Arc::clone(&handle));

    let (first_bootstrap, first_receiver) = manager.stream_bootstrap(core.run_id).await.unwrap();
    assert_eq!(first_bootstrap.stream_revision, 7);
    drop(first_receiver.expect("initial live receiver"));
    assert!(manager.runtimes.lock().await.contains_key(&core.run_id));
    assert!(!*stop_rx.borrow());

    manager
        .update_snapshot(
            &handle,
            core.portfolio.view(),
            core.open_orders(),
            |next| next.updated_at_ms = 61_000,
        )
        .await;

    let (reconnect_bootstrap, mut reconnect_receiver) =
        manager.stream_bootstrap(core.run_id).await.unwrap();
    assert_eq!(reconnect_bootstrap.stream_revision, 8);

    manager
        .update_snapshot(
            &handle,
            core.portfolio.view(),
            core.open_orders(),
            |next| next.updated_at_ms = 62_000,
        )
        .await;
    let update = reconnect_receiver
        .as_mut()
        .expect("reconnected live receiver")
        .recv()
        .await
        .unwrap();
    assert_eq!(update.stream_revision, 9);
    assert!(update.stream_revision > reconnect_bootstrap.stream_revision);

    core.stop(180_000, "phase4_verification_stop").unwrap();
    let after_first_stop = storage.trading_run_history(core.run_id).unwrap();
    let first_event_count = after_first_stop.events.len();
    assert_eq!(after_first_stop.run.status, RunStatus::Stopped);
    assert_eq!(
        after_first_stop
            .status_events
            .iter()
            .filter(|event| event.status == RunStatus::Stopped)
            .count(),
        1
    );

    core.stop(240_000, "phase4_verification_stop").unwrap();
    let after_second_stop = storage.trading_run_history(core.run_id).unwrap();
    assert_eq!(after_second_stop.events.len(), first_event_count);
    assert_eq!(
        after_second_stop
            .status_events
            .iter()
            .filter(|event| event.status == RunStatus::Stopped)
            .count(),
        1
    );

    drop(reconnect_receiver);
    drop(handle);
    drop(manager);
    drop(core);
    drop(storage);
    cleanup_database(&path);
}
