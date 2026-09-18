mod binance;
mod order_book;
mod service;
mod stream;
mod trades;
mod types;

pub use binance::BinanceMarketClient;
pub use service::MarketService;
pub use stream::{MarketEvent, parse_market_event};
pub use types::{
    Candle, MarketError, MarketKey, MarketQuote, MarketSnapshot, MarketTrade, MarketType,
    MarketUpdate, OrderBookLevel, OrderBookSnapshot,
};
