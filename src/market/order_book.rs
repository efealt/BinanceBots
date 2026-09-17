use super::types::{MarketError, MarketQuote, OrderBookLevel, OrderBookSnapshot};
use serde::Deserialize;

#[derive(Deserialize)]
struct RawBookTicker {
    #[serde(rename = "u")]
    update_id: u64,
    b: String,
    #[serde(rename = "B")]
    bid_quantity: String,
    a: String,
    #[serde(rename = "A")]
    ask_quantity: String,
}

#[derive(Deserialize)]
struct RawDepth {
    #[serde(rename = "lastUpdateId")]
    spot_update_id: Option<u64>,
    #[serde(rename = "u")]
    futures_update_id: Option<u64>,
    #[serde(alias = "b")]
    bids: Vec<[String; 2]>,
    #[serde(alias = "a")]
    asks: Vec<[String; 2]>,
}

pub fn parse_book_ticker(data: serde_json::Value) -> Result<MarketQuote, MarketError> {
    let raw: RawBookTicker =
        serde_json::from_value(data).map_err(|_| MarketError::InvalidPayload)?;
    let best_bid = parse_number(&raw.b)?;
    let best_bid_quantity = parse_number(&raw.bid_quantity)?;
    let best_ask = parse_number(&raw.a)?;
    let best_ask_quantity = parse_number(&raw.ask_quantity)?;

    Ok(MarketQuote {
        update_id: Some(raw.update_id),
        best_bid: Some(best_bid),
        best_bid_quantity: Some(best_bid_quantity),
        best_ask: Some(best_ask),
        best_ask_quantity: Some(best_ask_quantity),
        spread: Some(best_ask - best_bid),
        mid_price: Some((best_ask + best_bid) / 2.0),
    })
}

pub fn parse_depth(data: serde_json::Value) -> Result<OrderBookSnapshot, MarketError> {
    let raw: RawDepth = serde_json::from_value(data).map_err(|_| MarketError::InvalidPayload)?;

    Ok(OrderBookSnapshot {
        update_id: raw.spot_update_id.or(raw.futures_update_id),
        bids: parse_levels(raw.bids)?,
        asks: parse_levels(raw.asks)?,
    })
}

fn parse_levels(levels: Vec<[String; 2]>) -> Result<Vec<OrderBookLevel>, MarketError> {
    levels
        .into_iter()
        .map(|[price, quantity]| {
            Ok(OrderBookLevel {
                price: parse_number(&price)?,
                quantity: parse_number(&quantity)?,
            })
        })
        .collect()
}

fn parse_number(value: &str) -> Result<f64, MarketError> {
    value.parse().map_err(|_| MarketError::InvalidPayload)
}
