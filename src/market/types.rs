use serde::Serialize;
use thiserror::Error;

pub const MAX_CANDLES: usize = 1_000;
pub const MAX_RECENT_TRADES: usize = 40;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MarketType {
    Spot,
    UsdMarginedPerpetual,
}

impl MarketType {
    pub fn parse(value: &str) -> Result<Self, MarketError> {
        if value.eq_ignore_ascii_case("spot") {
            return Ok(Self::Spot);
        }
        if value.eq_ignore_ascii_case("usd_m_perpetual") {
            return Ok(Self::UsdMarginedPerpetual);
        }

        Err(MarketError::InvalidSelection(
            "market_type must be spot or usd_m_perpetual".into(),
        ))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MarketKey {
    pub symbol: String,
    pub interval: String,
    pub market_type: MarketType,
}

impl MarketKey {
    pub fn new(symbol: &str, interval: &str, market_type: MarketType) -> Result<Self, MarketError> {
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

        Ok(Self {
            symbol,
            interval,
            market_type,
        })
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

#[derive(Clone, Default, Serialize)]
pub struct MarketQuote {
    pub update_id: Option<u64>,
    pub best_bid: Option<f64>,
    pub best_bid_quantity: Option<f64>,
    pub best_ask: Option<f64>,
    pub best_ask_quantity: Option<f64>,
    pub spread: Option<f64>,
    pub mid_price: Option<f64>,
}

#[derive(Clone, Default, Serialize)]
pub struct OrderBookLevel {
    pub price: f64,
    pub quantity: f64,
}

#[derive(Clone, Default, Serialize)]
pub struct OrderBookSnapshot {
    pub update_id: Option<u64>,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
}

#[derive(Clone, Serialize)]
pub struct MarketTrade {
    pub trade_id: u64,
    pub price: f64,
    pub quantity: f64,
    pub trade_time: i64,
    pub is_buyer_maker: bool,
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
    pub quote: MarketQuote,
    pub order_book: OrderBookSnapshot,
    pub trades: Vec<MarketTrade>,
}

#[derive(Clone, Serialize)]
pub struct MarketUpdate {
    pub status: FeedStatus,
    pub candle: Option<Candle>,
    pub quote: MarketQuote,
    pub order_book: OrderBookSnapshot,
    pub trades: Vec<MarketTrade>,
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
