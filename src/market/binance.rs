use super::types::{Candle, MAX_CANDLES, MarketError, MarketKey};
use serde::Deserialize;

const REST_BASE_URL: &str = "https://api.binance.com";
const STREAM_BASE_URL: &str = "wss://stream.binance.com:9443/ws";

pub struct BinanceMarketClient {
    http: reqwest::Client,
}

impl BinanceMarketClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    pub async fn latest_candles(&self, key: &MarketKey) -> Result<Vec<Candle>, MarketError> {
        let url = format!(
            "{REST_BASE_URL}/api/v3/klines?symbol={}&interval={}&limit=1000",
            key.symbol, key.interval
        );
        let response = self.http.get(url).send().await?.error_for_status()?;

        response
            .json::<Vec<RawKline>>()
            .await?
            .into_iter()
            .map(Candle::try_from)
            .collect()
    }

    pub fn stream_url(&self, key: &MarketKey) -> String {
        format!("{STREAM_BASE_URL}/{}", key.stream_name())
    }
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct RawKline(
    i64,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    i64,
    String,
    String,
    String,
);

impl TryFrom<RawKline> for Candle {
    type Error = MarketError;

    fn try_from(value: RawKline) -> Result<Self, Self::Error> {
        Ok(Candle {
            open_time: value.0,
            open: parse_number(&value.1)?,
            high: parse_number(&value.2)?,
            low: parse_number(&value.3)?,
            close: parse_number(&value.4)?,
            volume: parse_number(&value.5)?,
            close_time: value.6,
            is_closed: true,
        })
    }
}

#[derive(Deserialize)]
struct KlineEvent {
    k: StreamKline,
}

#[derive(Deserialize)]
struct StreamKline {
    t: i64,
    #[serde(rename = "T")]
    close_time: i64,
    o: String,
    h: String,
    l: String,
    c: String,
    v: String,
    x: bool,
}

pub fn parse_stream_candle(payload: &str) -> Result<Candle, MarketError> {
    let event: KlineEvent =
        serde_json::from_str(payload).map_err(|_| MarketError::InvalidPayload)?;
    Ok(Candle {
        open_time: event.k.t,
        close_time: event.k.close_time,
        open: parse_number(&event.k.o)?,
        high: parse_number(&event.k.h)?,
        low: parse_number(&event.k.l)?,
        close: parse_number(&event.k.c)?,
        volume: parse_number(&event.k.v)?,
        is_closed: event.k.x,
    })
}

pub fn trim_to_capacity(candles: &mut std::collections::VecDeque<Candle>) {
    while candles.len() > MAX_CANDLES {
        candles.pop_front();
    }
}

fn parse_number(value: &str) -> Result<f64, MarketError> {
    value.parse().map_err(|_| MarketError::InvalidPayload)
}
