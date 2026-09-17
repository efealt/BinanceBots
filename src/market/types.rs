use serde::Serialize;
use thiserror::Error;

pub const MAX_CANDLES: usize = 1_000;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MarketKey {
    pub symbol: String,
    pub interval: String,
}

impl MarketKey {
    pub fn new(symbol: &str, interval: &str) -> Result<Self, MarketError> {
        let symbol = symbol.trim().to_uppercase();
        let interval = interval.trim().to_lowercase();

        if symbol.is_empty()
            || !symbol
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
        {
            return Err(MarketError::InvalidSelection(
                "symbol must be alphanumeric".into(),
            ));
        }

        if !matches!(interval.as_str(), "1m" | "5m" | "1h") {
            return Err(MarketError::InvalidSelection(
                "interval must be 1m, 5m, or 1h".into(),
            ));
        }

        Ok(Self { symbol, interval })
    }

    pub fn stream_name(&self) -> String {
        format!("{}@kline_{}", self.symbol.to_lowercase(), self.interval)
    }
}

#[derive(Clone, Serialize)]
pub struct Candle {
    pub open_time: i64,
    pub close_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub is_closed: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedStatus {
    Loading,
    Live,
    Reconnecting,
}

#[derive(Clone, Serialize)]
pub struct MarketSnapshot {
    pub symbol: String,
    pub interval: String,
    pub status: FeedStatus,
    pub candles: Vec<Candle>,
}

#[derive(Debug, Error)]
pub enum MarketError {
    #[error("invalid market selection: {0}")]
    InvalidSelection(String),
    #[error("Binance request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("unexpected Binance market-data response")]
    InvalidPayload,
}
