use super::{
    MarketError, MarketKey, MarketSnapshot,
    binance::{BinanceMarketClient, trim_to_capacity},
    stream::{MarketEvent, parse_market_event},
    types::{Candle, FeedStatus, MAX_RECENT_TRADES, MarketQuote, MarketTrade, OrderBookSnapshot},
};
use futures_util::StreamExt;
use std::{collections::HashMap, collections::VecDeque, sync::Arc};
use tokio::{sync::RwLock, task::JoinSet, time::Duration};
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
    quote: MarketQuote,
    order_book: OrderBookSnapshot,
    trades: VecDeque<MarketTrade>,
}

impl MarketFeed {
    fn new(key: MarketKey, candles: Vec<Candle>) -> Self {
        Self {
            key,
            state: Arc::new(RwLock::new(MarketFeedState {
                status: FeedStatus::Loading,
                candles: candles.into(),
                quote: MarketQuote::default(),
                order_book: OrderBookSnapshot::default(),
                trades: VecDeque::new(),
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
            quote: state.quote.clone(),
            order_book: state.order_book.clone(),
            trades: state.trades.iter().cloned().collect(),
        }
    }

    async fn run(self: Arc<Self>, client: Arc<BinanceMarketClient>) {
        loop {
            self.set_status(FeedStatus::Loading).await;

            let mut streams = Vec::new();
            let mut connected = true;
            for url in client.stream_urls(&self.key) {
                match connect_async(url).await {
                    Ok((stream, _)) => streams.push(stream),
                    Err(_) => {
                        connected = false;
                        break;
                    }
                }
            }

            if connected {
                self.set_status(FeedStatus::Live).await;
                let mut readers = JoinSet::new();
                for stream in streams {
                    let feed = self.clone();
                    readers.spawn(async move {
                        let (_, mut reader) = stream.split();
                        while let Some(message) = reader.next().await {
                            match message {
                                Ok(Message::Text(payload)) => {
                                    if let Ok(event) = parse_market_event(&payload.to_string()) {
                                        feed.apply_event(event).await;
                                    }
                                }
                                Ok(Message::Close(_)) | Err(_) => break,
                                _ => {}
                            }
                        }
                    });
                }

                let _ = readers.join_next().await;
                readers.abort_all();
                while readers.join_next().await.is_some() {}
            }

            self.set_status(FeedStatus::Reconnecting).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn set_status(&self, status: FeedStatus) {
        self.state.write().await.status = status;
    }

    async fn apply_event(&self, event: MarketEvent) {
        match event {
            MarketEvent::Candle(candle) => self.upsert_candle(candle).await,
            MarketEvent::Quote(quote) => self.state.write().await.quote = quote,
            MarketEvent::OrderBook(order_book) => {
                self.state.write().await.order_book = order_book;
            }
            MarketEvent::Trade(trade) => self.add_trade(trade).await,
        }
    }

    async fn add_trade(&self, trade: MarketTrade) {
        let mut state = self.state.write().await;
        state.trades.push_front(trade);
        while state.trades.len() > MAX_RECENT_TRADES {
            state.trades.pop_back();
        }
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
