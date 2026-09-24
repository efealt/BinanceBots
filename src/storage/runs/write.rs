use super::types::*;
use super::super::{StorageError, now_ms};
use super::super::reader::StorageReader;
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::Value;

impl StorageReader {
    pub fn create_trading_run(&self, spec: &TradingRunSpec) -> Result<TradingRun, StorageError> {
        self.initialize()?;
        validate_nonempty("strategy_id", &spec.strategy_id)?;
        validate_nonempty("strategy_version", &spec.strategy_version)?;
        if let Some(comparison_id) = spec.comparison_id.as_deref() {
            validate_nonempty("comparison_id", comparison_id)?;
        }
        if spec.mode == RunMode::Paper && spec.bot_id.is_none() {
            return Err(StorageError::InvalidTradingValue {
                field: "bot_id",
                value: "required for Live-Paper runs".into(),
            });
        }

        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        if let Some(bot_id) = spec.bot_id {
            let bot_exists: Option<i64> = transaction
                .query_row(
                    "SELECT bot_id FROM trading_bots WHERE bot_id = ?1",
                    params![bot_id],
                    |row| row.get(0),
                )
                .optional()?;
            if bot_exists.is_none() {
                return Err(StorageError::TradingBotNotFound(bot_id));
            }
        }

        let instrument_exists: Option<i64> = transaction
            .query_row(
                "SELECT instrument_id FROM market_instruments WHERE instrument_id = ?1",
                params![spec.instrument_id],
                |row| row.get(0),
            )
            .optional()?;
        if instrument_exists.is_none() {
            return Err(StorageError::TradingInstrumentNotFound(spec.instrument_id));
        }

        let now = now_ms();
        transaction.execute(
            "INSERT INTO trading_runs (
                bot_id, comparison_id, mode, status, strategy_id, strategy_version,
                strategy_params_json, instrument_id, initial_capital_decimal,
                run_config_json, data_source_json, execution_assumptions_json,
                created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, 'created', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            params![
                spec.bot_id,
                spec.comparison_id,
                spec.mode.as_str(),
                spec.strategy_id,
                spec.strategy_version,
                json_string(&spec.strategy_params)?,
                spec.instrument_id,
                spec.initial_capital.as_str(),
                json_string(&spec.run_config)?,
                json_string(&spec.data_source)?,
                json_string(&spec.execution_assumptions)?,
                now,
            ],
        )?;
        let run_id = transaction.last_insert_rowid();
        let event = allocate_event(
            &transaction,
            run_id,
            RunEventKind::RunStatus,
            EventTimes::new(now),
        )?;
        transaction.execute(
            "INSERT INTO trading_run_status_events (event_id, run_id, status)
             VALUES (?1, ?2, 'created')",
            params![event.event_id, run_id],
        )?;
        transaction.commit()?;
        self.trading_run(run_id)
    }

    pub fn set_trading_run_status(
        &self,
        run_id: i64,
        status: RunStatus,
        times: EventTimes,
        note: Option<&str>,
    ) -> Result<RunStatusRecord, StorageError> {
        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let current_raw: Option<String> = transaction
            .query_row(
                "SELECT status FROM trading_runs WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()?;
        let current_raw = current_raw.ok_or(StorageError::TradingRunNotFound(run_id))?;
        let current = RunStatus::parse(&current_raw)
            .ok_or_else(|| StorageError::InvalidTradingValue {
                field: "run.status",
                value: current_raw,
            })?;
        if !valid_run_transition(current, status) {
            return Err(StorageError::InvalidRunStatusTransition {
                from: current.as_str(),
                to: status.as_str(),
            });
        }

        let event = allocate_event(&transaction, run_id, RunEventKind::RunStatus, times)?;
        let started_at_ms = (status == RunStatus::Running).then_some(times.event_time_ms);
        let ended_at_ms = status.terminal().then_some(times.event_time_ms);
        transaction.execute(
            "UPDATE trading_runs
             SET status = ?1,
                 started_at_ms = COALESCE(started_at_ms, ?2),
                 ended_at_ms = COALESCE(?3, ended_at_ms),
                 updated_at_ms = ?4
             WHERE run_id = ?5",
            params![
                status.as_str(),
                started_at_ms,
                ended_at_ms,
                event.persisted_at_ms,
                run_id,
            ],
        )?;
        transaction.execute(
            "INSERT INTO trading_run_status_events (event_id, run_id, status, note)
             VALUES (?1, ?2, ?3, ?4)",
            params![event.event_id, run_id, status.as_str(), note],
        )?;
        transaction.commit()?;
        Ok(RunStatusRecord {
            event,
            status,
            note: note.map(str::to_string),
        })
    }

    pub fn record_decision(&self, input: &DecisionInput) -> Result<DecisionRecord, StorageError> {
        validate_nonempty("decision_type", &input.decision_type)?;
        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let event = allocate_event(
            &transaction,
            input.run_id,
            RunEventKind::Decision,
            input.times,
        )?;
        transaction.execute(
            "INSERT INTO trading_decisions (event_id, run_id, decision_type, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                event.event_id,
                input.run_id,
                input.decision_type,
                json_string(&input.payload)?,
            ],
        )?;
        transaction.commit()?;
        Ok(DecisionRecord {
            event,
            decision_type: input.decision_type.clone(),
            payload: input.payload.clone(),
        })
    }

    pub fn record_order_intent(
        &self,
        input: &OrderIntentInput,
    ) -> Result<OrderIntentRecord, StorageError> {
        if let Some(intent_key) = input.intent_key.as_deref() {
            validate_nonempty("intent_key", intent_key)?;
        }
        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let event = allocate_event(
            &transaction,
            input.run_id,
            RunEventKind::OrderIntent,
            input.times,
        )?;
        transaction.execute(
            "INSERT INTO trading_order_intents (
                event_id, run_id, intent_key, side, order_type, time_in_force,
                price_decimal, quantity_decimal, stop_price_decimal, reduce_only, metadata_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                event.event_id,
                input.run_id,
                input.intent_key,
                input.side.as_str(),
                input.order_type.as_str(),
                input.time_in_force.map(TimeInForce::as_str),
                input.price.as_ref().map(ExactDecimal::as_str),
                input.quantity.as_str(),
                input.stop_price.as_ref().map(ExactDecimal::as_str),
                if input.reduce_only { 1_i64 } else { 0_i64 },
                json_string(&input.metadata)?,
            ],
        )?;
        transaction.commit()?;
        Ok(OrderIntentRecord {
            event,
            intent_key: input.intent_key.clone(),
            side: input.side,
            order_type: input.order_type,
            time_in_force: input.time_in_force,
            price: input.price.clone(),
            quantity: input.quantity.clone(),
            stop_price: input.stop_price.clone(),
            reduce_only: input.reduce_only,
            metadata: input.metadata.clone(),
        })
    }

    pub fn create_trading_order(
        &self,
        input: &CreateOrderInput,
    ) -> Result<TradingOrder, StorageError> {
        if let Some(value) = input.client_order_id.as_deref() {
            validate_nonempty("client_order_id", value)?;
        }
        if let Some(value) = input.exchange_order_id.as_deref() {
            validate_nonempty("exchange_order_id", value)?;
        }

        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        if let Some(intent_event_id) = input.intent_event_id {
            let intent_exists: Option<i64> = transaction
                .query_row(
                    "SELECT event_id FROM trading_order_intents
                     WHERE event_id = ?1 AND run_id = ?2",
                    params![intent_event_id, input.run_id],
                    |row| row.get(0),
                )
                .optional()?;
            if intent_exists.is_none() {
                return Err(StorageError::TradingIntentNotFound(intent_event_id));
            }
        }

        let event = allocate_event(
            &transaction,
            input.run_id,
            RunEventKind::OrderState,
            input.times,
        )?;
        transaction.execute(
            "INSERT INTO trading_orders (
                run_id, created_event_id, intent_event_id, client_order_id, exchange_order_id,
                side, order_type, time_in_force, price_decimal, quantity_decimal,
                stop_price_decimal, metadata_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                input.run_id,
                event.event_id,
                input.intent_event_id,
                input.client_order_id,
                input.exchange_order_id,
                input.side.as_str(),
                input.order_type.as_str(),
                input.time_in_force.map(TimeInForce::as_str),
                input.price.as_ref().map(ExactDecimal::as_str),
                input.quantity.as_str(),
                input.stop_price.as_ref().map(ExactDecimal::as_str),
                json_string(&input.metadata)?,
            ],
        )?;
        let order_id = transaction.last_insert_rowid();
        transaction.execute(
            "INSERT INTO trading_order_state_events (
                event_id, run_id, order_id, status, filled_quantity_decimal, metadata_json
             ) VALUES (?1, ?2, ?3, 'created', '0', '{}')",
            params![event.event_id, input.run_id, order_id],
        )?;
        transaction.commit()?;

        Ok(TradingOrder {
            order_id,
            run_id: input.run_id,
            created_event: event,
            intent_event_id: input.intent_event_id,
            client_order_id: input.client_order_id.clone(),
            exchange_order_id: input.exchange_order_id.clone(),
            side: input.side,
            order_type: input.order_type,
            time_in_force: input.time_in_force,
            price: input.price.clone(),
            quantity: input.quantity.clone(),
            stop_price: input.stop_price.clone(),
            metadata: input.metadata.clone(),
        })
    }

    pub fn record_order_state(
        &self,
        input: &OrderStateInput,
    ) -> Result<OrderStateRecord, StorageError> {
        if input.status == OrderStatus::Created {
            return Err(StorageError::InvalidTradingValue {
                field: "order.status",
                value: "created can only be emitted by create_trading_order".to_string(),
            });
        }
        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let run_id = order_run_id(&transaction, input.order_id)?;
        let event = allocate_event(
            &transaction,
            run_id,
            RunEventKind::OrderState,
            input.times,
        )?;
        transaction.execute(
            "INSERT INTO trading_order_state_events (
                event_id, run_id, order_id, status, filled_quantity_decimal,
                average_fill_price_decimal, reject_reason, metadata_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.event_id,
                run_id,
                input.order_id,
                input.status.as_str(),
                input.filled_quantity.as_str(),
                input.average_fill_price.as_ref().map(ExactDecimal::as_str),
                input.reject_reason,
                json_string(&input.metadata)?,
            ],
        )?;
        transaction.commit()?;
        Ok(OrderStateRecord {
            event,
            order_id: input.order_id,
            status: input.status,
            filled_quantity: input.filled_quantity.clone(),
            average_fill_price: input.average_fill_price.clone(),
            reject_reason: input.reject_reason.clone(),
            metadata: input.metadata.clone(),
        })
    }

    pub fn record_fill(&self, input: &FillInput) -> Result<FillRecord, StorageError> {
        if let Some(value) = input.exchange_trade_id.as_deref() {
            validate_nonempty("exchange_trade_id", value)?;
        }
        if let Some(value) = input.fee_asset.as_deref() {
            validate_nonempty("fee_asset", value)?;
        }

        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let run_id = order_run_id(&transaction, input.order_id)?;
        let event = allocate_event(&transaction, run_id, RunEventKind::Fill, input.times)?;
        transaction.execute(
            "INSERT INTO trading_fills (
                event_id, run_id, order_id, exchange_trade_id, price_decimal,
                quantity_decimal, fee_decimal, fee_asset, liquidity_role, metadata_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                event.event_id,
                run_id,
                input.order_id,
                input.exchange_trade_id,
                input.price.as_str(),
                input.quantity.as_str(),
                input.fee.as_ref().map(ExactDecimal::as_str),
                input.fee_asset,
                input.liquidity_role.map(LiquidityRole::as_str),
                json_string(&input.metadata)?,
            ],
        )?;
        transaction.commit()?;
        Ok(FillRecord {
            event,
            order_id: input.order_id,
            exchange_trade_id: input.exchange_trade_id.clone(),
            price: input.price.clone(),
            quantity: input.quantity.clone(),
            fee: input.fee.clone(),
            fee_asset: input.fee_asset.clone(),
            liquidity_role: input.liquidity_role,
            metadata: input.metadata.clone(),
        })
    }

    pub fn record_position_snapshot(
        &self,
        input: &PositionSnapshotInput,
    ) -> Result<PositionSnapshotRecord, StorageError> {
        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let event = allocate_event(
            &transaction,
            input.run_id,
            RunEventKind::Position,
            input.times,
        )?;
        transaction.execute(
            "INSERT INTO trading_position_snapshots (
                event_id, run_id, position_quantity_decimal, average_entry_price_decimal,
                mark_price_decimal, realized_pnl_decimal, unrealized_pnl_decimal,
                cash_balance_decimal, metadata_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                event.event_id,
                input.run_id,
                input.position_quantity.as_str(),
                input.average_entry_price.as_ref().map(ExactDecimal::as_str),
                input.mark_price.as_ref().map(ExactDecimal::as_str),
                input.realized_pnl.as_ref().map(ExactDecimal::as_str),
                input.unrealized_pnl.as_ref().map(ExactDecimal::as_str),
                input.cash_balance.as_ref().map(ExactDecimal::as_str),
                json_string(&input.metadata)?,
            ],
        )?;
        transaction.commit()?;
        Ok(PositionSnapshotRecord {
            event,
            position_quantity: input.position_quantity.clone(),
            average_entry_price: input.average_entry_price.clone(),
            mark_price: input.mark_price.clone(),
            realized_pnl: input.realized_pnl.clone(),
            unrealized_pnl: input.unrealized_pnl.clone(),
            cash_balance: input.cash_balance.clone(),
            metadata: input.metadata.clone(),
        })
    }

    pub fn record_equity_snapshots(
        &self,
        inputs: &[EquitySnapshotInput],
    ) -> Result<Vec<EquitySnapshotRecord>, StorageError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let mut records = Vec::with_capacity(inputs.len());
        for input in inputs {
            let event = allocate_event(
                &transaction,
                input.run_id,
                RunEventKind::Equity,
                input.times,
            )?;
            transaction.execute(
                "INSERT INTO trading_equity_snapshots (
                    event_id, run_id, equity_decimal, cash_balance_decimal,
                    realized_pnl_decimal, unrealized_pnl_decimal, fees_paid_decimal, metadata_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    event.event_id,
                    input.run_id,
                    input.equity.as_str(),
                    input.cash_balance.as_ref().map(ExactDecimal::as_str),
                    input.realized_pnl.as_ref().map(ExactDecimal::as_str),
                    input.unrealized_pnl.as_ref().map(ExactDecimal::as_str),
                    input.fees_paid.as_ref().map(ExactDecimal::as_str),
                    json_string(&input.metadata)?,
                ],
            )?;
            records.push(EquitySnapshotRecord {
                event,
                equity: input.equity.clone(),
                cash_balance: input.cash_balance.clone(),
                realized_pnl: input.realized_pnl.clone(),
                unrealized_pnl: input.unrealized_pnl.clone(),
                fees_paid: input.fees_paid.clone(),
                metadata: input.metadata.clone(),
            });
        }
        transaction.commit()?;
        Ok(records)
    }

    pub fn record_equity_snapshot(
        &self,
        input: &EquitySnapshotInput,
    ) -> Result<EquitySnapshotRecord, StorageError> {
        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let event = allocate_event(
            &transaction,
            input.run_id,
            RunEventKind::Equity,
            input.times,
        )?;
        transaction.execute(
            "INSERT INTO trading_equity_snapshots (
                event_id, run_id, equity_decimal, cash_balance_decimal,
                realized_pnl_decimal, unrealized_pnl_decimal, fees_paid_decimal, metadata_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.event_id,
                input.run_id,
                input.equity.as_str(),
                input.cash_balance.as_ref().map(ExactDecimal::as_str),
                input.realized_pnl.as_ref().map(ExactDecimal::as_str),
                input.unrealized_pnl.as_ref().map(ExactDecimal::as_str),
                input.fees_paid.as_ref().map(ExactDecimal::as_str),
                json_string(&input.metadata)?,
            ],
        )?;
        transaction.commit()?;
        Ok(EquitySnapshotRecord {
            event,
            equity: input.equity.clone(),
            cash_balance: input.cash_balance.clone(),
            realized_pnl: input.realized_pnl.clone(),
            unrealized_pnl: input.unrealized_pnl.clone(),
            fees_paid: input.fees_paid.clone(),
            metadata: input.metadata.clone(),
        })
    }
}

fn valid_run_transition(from: RunStatus, to: RunStatus) -> bool {
    matches!(
        (from, to),
        (RunStatus::Created, RunStatus::Running)
            | (RunStatus::Created, RunStatus::Failed)
            | (RunStatus::Created, RunStatus::Stopped)
            | (RunStatus::Running, RunStatus::Completed)
            | (RunStatus::Running, RunStatus::Failed)
            | (RunStatus::Running, RunStatus::Stopped)
    )
}

fn allocate_event(
    transaction: &Transaction<'_>,
    run_id: i64,
    event_kind: RunEventKind,
    times: EventTimes,
) -> Result<TradingRunEvent, StorageError> {
    let persisted_at_ms = now_ms();
    let run_sequence: Option<i64> = transaction
        .query_row(
            "UPDATE trading_runs
             SET next_event_sequence = next_event_sequence + 1, updated_at_ms = ?1
             WHERE run_id = ?2
             RETURNING next_event_sequence",
            params![persisted_at_ms, run_id],
            |row| row.get(0),
        )
        .optional()?;
    let run_sequence = run_sequence.ok_or(StorageError::TradingRunNotFound(run_id))?;
    transaction.execute(
        "INSERT INTO trading_run_events (
            run_id, run_sequence, event_kind, event_time_ms,
            exchange_time_ms, received_at_ms, persisted_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            run_id,
            run_sequence,
            event_kind.as_str(),
            times.event_time_ms,
            times.exchange_time_ms,
            times.received_at_ms,
            persisted_at_ms,
        ],
    )?;
    Ok(TradingRunEvent {
        event_id: transaction.last_insert_rowid(),
        run_id,
        run_sequence,
        event_kind,
        event_time_ms: times.event_time_ms,
        exchange_time_ms: times.exchange_time_ms,
        received_at_ms: times.received_at_ms,
        persisted_at_ms,
    })
}

fn order_run_id(transaction: &Transaction<'_>, order_id: i64) -> Result<i64, StorageError> {
    transaction
        .query_row(
            "SELECT run_id FROM trading_orders WHERE order_id = ?1",
            params![order_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StorageError::TradingOrderNotFound(order_id))
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        return Err(StorageError::InvalidTradingValue {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn json_string(value: &Value) -> Result<String, StorageError> {
    Ok(serde_json::to_string(value)?)
}
