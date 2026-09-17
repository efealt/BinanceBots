use super::types::{MarketError, MarketTrade};
use serde::Deserialize;

#[derive(Deserialize)]
struct RawTrade {
    t: Option<u64>,
    a: Option<u64>,
    p: String,
    q: String,
    #[serde(rename = "T")]
    trade_time: i64,
    m: bool,
}

pub fn parse_trade(data: serde_json::Value) -> Result<MarketTrade, MarketError> {
    let raw: RawTrade = serde_json::from_value(data).map_err(|_| MarketError::InvalidPayload)?;

    Ok(MarketTrade {
        trade_id: raw.t.or(raw.a).ok_or(MarketError::InvalidPayload)?,
        price: parse_number(&raw.p)?,
        quantity: parse_number(&raw.q)?,
        trade_time: raw.trade_time,
        is_buyer_maker: raw.m,
    })
}

fn parse_number(value: &str) -> Result<f64, MarketError> {
    value.parse().map_err(|_| MarketError::InvalidPayload)
}
