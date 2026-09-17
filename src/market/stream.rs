use super::{
    binance::parse_stream_candle_value,
    order_book::{parse_book_ticker, parse_depth},
    trades::parse_trade,
    types::{Candle, MarketError, MarketQuote, MarketTrade, OrderBookSnapshot},
};
use serde::Deserialize;

pub enum MarketEvent {
    Candle(Candle),
    Quote(MarketQuote),
    OrderBook(OrderBookSnapshot),
    Trade(MarketTrade),
}

#[derive(Deserialize)]
struct CombinedStreamEvent {
    stream: String,
    data: serde_json::Value,
}

pub fn parse_market_event(payload: &str) -> Result<MarketEvent, MarketError> {
    let event: CombinedStreamEvent =
        serde_json::from_str(payload).map_err(|_| MarketError::InvalidPayload)?;
    let stream = event.stream.to_ascii_lowercase();

    if stream.contains("@kline_") {
        return Ok(MarketEvent::Candle(parse_stream_candle_value(event.data)?));
    }
    if stream.ends_with("@bookticker") {
        return Ok(MarketEvent::Quote(parse_book_ticker(event.data)?));
    }
    if stream.contains("@depth") {
        return Ok(MarketEvent::OrderBook(parse_depth(event.data)?));
    }
    if stream.ends_with("@trade") || stream.ends_with("@aggtrade") {
        return Ok(MarketEvent::Trade(parse_trade(event.data)?));
    }

    Err(MarketError::InvalidPayload)
}
