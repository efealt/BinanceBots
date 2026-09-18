use crate::{
    market::{BinanceMarketClient, MarketError, MarketKey, MarketType, parse_market_event},
    storage::{CaptureSpec, CaptureStatus, CaptureWriter, CapturedEvent, StorageError},
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::task::JoinSet;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DEFAULT_SYMBOL: &str = "BTCUSDT";
const DEFAULT_INTERVAL: &str = "1m";
const DEFAULT_DURATION_SECONDS: u64 = 60;
const DEFAULT_DATABASE: &str = "data/binance_grid.sqlite3";

pub struct CaptureOptions {
    pub symbol: String,
    pub interval: String,
    pub market_type: MarketType,
    pub duration: Duration,
    pub database_path: PathBuf,
}

impl CaptureOptions {
    pub fn from_args(args: &[String]) -> Result<Self, CollectorError> {
        let mut symbol = DEFAULT_SYMBOL.to_string();
        let mut interval = DEFAULT_INTERVAL.to_string();
        let mut market_type = MarketType::Spot;
        let mut duration_seconds = DEFAULT_DURATION_SECONDS;
        let mut database_path = PathBuf::from(DEFAULT_DATABASE);
        let mut index = 0;

        while index < args.len() {
            let flag = args[index].as_str();
            let value = args.get(index + 1).ok_or_else(|| {
                CollectorError::InvalidArguments(format!("missing value for {flag}"))
            })?;

            match flag {
                "--symbol" => symbol = value.to_uppercase(),
                "--interval" => interval = value.to_lowercase(),
                "--market-type" => {
                    market_type = MarketType::parse(value)?;
                }
                "--duration-seconds" => {
                    duration_seconds = value.parse().map_err(|_| {
                        CollectorError::InvalidArguments(
                            "--duration-seconds must be a positive integer".into(),
                        )
                    })?;
                    if duration_seconds == 0 {
                        return Err(CollectorError::InvalidArguments(
                            "--duration-seconds must be positive".into(),
                        ));
                    }
                }
                "--database" => database_path = PathBuf::from(value),
                "--help" => return Err(CollectorError::Usage(usage().into())),
                _ => {
                    return Err(CollectorError::InvalidArguments(format!(
                        "unknown capture option: {flag}\n\n{}",
                        usage()
                    )));
                }
            }
            index += 2;
        }

        Ok(Self {
            symbol,
            interval,
            market_type,
            duration: Duration::from_secs(duration_seconds),
            database_path,
        })
    }
}

pub async fn run(options: CaptureOptions) -> Result<(), CollectorError> {
    let key = MarketKey::new(&options.symbol, &options.interval, options.market_type)?;
    let client = BinanceMarketClient::new();
    let writer = CaptureWriter::start(CaptureSpec {
        key: key.clone(),
        database_path: options.database_path.clone(),
        started_at_ms: now_ms(),
    })?;
    let capture_id = writer.capture_id();
    let writer_handle = writer.handle();
    let connected_streams = Arc::new(AtomicUsize::new(0));
    let mut readers = JoinSet::new();

    for url in client.stream_urls(&key) {
        let writer_handle = writer_handle.clone();
        let connected_streams = connected_streams.clone();
        readers.spawn(async move {
            let Ok((socket, _)) = connect_async(&url).await else {
                return;
            };
            connected_streams.fetch_add(1, Ordering::Relaxed);
            let (_, mut reader) = socket.split();

            while let Some(message) = reader.next().await {
                let Message::Text(payload) = (match message {
                    Ok(message) => message,
                    Err(_) => break,
                }) else {
                    continue;
                };
                let payload = payload.to_string();
                let Some(stream_name) = stream_name(&payload) else {
                    continue;
                };
                let Ok(event) = parse_market_event(&payload) else {
                    continue;
                };

                let captured = CapturedEvent::new(stream_name, now_ms(), payload, event);
                if writer_handle.record(captured).is_err() {
                    break;
                }
            }
        });
    }

    tokio::time::sleep(options.duration).await;
    readers.abort_all();
    while readers.join_next().await.is_some() {}

    let connected = connected_streams.load(Ordering::Relaxed);
    let status = if connected > 0 {
        CaptureStatus::Completed
    } else {
        CaptureStatus::Failed
    };
    let reason = if connected > 0 {
        "duration_elapsed"
    } else {
        "no_stream_connected"
    };
    writer.finish(now_ms(), status, reason)?;

    if connected == 0 {
        return Err(CollectorError::NoStreamsConnected);
    }

    println!(
        "Live capture complete: capture_id={capture_id}, database={}",
        options.database_path.display()
    );
    Ok(())
}

#[derive(Deserialize)]
struct StreamEnvelope {
    stream: String,
}

fn stream_name(payload: &str) -> Option<String> {
    serde_json::from_str::<StreamEnvelope>(payload)
        .ok()
        .map(|envelope| envelope.stream)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn usage() -> &'static str {
    "Usage: cargo run -- --capture-live [options]\n\n\
     --symbol SYMBOL                 default: BTCUSDT\n\
     --interval 1m|5m|1h             default: 1m\n\
     --market-type spot|usd_m_perpetual  default: spot\n\
     --duration-seconds N             default: 60\n\
     --database PATH                  default: data/binance_grid.sqlite3"
}

#[derive(Debug, Error)]
pub enum CollectorError {
    #[error("invalid capture option: {0}")]
    InvalidArguments(String),
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Market(#[from] MarketError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("no Binance WebSocket stream connected")]
    NoStreamsConnected,
}
