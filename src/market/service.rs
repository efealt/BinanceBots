use super::{
    MarketError, MarketKey, MarketSnapshot,
    binance::{BinanceMarketClient, parse_stream_candle, trim_to_capacity},
    types::{Candle, FeedStatus},
};
use futures_util::StreamExt;
use std::{collections::HashMap, collections::VecDeque, sync::Arc};
use tokio::{sync::RwLock, time::Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub struct MarketService {
    client: Arc<BinanceMarketClient>,
    feeds: RwLock<HashMap<MarketKey, Arc<MarketFeed>>>,
}

impl MarketService {
    pub fn new() -> Self {
        Self {
            client: Arc::new(BinanceMarketClient::new()),
            feeds: RwLock::new(HashMap::new()),
        }
    }

    pub async fn snapshot_for(&self, key: MarketKey) -> Result<MarketSnapshot, MarketError> {
        let feed = self.feed_for(key).await?;
        Ok(feed.snapshot().await)
    }

    async fn feed_for(&self, key: MarketKey) -> Result<Arc<MarketFeed>, MarketError> {
        if let Some(feed) = self.feeds.read().await.get(&key).cloned() {
            return Ok(feed);
        }

        let candles = self.client.latest_candles(&key).await?;
        let feed = Arc::new(MarketFeed::new(key.clone(), candles));
        let mut feeds = self.feeds.write().await;

        if let Some(existing) = feeds.get(&key).cloned() {
            return Ok(existing);
        }

        feeds.insert(key, feed.clone());
        feed.start(self.client.clone());
        Ok(feed)
    }
}

struct MarketFeed {
    key: MarketKey,
    state: Arc<RwLock<MarketFeedState>>,
}

struct MarketFeedState {
    status: FeedStatus,
    candles: VecDeque<Candle>,
}

impl MarketFeed {
    fn new(key: MarketKey, candles: Vec<Candle>) -> Self {
        Self {
            key,
            state: Arc::new(RwLock::new(MarketFeedState {
                status: FeedStatus::Loading,
                candles: candles.into(),
            })),
        }
    }

    fn start(self: &Arc<Self>, client: Arc<BinanceMarketClient>) {
        let feed = self.clone();
        tokio::spawn(async move {
            feed.run(client).await;
        });
    }

    async fn snapshot(&self) -> MarketSnapshot {
        let state = self.state.read().await;
        MarketSnapshot {
            symbol: self.key.symbol.clone(),
            interval: self.key.interval.clone(),
            status: state.status,
            candles: state.candles.iter().cloned().collect(),
        }
    }

    async fn run(self: Arc<Self>, client: Arc<BinanceMarketClient>) {
        loop {
            self.set_status(FeedStatus::Loading).await;

            match connect_async(client.stream_url(&self.key)).await {
                Ok((stream, _)) => {
                    self.set_status(FeedStatus::Live).await;
                    let (_, mut reader) = stream.split();

                    while let Some(message) = reader.next().await {
                        match message {
                            Ok(Message::Text(payload)) => {
                                if let Ok(candle) = parse_stream_candle(&payload.to_string()) {
                                    self.upsert_candle(candle).await;
                                }
                            }
                            Ok(Message::Close(_)) | Err(_) => break,
                            _ => {}
                        }
                    }
                }
                Err(_) => {}
            }

            self.set_status(FeedStatus::Reconnecting).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn set_status(&self, status: FeedStatus) {
        self.state.write().await.status = status;
    }

    async fn upsert_candle(&self, candle: Candle) {
        let mut state = self.state.write().await;
        if let Some(last) = state.candles.back_mut() {
            if last.open_time == candle.open_time {
                *last = candle;
                return;
            }
        }

        state.candles.push_back(candle);
        trim_to_capacity(&mut state.candles);
    }
}
