use super::types::*;
use super::super::{StorageError};
use super::super::reader::StorageReader;
use rusqlite::params;
use serde_json::Value;
use std::io;

impl StorageReader {
    pub fn trading_run(&self, run_id: i64) -> Result<TradingRun, StorageError> {
        let connection = self.open()?;
        connection
            .query_row(RUN_SELECT, params![run_id], map_trading_run)
            .optional()?
            .ok_or(StorageError::TradingRunNotFound(run_id))
    }

    pub fn trading_runs_by_comparison_id(
        &self,
        comparison_id: &str,
    ) -> Result<Vec<TradingRun>, StorageError> {
        if comparison_id.trim().is_empty() {
            return Err(StorageError::InvalidTradingValue {
                field: "comparison_id",
                value: comparison_id.to_string(),
            });
        }
        let connection = self.open()?;
        let sql = format!("{RUN_SELECT_BASE} WHERE comparison_id = ?1 ORDER BY run_id");
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![comparison_id], map_trading_run)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn trading_run_history(&self, run_id: i64) -> Result<TradingRunHistory, StorageError> {
        let run = self.trading_run(run_id)?;
        let connection = self.open()?;

        let events = query_rows(
            &connection,
            "SELECT event_id, run_id, run_sequence, event_kind, event_time_ms,
                    exchange_time_ms, received_at_ms, persisted_at_ms
             FROM trading_run_events
             WHERE run_id = ?1 ORDER BY run_sequence",
            run_id,
            map_event,
        )?;

        let status_events = query_rows(
            &connection,
            "SELECT e.event_id, e.run_id, e.run_sequence, e.event_kind, e.event_time_ms,
                    e.exchange_time_ms, e.received_at_ms, e.persisted_at_ms,
                    s.status, s.note
             FROM trading_run_status_events s
             JOIN trading_run_events e ON e.event_id = s.event_id
             WHERE s.run_id = ?1 ORDER BY e.run_sequence",
            run_id,
            map_status_record,
        )?;

        let decisions = query_rows(
            &connection,
            "SELECT e.event_id, e.run_id, e.run_sequence, e.event_kind, e.event_time_ms,
                    e.exchange_time_ms, e.received_at_ms, e.persisted_at_ms,
                    d.decision_type, d.payload_json
             FROM trading_decisions d
             JOIN trading_run_events e ON e.event_id = d.event_id
             WHERE d.run_id = ?1 ORDER BY e.run_sequence",
            run_id,
            map_decision_record,
        )?;

        let order_intents = query_rows(
            &connection,
            "SELECT e.event_id, e.run_id, e.run_sequence, e.event_kind, e.event_time_ms,
                    e.exchange_time_ms, e.received_at_ms, e.persisted_at_ms,
                    i.intent_key, i.side, i.order_type, i.time_in_force,
                    i.price_decimal, i.quantity_decimal, i.stop_price_decimal,
                    i.reduce_only, i.metadata_json
             FROM trading_order_intents i
             JOIN trading_run_events e ON e.event_id = i.event_id
             WHERE i.run_id = ?1 ORDER BY e.run_sequence",
            run_id,
            map_intent_record,
        )?;

        let orders = query_rows(
            &connection,
            "SELECT o.order_id, o.run_id,
                    e.event_id, e.run_id, e.run_sequence, e.event_kind, e.event_time_ms,
                    e.exchange_time_ms, e.received_at_ms, e.persisted_at_ms,
                    o.intent_event_id, o.client_order_id, o.exchange_order_id,
                    o.side, o.order_type, o.time_in_force, o.price_decimal,
                    o.quantity_decimal, o.stop_price_decimal, o.metadata_json
             FROM trading_orders o
             JOIN trading_run_events e ON e.event_id = o.created_event_id
             WHERE o.run_id = ?1 ORDER BY e.run_sequence",
            run_id,
            map_order_record,
        )?;

        let order_states = query_rows(
            &connection,
            "SELECT e.event_id, e.run_id, e.run_sequence, e.event_kind, e.event_time_ms,
                    e.exchange_time_ms, e.received_at_ms, e.persisted_at_ms,
                    s.order_id, s.status, s.filled_quantity_decimal,
                    s.average_fill_price_decimal, s.reject_reason, s.metadata_json
             FROM trading_order_state_events s
             JOIN trading_run_events e ON e.event_id = s.event_id
             WHERE s.run_id = ?1 ORDER BY e.run_sequence",
            run_id,
            map_order_state_record,
        )?;

        let fills = query_rows(
            &connection,
            "SELECT e.event_id, e.run_id, e.run_sequence, e.event_kind, e.event_time_ms,
                    e.exchange_time_ms, e.received_at_ms, e.persisted_at_ms,
                    f.order_id, f.exchange_trade_id, f.price_decimal, f.quantity_decimal,
                    f.fee_decimal, f.fee_asset, f.liquidity_role, f.metadata_json
             FROM trading_fills f
             JOIN trading_run_events e ON e.event_id = f.event_id
             WHERE f.run_id = ?1 ORDER BY e.run_sequence",
            run_id,
            map_fill_record,
        )?;

        let positions = query_rows(
            &connection,
            "SELECT e.event_id, e.run_id, e.run_sequence, e.event_kind, e.event_time_ms,
                    e.exchange_time_ms, e.received_at_ms, e.persisted_at_ms,
                    p.position_quantity_decimal, p.average_entry_price_decimal,
                    p.mark_price_decimal, p.realized_pnl_decimal, p.unrealized_pnl_decimal,
                    p.cash_balance_decimal, p.metadata_json
             FROM trading_position_snapshots p
             JOIN trading_run_events e ON e.event_id = p.event_id
             WHERE p.run_id = ?1 ORDER BY e.run_sequence",
            run_id,
            map_position_record,
        )?;

        let equity = query_rows(
            &connection,
            "SELECT e.event_id, e.run_id, e.run_sequence, e.event_kind, e.event_time_ms,
                    e.exchange_time_ms, e.received_at_ms, e.persisted_at_ms,
                    q.equity_decimal, q.cash_balance_decimal, q.realized_pnl_decimal,
                    q.unrealized_pnl_decimal, q.fees_paid_decimal, q.metadata_json
             FROM trading_equity_snapshots q
             JOIN trading_run_events e ON e.event_id = q.event_id
             WHERE q.run_id = ?1 ORDER BY e.run_sequence",
            run_id,
            map_equity_record,
        )?;

        Ok(TradingRunHistory {
            run,
            events,
            status_events,
            decisions,
            order_intents,
            orders,
            order_states,
            fills,
            positions,
            equity,
        })
    }
}

use rusqlite::OptionalExtension;

const RUN_SELECT_BASE: &str =
    "SELECT run_id, comparison_id, mode, status, strategy_id, strategy_version,
            strategy_params_json, instrument_id, initial_capital_decimal,
            run_config_json, data_source_json, execution_assumptions_json,
            started_at_ms, ended_at_ms, created_at_ms, updated_at_ms
     FROM trading_runs";
const RUN_SELECT: &str =
    "SELECT run_id, comparison_id, mode, status, strategy_id, strategy_version,
            strategy_params_json, instrument_id, initial_capital_decimal,
            run_config_json, data_source_json, execution_assumptions_json,
            started_at_ms, ended_at_ms, created_at_ms, updated_at_ms
     FROM trading_runs WHERE run_id = ?1";

fn query_rows<T>(
    connection: &rusqlite::Connection,
    sql: &str,
    run_id: i64,
    mapper: fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, StorageError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params![run_id], mapper)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn map_trading_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<TradingRun> {
    Ok(TradingRun {
        run_id: row.get(0)?,
        comparison_id: row.get(1)?,
        mode: enum_from_row(row, 2, RunMode::parse)?,
        status: enum_from_row(row, 3, RunStatus::parse)?,
        strategy_id: row.get(4)?,
        strategy_version: row.get(5)?,
        strategy_params: json_from_row(row, 6)?,
        instrument_id: row.get(7)?,
        initial_capital: decimal_from_row(row, 8)?,
        run_config: json_from_row(row, 9)?,
        data_source: json_from_row(row, 10)?,
        execution_assumptions: json_from_row(row, 11)?,
        started_at_ms: row.get(12)?,
        ended_at_ms: row.get(13)?,
        created_at_ms: row.get(14)?,
        updated_at_ms: row.get(15)?,
    })
}

fn map_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<TradingRunEvent> {
    map_event_at(row, 0)
}

fn map_event_at(
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> rusqlite::Result<TradingRunEvent> {
    Ok(TradingRunEvent {
        event_id: row.get(offset)?,
        run_id: row.get(offset + 1)?,
        run_sequence: row.get(offset + 2)?,
        event_kind: enum_from_row(row, offset + 3, RunEventKind::parse)?,
        event_time_ms: row.get(offset + 4)?,
        exchange_time_ms: row.get(offset + 5)?,
        received_at_ms: row.get(offset + 6)?,
        persisted_at_ms: row.get(offset + 7)?,
    })
}

fn map_status_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunStatusRecord> {
    Ok(RunStatusRecord {
        event: map_event_at(row, 0)?,
        status: enum_from_row(row, 8, RunStatus::parse)?,
        note: row.get(9)?,
    })
}

fn map_decision_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<DecisionRecord> {
    Ok(DecisionRecord {
        event: map_event_at(row, 0)?,
        decision_type: row.get(8)?,
        payload: json_from_row(row, 9)?,
    })
}

fn map_intent_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<OrderIntentRecord> {
    Ok(OrderIntentRecord {
        event: map_event_at(row, 0)?,
        intent_key: row.get(8)?,
        side: enum_from_row(row, 9, OrderSide::parse)?,
        order_type: enum_from_row(row, 10, OrderType::parse)?,
        time_in_force: optional_enum_from_row(row, 11, TimeInForce::parse)?,
        price: optional_decimal_from_row(row, 12)?,
        quantity: decimal_from_row(row, 13)?,
        stop_price: optional_decimal_from_row(row, 14)?,
        reduce_only: row.get::<_, i64>(15)? != 0,
        metadata: json_from_row(row, 16)?,
    })
}

fn map_order_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<TradingOrder> {
    Ok(TradingOrder {
        order_id: row.get(0)?,
        run_id: row.get(1)?,
        created_event: map_event_at(row, 2)?,
        intent_event_id: row.get(10)?,
        client_order_id: row.get(11)?,
        exchange_order_id: row.get(12)?,
        side: enum_from_row(row, 13, OrderSide::parse)?,
        order_type: enum_from_row(row, 14, OrderType::parse)?,
        time_in_force: optional_enum_from_row(row, 15, TimeInForce::parse)?,
        price: optional_decimal_from_row(row, 16)?,
        quantity: decimal_from_row(row, 17)?,
        stop_price: optional_decimal_from_row(row, 18)?,
        metadata: json_from_row(row, 19)?,
    })
}

fn map_order_state_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<OrderStateRecord> {
    Ok(OrderStateRecord {
        event: map_event_at(row, 0)?,
        order_id: row.get(8)?,
        status: enum_from_row(row, 9, OrderStatus::parse)?,
        filled_quantity: decimal_from_row(row, 10)?,
        average_fill_price: optional_decimal_from_row(row, 11)?,
        reject_reason: row.get(12)?,
        metadata: json_from_row(row, 13)?,
    })
}

fn map_fill_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<FillRecord> {
    Ok(FillRecord {
        event: map_event_at(row, 0)?,
        order_id: row.get(8)?,
        exchange_trade_id: row.get(9)?,
        price: decimal_from_row(row, 10)?,
        quantity: decimal_from_row(row, 11)?,
        fee: optional_decimal_from_row(row, 12)?,
        fee_asset: row.get(13)?,
        liquidity_role: optional_enum_from_row(row, 14, LiquidityRole::parse)?,
        metadata: json_from_row(row, 15)?,
    })
}

fn map_position_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<PositionSnapshotRecord> {
    Ok(PositionSnapshotRecord {
        event: map_event_at(row, 0)?,
        position_quantity: decimal_from_row(row, 8)?,
        average_entry_price: optional_decimal_from_row(row, 9)?,
        mark_price: optional_decimal_from_row(row, 10)?,
        realized_pnl: optional_decimal_from_row(row, 11)?,
        unrealized_pnl: optional_decimal_from_row(row, 12)?,
        cash_balance: optional_decimal_from_row(row, 13)?,
        metadata: json_from_row(row, 14)?,
    })
}

fn map_equity_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<EquitySnapshotRecord> {
    Ok(EquitySnapshotRecord {
        event: map_event_at(row, 0)?,
        equity: decimal_from_row(row, 8)?,
        cash_balance: optional_decimal_from_row(row, 9)?,
        realized_pnl: optional_decimal_from_row(row, 10)?,
        unrealized_pnl: optional_decimal_from_row(row, 11)?,
        fees_paid: optional_decimal_from_row(row, 12)?,
        metadata: json_from_row(row, 13)?,
    })
}

fn enum_from_row<T>(
    row: &rusqlite::Row<'_>,
    index: usize,
    parser: fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    let value: String = row.get(index)?;
    parser(&value).ok_or_else(|| conversion_failure(index, format!("invalid enum value: {value}")))
}

fn optional_enum_from_row<T>(
    row: &rusqlite::Row<'_>,
    index: usize,
    parser: fn(&str) -> Option<T>,
) -> rusqlite::Result<Option<T>> {
    let value: Option<String> = row.get(index)?;
    value
        .map(|value| {
            parser(&value)
                .ok_or_else(|| conversion_failure(index, format!("invalid enum value: {value}")))
        })
        .transpose()
}

fn decimal_from_row(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<ExactDecimal> {
    let value: String = row.get(index)?;
    ExactDecimal::new(&value)
        .map_err(|_| conversion_failure(index, format!("invalid exact decimal: {value}")))
}

fn optional_decimal_from_row(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<ExactDecimal>> {
    let value: Option<String> = row.get(index)?;
    value
        .map(|value| {
            ExactDecimal::new(&value)
                .map_err(|_| conversion_failure(index, format!("invalid exact decimal: {value}")))
        })
        .transpose()
}

fn json_from_row(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Value> {
    let value: String = row.get(index)?;
    serde_json::from_str(&value)
        .map_err(|error| conversion_failure(index, format!("invalid JSON: {error}")))
}

fn conversion_failure(index: usize, message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(io::Error::new(io::ErrorKind::InvalidData, message)),
    )
}
