mod binance;
mod order_book;
mod service;
mod stream;
mod trades;
mod types;

pub use service::MarketService;
pub use types::{MarketError, MarketKey, MarketSnapshot, MarketType, MarketUpdate};
