use super::types::{Candle, MAX_CANDLES, MarketError, MarketKey, MarketType};
use serde::Deserialize;

const SPOT_REST_BASE_URL: &str = "https://api.binance.com";
const SPOT_STREAM_BASE_URL: &str = "wss://stream.binance.com:9443";
const USD_M_REST_BASE_URL: &str = "https://fapi.binance.com";
const USD_M_PUBLIC_STREAM_BASE_URL: &str = "wss://fstream.binance.com/public/stream";
const USD_M_MARKET_STREAM_BASE_URL: &str = "wss://fstream.binance.com/market/stream";

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
        let (base_url, path) = match key.market_type {
            MarketType::Spot => (SPOT_REST_BASE_URL, "/api/v3/klines"),
            MarketType::UsdMarginedPerpetual => (USD_M_REST_BASE_URL, "/fapi/v1/klines"),
        };
        let url = format!(
            "{base_url}{path}?symbol={}&interval={}&limit=1000",
            key.symbol, key.interval,
        );
        let response = self.http.get(url).send().await?.error_for_status()?;

        response
            .json::<Vec<RawKline>>()
            .await?
            .into_iter()
            .map(Candle::try_from)
            .collect()
    }

    pub fn stream_urls(&self, key: &MarketKey) -> Vec<String> {
        let symbol = key.symbol.to_lowercase();
        let kline_stream = key.stream_name();

        match key.market_type {
            MarketType::Spot => vec![format!(
                "{SPOT_STREAM_BASE_URL}/stream?streams={kline_stream}/{symbol}@bookTicker/{symbol}@depth20@100ms/{symbol}@trade",
            )],
            MarketType::UsdMarginedPerpetual => vec![
                format!("{USD_M_MARKET_STREAM_BASE_URL}?streams={kline_stream}/{symbol}@aggTrade",),
                format!(
                    "{USD_M_PUBLIC_STREAM_BASE_URL}?streams={symbol}@bookTicker/{symbol}@depth20",
                ),
            ],
        }
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

pub fn parse_stream_candle_value(value: serde_json::Value) -> Result<Candle, MarketError> {
    let event: KlineEvent =
        serde_json::from_value(value).map_err(|_| MarketError::InvalidPayload)?;
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
