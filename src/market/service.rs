use super::{
    MarketError, MarketKey, MarketSnapshot, MarketUpdate,
    binance::{BinanceMarketClient, trim_to_capacity},
    stream::{MarketEvent, parse_market_event},
    types::{Candle, FeedStatus, MAX_RECENT_TRADES, MarketQuote, MarketTrade, OrderBookSnapshot},
};
use futures_util::StreamExt;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::{
    sync::{Mutex, RwLock, broadcast},
    task::JoinSet,
    time::Duration,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const REALTIME_EVENT_CHANNEL_CAPACITY: usize = 16_384;

#[derive(Clone)]
pub struct MarketRealtimeEvent {
    pub sequence: u64,
    pub connection_epoch: u64,
    pub received_at_ms: i64,
    pub kind: MarketRealtimeEventKind,
}

#[derive(Clone)]
pub enum MarketRealtimeEventKind {
    Status(FeedStatus),
    Candle(Candle),
    Trade(MarketTrade),
}

#[derive(Debug, Error)]
pub enum RunMarketFeedError {
    #[error("Run {run_id} realtime market subscription lagged by {missed_events} backend events")]
    Lagged { run_id: i64, missed_events: u64 },
    #[error("Run {run_id} realtime market subscription closed")]
    Closed { run_id: i64 },
    #[error("Run {run_id} realtime market sequence gap: expected {expected_sequence}, received {actual_sequence}")]
    SequenceGap { run_id: i64, expected_sequence: u64, actual_sequence: u64 },
    #[error("Run {run_id} realtime market connection epoch changed from {previous_epoch} to {current_epoch}")]
    ConnectionEpochChanged { run_id: i64, previous_epoch: u64, current_epoch: u64 },
    #[error("Run {run_id} realtime trade order moved backwards from trade {previous_trade_id} to {current_trade_id}")]
    TradeOutOfOrder { run_id: i64, previous_trade_id: u64, current_trade_id: u64 },
}

impl RunMarketFeedError {
    pub fn stable_reason(&self) -> String {
        match self {
            Self::Lagged { missed_events, .. } => format!(
                "execution_feed_lagged: missed {missed_events} backend realtime market events; trade chronology cannot be proven"
            ),
            Self::Closed { .. } => {
                "execution_feed_closed: backend realtime market event channel closed".into()
            }
            Self::SequenceGap { expected_sequence, actual_sequence, .. } => format!(
                "execution_feed_sequence_gap: expected event sequence {expected_sequence}, received {actual_sequence}"
            ),
            Self::ConnectionEpochChanged { previous_epoch, current_epoch, .. } => format!(
                "execution_feed_reconnected: connection epoch changed from {previous_epoch} to {current_epoch}; missed trades cannot be ruled out"
            ),
            Self::TradeOutOfOrder { previous_trade_id, current_trade_id, .. } => format!(
                "execution_trade_out_of_order: previous trade id {previous_trade_id}, received {current_trade_id}"
            ),
        }
    }
}

pub struct RunMarketSubscription {
    run_id: i64,
    receiver: broadcast::Receiver<MarketRealtimeEvent>,
    last_sequence: Option<u64>,
    connection_epoch: Option<u64>,
    last_trade_id: Option<u64>,
}

impl RunMarketSubscription {
    fn new(run_id: i64, receiver: broadcast::Receiver<MarketRealtimeEvent>) -> Self {
        Self {
            run_id,
            receiver,
            last_sequence: None,
            connection_epoch: None,
            last_trade_id: None,
        }
    }

    pub async fn recv(&mut self) -> Result<MarketRealtimeEvent, RunMarketFeedError> {
        loop {
            let event = match self.receiver.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(missed_events)) => {
                    return Err(RunMarketFeedError::Lagged { run_id: self.run_id, missed_events });
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(RunMarketFeedError::Closed { run_id: self.run_id });
                }
            };

            if let Some(last_sequence) = self.last_sequence {
                let expected_sequence = last_sequence.saturating_add(1);
                if event.sequence != expected_sequence {
                    return Err(RunMarketFeedError::SequenceGap {
                        run_id: self.run_id,
                        expected_sequence,
                        actual_sequence: event.sequence,
                    });
                }
            }
            self.last_sequence = Some(event.sequence);

            if event.connection_epoch > 0 {
                if let Some(previous_epoch) = self.connection_epoch
                    && previous_epoch != event.connection_epoch
                {
                    return Err(RunMarketFeedError::ConnectionEpochChanged {
                        run_id: self.run_id,
                        previous_epoch,
                        current_epoch: event.connection_epoch,
                    });
                }
                self.connection_epoch = Some(event.connection_epoch);
            }

            if let MarketRealtimeEventKind::Trade(trade) = &event.kind {
                if let Some(previous_trade_id) = self.last_trade_id {
                    if trade.trade_id == previous_trade_id {
                        continue;
                    }
                    if trade.trade_id < previous_trade_id {
                        return Err(RunMarketFeedError::TradeOutOfOrder {
                            run_id: self.run_id,
                            previous_trade_id,
                            current_trade_id: trade.trade_id,
                        });
                    }
                }
                self.last_trade_id = Some(trade.trade_id);
            }

            return Ok(event);
        }
    }

    pub fn run_id(&self) -> i64 {
        self.run_id
    }

    pub fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    pub fn last_trade_id(&self) -> Option<u64> {
        self.last_trade_id
    }
}

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

    pub async fn update_for(&self, key: &MarketKey) -> Result<MarketUpdate, MarketError> {
        let feed = self.feed_for(key.clone()).await?;
        Ok(feed.update().await)
    }

    pub async fn subscribe_for_run(
        &self,
        key: MarketKey,
        run_id: i64,
    ) -> Result<RunMarketSubscription, MarketError> {
        let feed = self.feed_for(key).await?;
        Ok(feed.subscribe_for_run(run_id))
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
    realtime_tx: broadcast::Sender<MarketRealtimeEvent>,
    delivery: Mutex<MarketDeliveryState>,
}

struct MarketFeedState {
    status: FeedStatus,
    candles: VecDeque<Candle>,
    quote: MarketQuote,
    order_book: OrderBookSnapshot,
    trades: VecDeque<MarketTrade>,
}

struct MarketDeliveryState {
    sequence: u64,
    connection_epoch: u64,
}

impl MarketFeed {
    fn new(key: MarketKey, candles: Vec<Candle>) -> Self {
        Self::new_with_event_capacity(key, candles, REALTIME_EVENT_CHANNEL_CAPACITY)
    }

    fn new_with_event_capacity(key: MarketKey, candles: Vec<Candle>, event_capacity: usize) -> Self {
        let (realtime_tx, _) = broadcast::channel(event_capacity.max(1));
        Self {
            key,
            state: Arc::new(RwLock::new(MarketFeedState {
                status: FeedStatus::Loading,
                candles: candles.into(),
                quote: MarketQuote::default(),
                order_book: OrderBookSnapshot::default(),
                trades: VecDeque::new(),
            })),
            realtime_tx,
            delivery: Mutex::new(MarketDeliveryState {
                sequence: 0,
                connection_epoch: 0,
            }),
        }
    }

    fn subscribe_for_run(&self, run_id: i64) -> RunMarketSubscription {
        RunMarketSubscription::new(run_id, self.realtime_tx.subscribe())
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

    async fn update(&self) -> MarketUpdate {
        let state = self.state.read().await;
        MarketUpdate {
            status: state.status,
            candle: state.candles.back().cloned(),
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
                self.begin_connection_epoch().await;
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
                                        feed.apply_event(event, now_ms()).await;
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

    async fn begin_connection_epoch(&self) {
        let mut delivery = self.delivery.lock().await;
        delivery.connection_epoch = delivery.connection_epoch.saturating_add(1);
    }

    async fn set_status(&self, status: FeedStatus) {
        self.state.write().await.status = status;
        self.publish_realtime(MarketRealtimeEventKind::Status(status), now_ms()).await;
    }

    async fn apply_event(&self, event: MarketEvent, received_at_ms: i64) {
        match event {
            MarketEvent::Candle(candle) => {
                self.upsert_candle(candle.clone()).await;
                self.publish_realtime(MarketRealtimeEventKind::Candle(candle), received_at_ms).await;
            }
            MarketEvent::Quote(quote) => self.state.write().await.quote = quote,
            MarketEvent::OrderBook(order_book) => {
                self.state.write().await.order_book = order_book;
            }
            MarketEvent::Trade(trade) => {
                self.add_trade(trade.clone()).await;
                self.publish_realtime(MarketRealtimeEventKind::Trade(trade), received_at_ms).await;
            }
        }
    }

    async fn publish_realtime(&self, kind: MarketRealtimeEventKind, received_at_ms: i64) {
        let mut delivery = self.delivery.lock().await;
        delivery.sequence = delivery.sequence.saturating_add(1);
        let event = MarketRealtimeEvent {
            sequence: delivery.sequence,
            connection_epoch: delivery.connection_epoch,
            received_at_ms,
            kind,
        };
        let _ = self.realtime_tx.send(event);
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::{MarketType, stream::MarketEvent};

    fn key() -> MarketKey {
        MarketKey::new("BTCUSDT", "1m", MarketType::Spot).unwrap()
    }

    fn trade(trade_id: u64, trade_time: i64, price: f64) -> MarketTrade {
        MarketTrade {
            trade_id,
            price,
            quantity: 2.5,
            trade_time,
            is_buyer_maker: false,
        }
    }

    fn candle(open_time: i64) -> Candle {
        Candle {
            open_time,
            close_time: open_time + 59_999,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            volume: 10.0,
            is_closed: false,
        }
    }

    #[tokio::test]
    async fn run_subscription_receives_trade_and_candle_without_polling() {
        let feed = MarketFeed::new_with_event_capacity(key(), Vec::new(), 16);
        let mut subscription = feed.subscribe_for_run(11);

        feed.begin_connection_epoch().await;
        feed.set_status(FeedStatus::Live).await;
        feed.apply_event(MarketEvent::Trade(trade(41, 1_000, 99.5)), 1_001).await;
        feed.apply_event(MarketEvent::Candle(candle(0)), 1_002).await;

        let status = subscription.recv().await.unwrap();
        assert!(matches!(status.kind, MarketRealtimeEventKind::Status(FeedStatus::Live)));

        let trade_event = subscription.recv().await.unwrap();
        match trade_event.kind {
            MarketRealtimeEventKind::Trade(value) => {
                assert_eq!(value.trade_id, 41);
                assert_eq!(value.trade_time, 1_000);
                assert_eq!(value.price, 99.5);
                assert_eq!(value.quantity, 2.5);
                assert_eq!(trade_event.received_at_ms, 1_001);
            }
            _ => panic!("expected trade event"),
        }

        let candle_event = subscription.recv().await.unwrap();
        assert!(matches!(candle_event.kind, MarketRealtimeEventKind::Candle(_)));
        assert_eq!(subscription.last_trade_id(), Some(41));
    }

    #[tokio::test]
    async fn run_subscription_suppresses_duplicate_trade_identity() {
        let feed = MarketFeed::new_with_event_capacity(key(), Vec::new(), 16);
        let mut subscription = feed.subscribe_for_run(12);

        feed.begin_connection_epoch().await;
        feed.set_status(FeedStatus::Live).await;
        let _ = subscription.recv().await.unwrap();

        feed.apply_event(MarketEvent::Trade(trade(50, 2_000, 100.0)), 2_001).await;
        feed.apply_event(MarketEvent::Trade(trade(50, 2_000, 100.0)), 2_002).await;
        feed.apply_event(MarketEvent::Trade(trade(51, 2_010, 100.1)), 2_011).await;

        let first = subscription.recv().await.unwrap();
        let second = subscription.recv().await.unwrap();
        assert!(matches!(first.kind, MarketRealtimeEventKind::Trade(ref value) if value.trade_id == 50));
        assert!(matches!(second.kind, MarketRealtimeEventKind::Trade(ref value) if value.trade_id == 51));
        assert_eq!(subscription.last_trade_id(), Some(51));
        assert_eq!(subscription.last_sequence(), Some(second.sequence));
    }

    #[tokio::test]
    async fn run_subscription_reports_connection_epoch_change() {
        let feed = MarketFeed::new_with_event_capacity(key(), Vec::new(), 16);
        let mut subscription = feed.subscribe_for_run(13);

        feed.begin_connection_epoch().await;
        feed.set_status(FeedStatus::Live).await;
        let first = subscription.recv().await.unwrap();
        assert_eq!(first.connection_epoch, 1);

        feed.set_status(FeedStatus::Reconnecting).await;
        let reconnecting = subscription.recv().await.unwrap();
        assert!(matches!(
            reconnecting.kind,
            MarketRealtimeEventKind::Status(FeedStatus::Reconnecting)
        ));

        feed.begin_connection_epoch().await;
        feed.set_status(FeedStatus::Live).await;
        let error = subscription.recv().await.unwrap_err();
        assert!(matches!(
            error,
            RunMarketFeedError::ConnectionEpochChanged {
                previous_epoch: 1,
                current_epoch: 2,
                ..
            }
        ));
        assert!(error.stable_reason().contains("execution_feed_reconnected"));
    }

    #[tokio::test]
    async fn run_subscription_reports_broadcast_lag_instead_of_silent_loss() {
        let feed = MarketFeed::new_with_event_capacity(key(), Vec::new(), 2);
        let mut subscription = feed.subscribe_for_run(14);

        for _ in 0..8 {
            feed.set_status(FeedStatus::Loading).await;
        }

        let error = subscription.recv().await.unwrap_err();
        assert!(matches!(
            error,
            RunMarketFeedError::Lagged { missed_events, .. } if missed_events > 0
        ));
        assert!(error.stable_reason().contains("execution_feed_lagged"));
    }
}
