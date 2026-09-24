use super::{StorageError, now_ms};
use super::reader::StorageReader;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct TradingBotSpec {
    pub bot_name: String,
    pub config: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct TradingBot {
    pub bot_id: i64,
    pub bot_name: String,
    pub config: Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl StorageReader {
    pub fn create_trading_bot(&self, spec: &TradingBotSpec) -> Result<TradingBot, StorageError> {
        self.initialize()?;
        validate_bot_spec(spec)?;
        let connection = self.open_write()?;
        let now = now_ms();
        connection.execute(
            "INSERT INTO trading_bots (bot_name, config_json, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?3)",
            params![spec.bot_name.trim(), serde_json::to_string(&spec.config)?, now],
        )?;
        self.trading_bot(connection.last_insert_rowid())
    }

    pub fn update_trading_bot(&self, bot_id: i64, spec: &TradingBotSpec) -> Result<TradingBot, StorageError> {
        self.initialize()?;
        validate_bot_id(bot_id)?;
        validate_bot_spec(spec)?;
        let connection = self.open_write()?;
        let updated = connection.execute(
            "UPDATE trading_bots
             SET bot_name = ?1, config_json = ?2, updated_at_ms = ?3
             WHERE bot_id = ?4",
            params![spec.bot_name.trim(), serde_json::to_string(&spec.config)?, now_ms(), bot_id],
        )?;
        if updated == 0 {
            return Err(StorageError::TradingBotNotFound(bot_id));
        }
        self.trading_bot(bot_id)
    }

    pub fn trading_bot(&self, bot_id: i64) -> Result<TradingBot, StorageError> {
        validate_bot_id(bot_id)?;
        let connection = self.open()?;
        connection
            .query_row(
                "SELECT bot_id, bot_name, config_json, created_at_ms, updated_at_ms
                 FROM trading_bots WHERE bot_id = ?1",
                params![bot_id],
                map_trading_bot,
            )
            .optional()?
            .ok_or(StorageError::TradingBotNotFound(bot_id))
    }

    pub fn trading_bots(&self) -> Result<Vec<TradingBot>, StorageError> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT bot_id, bot_name, config_json, created_at_ms, updated_at_ms
             FROM trading_bots
             ORDER BY updated_at_ms DESC, bot_id DESC",
        )?;
        let rows = statement.query_map([], map_trading_bot)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn validate_bot_id(bot_id: i64) -> Result<(), StorageError> {
    if bot_id <= 0 {
        return Err(StorageError::InvalidTradingValue {
            field: "bot_id",
            value: bot_id.to_string(),
        });
    }
    Ok(())
}

fn validate_bot_spec(spec: &TradingBotSpec) -> Result<(), StorageError> {
    let name = spec.bot_name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(StorageError::InvalidTradingValue {
            field: "bot_name",
            value: spec.bot_name.clone(),
        });
    }
    if !spec.config.is_object() {
        return Err(StorageError::InvalidTradingValue {
            field: "bot.config",
            value: spec.config.to_string(),
        });
    }
    Ok(())
}

fn map_trading_bot(row: &rusqlite::Row<'_>) -> rusqlite::Result<TradingBot> {
    let config_json: String = row.get(2)?;
    let config = serde_json::from_str(&config_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    Ok(TradingBot {
        bot_id: row.get(0)?,
        bot_name: row.get(1)?,
        config,
        created_at_ms: row.get(3)?,
        updated_at_ms: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ExactDecimal, RunMode, TradingRunSpec};
    use rusqlite::Connection;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_database(label: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("binance-grid-bots-{label}-{}-{suffix}.sqlite3", std::process::id()))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    fn instrument(path: &std::path::Path) -> i64 {
        let connection = Connection::open(path).unwrap();
        connection.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        connection.execute(
            "INSERT INTO market_instruments (venue, market_type, symbol, created_at_ms)
             VALUES ('binance', 'spot', 'BTCUSDT', 1)",
            [],
        ).unwrap();
        connection.last_insert_rowid()
    }

    fn run_spec(bot_id: i64, instrument_id: i64, spacing: i64) -> TradingRunSpec {
        TradingRunSpec {
            bot_id: Some(bot_id),
            comparison_id: None,
            mode: RunMode::Paper,
            strategy_id: "static-grid-fixture".into(),
            strategy_version: "1".into(),
            strategy_params: json!({"spacing_bps": spacing}),
            instrument_id,
            initial_capital: ExactDecimal::new("1000").unwrap(),
            run_config: json!({"spacing_bps": spacing}),
            data_source: json!({"kind": "test", "symbol": "BTCUSDT"}),
            execution_assumptions: json!({"fee_bps": 1}),
        }
    }

    #[test]
    fn bot_can_be_saved_without_creating_a_run() {
        let path = temp_database("save-only");
        let reader = StorageReader::new(path.clone());
        reader.initialize().unwrap();
        let bot = reader.create_trading_bot(&TradingBotSpec {
            bot_name: "BTC Grid".into(),
            config: json!({"symbol": "BTCUSDT", "spacing_bps": 25}),
        }).unwrap();
        assert_eq!(bot.bot_name, "BTC Grid");
        assert_eq!(reader.trading_bots().unwrap().len(), 1);
        assert!(reader.trading_runs_by_mode(RunMode::Paper, 10).unwrap().is_empty());
        drop(reader);
        cleanup(&path);
    }

    #[test]
    fn editing_bot_does_not_rewrite_prior_run_snapshots() {
        let path = temp_database("immutable-runs");
        let reader = StorageReader::new(path.clone());
        reader.initialize().unwrap();
        let instrument_id = instrument(&path);
        let bot = reader.create_trading_bot(&TradingBotSpec {
            bot_name: "BTC Grid".into(),
            config: json!({"spacing_bps": 25}),
        }).unwrap();
        let first = reader.create_trading_run(&run_spec(bot.bot_id, instrument_id, 25)).unwrap();
        let updated = reader.update_trading_bot(bot.bot_id, &TradingBotSpec {
            bot_name: "BTC Grid Revised".into(),
            config: json!({"spacing_bps": 40}),
        }).unwrap();
        let second = reader.create_trading_run(&run_spec(bot.bot_id, instrument_id, 40)).unwrap();
        let first_after = reader.trading_run(first.run_id).unwrap();
        let second_after = reader.trading_run(second.run_id).unwrap();
        assert_eq!(updated.config["spacing_bps"], 40);
        assert_eq!(first_after.bot_id, Some(bot.bot_id));
        assert_eq!(first_after.run_config["spacing_bps"], 25);
        assert_eq!(second_after.run_config["spacing_bps"], 40);
        drop(reader);
        cleanup(&path);
    }

    #[test]
    fn paper_run_requires_a_real_bot() {
        let path = temp_database("required-link");
        let reader = StorageReader::new(path.clone());
        reader.initialize().unwrap();
        let instrument_id = instrument(&path);
        let mut missing = run_spec(999_999, instrument_id, 25);
        assert!(matches!(
            reader.create_trading_run(&missing),
            Err(StorageError::TradingBotNotFound(999_999))
        ));
        missing.bot_id = None;
        assert!(matches!(
            reader.create_trading_run(&missing),
            Err(StorageError::InvalidTradingValue { field: "bot_id", .. })
        ));
        drop(reader);
        cleanup(&path);
    }
}
