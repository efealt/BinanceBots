mod reader;
mod schema;

use crate::market::{Candle, MarketEvent, MarketKey, MarketQuote, MarketTrade, OrderBookSnapshot};
use rusqlite::{Connection, Transaction, params};
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, SyncSender, channel, sync_channel},
    thread::JoinHandle,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

const WRITER_QUEUE_CAPACITY: usize = 4_096;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("storage directory error: {0}")]
    Io(#[from] std::io::Error),
    #[error("storage value is too large for SQLite")]
    ValueTooLarge,
    #[error("capture writer is unavailable")]
    WriterUnavailable,
    #[error("capture writer could not start: {0}")]
    WriterStart(String),
    #[error("capture writer thread panicked")]
    WriterPanicked,
    #[error("capture writer did not return a result")]
    WriterResponseUnavailable,
    #[error("historical dataset {0} was not found")]
    DatasetNotFound(i64),
    #[error("WebSocket capture {0} was not found")]
    CaptureNotFound(i64),
    #[error("data entry already exists for {symbol} {market_type} {interval}")]
    DataDownloadAlreadyExists {
        symbol: String,
        market_type: String,
        interval: String,
    },
    #[error("data entry {0} was not found")]
    DataDownloadNotFound(i64),
    #[error("data entry {0} is currently downloading")]
    DataDownloadAlreadyRunning(i64),
    #[error("a UTC start date is required before this data entry can download")]
    DataDownloadStartDateRequired,
    #[error("the saved start date cannot be changed after a download entry is created")]
    DataDownloadStartDateImmutable,
}

pub use reader::{
    CaptureInspection, CaptureSummary, DataDownload, DataDownloadSpec, DatasetInspection,
    DatasetSummary, DownloadRunPreparation, HistoricalKline, InspectionCandle, StorageReader,
};

#[derive(Clone, Copy)]
pub enum CaptureStatus {
    Completed,
    Failed,
}

impl CaptureStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

pub struct CaptureSpec {
    pub key: MarketKey,
    pub database_path: PathBuf,
    pub started_at_ms: i64,
}

pub struct CapturedEvent {
    stream_name: String,
    received_at_ms: i64,
    raw_payload: String,
    event: MarketEvent,
}

impl CapturedEvent {
    pub fn new(
        stream_name: String,
        received_at_ms: i64,
        raw_payload: String,
        event: MarketEvent,
    ) -> Self {
        Self {
            stream_name,
            received_at_ms,
            raw_payload,
            event,
        }
    }
}

#[derive(Clone)]
pub struct CaptureWriterHandle {
    sender: SyncSender<WriteCommand>,
}

impl CaptureWriterHandle {
    pub fn record(&self, event: CapturedEvent) -> Result<(), StorageError> {
        self.sender
            .send(WriteCommand::Event(event))
            .map_err(|_| StorageError::WriterUnavailable)
    }
}

pub struct CaptureWriter {
    capture_id: i64,
    sender: SyncSender<WriteCommand>,
    thread: Option<JoinHandle<Result<(), StorageError>>>,
}

impl CaptureWriter {
    pub fn start(spec: CaptureSpec) -> Result<Self, StorageError> {
        let store = SqliteStore::open_for_capture(&spec)?;
        let capture_id = store.capture_id;
        let (sender, receiver) = sync_channel(WRITER_QUEUE_CAPACITY);
        let thread = std::thread::Builder::new()
            .name("binance-grid-sqlite-writer".into())
            .spawn(move || writer_loop(store, receiver))
            .map_err(|error| StorageError::WriterStart(error.to_string()))?;

        Ok(Self {
            capture_id,
            sender,
            thread: Some(thread),
        })
    }

    pub fn capture_id(&self) -> i64 {
        self.capture_id
    }

    pub fn handle(&self) -> CaptureWriterHandle {
        CaptureWriterHandle {
            sender: self.sender.clone(),
        }
    }

    pub fn finish(
        mut self,
        ended_at_ms: i64,
        status: CaptureStatus,
        reason: &str,
    ) -> Result<(), StorageError> {
        let (response_sender, response_receiver) = channel();
        let command = WriteCommand::Finish {
            ended_at_ms,
            status,
            reason: reason.to_string(),
            response: response_sender,
        };

        if self.sender.send(command).is_err() {
            let thread_result = self.join_thread();
            return match thread_result {
                Err(error) => Err(error),
                Ok(()) => Err(StorageError::WriterUnavailable),
            };
        }

        let response = response_receiver.recv().ok();
        let thread_result = self.join_thread();

        match response {
            Some(result) => {
                result?;
                thread_result
            }
            None => {
                thread_result?;
                Err(StorageError::WriterResponseUnavailable)
            }
        }
    }

    fn join_thread(&mut self) -> Result<(), StorageError> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };

        thread.join().map_err(|_| StorageError::WriterPanicked)?
    }
}

enum WriteCommand {
    Event(CapturedEvent),
    Finish {
        ended_at_ms: i64,
        status: CaptureStatus,
        reason: String,
        response: std::sync::mpsc::Sender<Result<(), StorageError>>,
    },
}

struct SqliteStore {
    connection: Connection,
    capture_id: i64,
}

impl SqliteStore {
    fn open_for_capture(spec: &CaptureSpec) -> Result<Self, StorageError> {
        if let Some(parent) = spec.database_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(&spec.database_path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )?;
        schema::migrate(&connection)?;

        let instrument_id = ensure_instrument(&connection, &spec.key)?;
        connection.execute(
            "INSERT INTO live_capture_sessions
                (instrument_id, interval, started_at_ms, status)
             VALUES (?1, ?2, ?3, 'running')",
            params![instrument_id, spec.key.interval, spec.started_at_ms],
        )?;

        Ok(Self {
            capture_id: connection.last_insert_rowid(),
            connection,
        })
    }

    fn insert_event(&mut self, event: &CapturedEvent) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        let exchange_event_time_ms = event.exchange_event_time_ms();
        transaction.execute(
            "INSERT INTO live_events
                (capture_id, event_type, stream_name, exchange_event_time_ms,
                 received_at_ms, raw_payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                self.capture_id,
                event.event_type(),
                event.stream_name,
                exchange_event_time_ms,
                event.received_at_ms,
                event.raw_payload,
            ],
        )?;
        let event_id = transaction.last_insert_rowid();

        match &event.event {
            MarketEvent::Candle(candle) => {
                insert_kline(&transaction, self.capture_id, event_id, event, candle)?
            }
            MarketEvent::Quote(quote) => insert_book_ticker(
                &transaction,
                self.capture_id,
                event_id,
                event.received_at_ms,
                quote,
            )?,
            MarketEvent::OrderBook(order_book) => insert_depth(
                &transaction,
                self.capture_id,
                event_id,
                event.received_at_ms,
                order_book,
            )?,
            MarketEvent::Trade(trade) => {
                insert_trade(&transaction, self.capture_id, event_id, event, trade)?
            }
        }

        transaction.commit()?;
        Ok(())
    }

    fn finish_capture(
        &mut self,
        ended_at_ms: i64,
        status: CaptureStatus,
        reason: &str,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "UPDATE live_capture_sessions
             SET ended_at_ms = ?1, status = ?2, stop_reason = ?3
             WHERE capture_id = ?4",
            params![ended_at_ms, status.as_str(), reason, self.capture_id],
        )?;
        Ok(())
    }
}

fn writer_loop(
    mut store: SqliteStore,
    receiver: Receiver<WriteCommand>,
) -> Result<(), StorageError> {
    loop {
        match receiver.recv() {
            Ok(WriteCommand::Event(event)) => {
                if let Err(error) = store.insert_event(&event) {
                    let _ = store.finish_capture(
                        now_ms(),
                        CaptureStatus::Failed,
                        "sqlite_write_failed",
                    );
                    return Err(error);
                }
            }
            Ok(WriteCommand::Finish {
                ended_at_ms,
                status,
                reason,
                response,
            }) => {
                let result = store.finish_capture(ended_at_ms, status, &reason);
                let _ = response.send(result);
                return Ok(());
            }
            Err(_) => {
                let _ =
                    store.finish_capture(now_ms(), CaptureStatus::Failed, "writer_disconnected");
                return Err(StorageError::WriterUnavailable);
            }
        }
    }
}

fn ensure_instrument(connection: &Connection, key: &MarketKey) -> Result<i64, StorageError> {
    connection.execute(
        "INSERT INTO market_instruments (venue, market_type, symbol, created_at_ms)
         VALUES ('binance', ?1, ?2, ?3)
         ON CONFLICT (venue, market_type, symbol) DO NOTHING",
        params![key.market_type.as_str(), key.symbol, now_ms()],
    )?;

    Ok(connection.query_row(
        "SELECT instrument_id
         FROM market_instruments
         WHERE venue = 'binance' AND market_type = ?1 AND symbol = ?2",
        params![key.market_type.as_str(), key.symbol],
        |row| row.get(0),
    )?)
}

fn insert_kline(
    transaction: &Transaction<'_>,
    capture_id: i64,
    event_id: i64,
    event: &CapturedEvent,
    candle: &Candle,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO live_kline_updates
            (event_id, capture_id, open_time_ms, close_time_ms,
             open_price, high_price, low_price, close_price, base_volume,
             is_closed, received_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            event_id,
            capture_id,
            candle.open_time,
            candle.close_time,
            candle.open,
            candle.high,
            candle.low,
            candle.close,
            candle.volume,
            if candle.is_closed { 1_i64 } else { 0_i64 },
            event.received_at_ms,
        ],
    )?;
    Ok(())
}

fn insert_book_ticker(
    transaction: &Transaction<'_>,
    capture_id: i64,
    event_id: i64,
    received_at_ms: i64,
    quote: &MarketQuote,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO live_book_ticker
            (event_id, capture_id, update_id, bid_price, bid_quantity,
             ask_price, ask_quantity, received_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event_id,
            capture_id,
            quote.update_id.map(sqlite_id).transpose()?,
            quote.best_bid,
            quote.best_bid_quantity,
            quote.best_ask,
            quote.best_ask_quantity,
            received_at_ms,
        ],
    )?;
    Ok(())
}

fn insert_depth(
    transaction: &Transaction<'_>,
    capture_id: i64,
    event_id: i64,
    received_at_ms: i64,
    order_book: &OrderBookSnapshot,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO live_depth_snapshots
            (event_id, capture_id, update_id, received_at_ms)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            event_id,
            capture_id,
            order_book.update_id.map(sqlite_id).transpose()?,
            received_at_ms,
        ],
    )?;

    for (level_index, level) in order_book.bids.iter().enumerate() {
        insert_depth_level(transaction, event_id, "bid", level_index, level)?;
    }
    for (level_index, level) in order_book.asks.iter().enumerate() {
        insert_depth_level(transaction, event_id, "ask", level_index, level)?;
    }
    Ok(())
}

fn insert_depth_level(
    transaction: &Transaction<'_>,
    event_id: i64,
    side: &str,
    level_index: usize,
    level: &crate::market::OrderBookLevel,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO live_depth_levels
            (event_id, side, level_index, price, quantity)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            event_id,
            side,
            level_index as i64,
            level.price,
            level.quantity
        ],
    )?;
    Ok(())
}

fn insert_trade(
    transaction: &Transaction<'_>,
    capture_id: i64,
    event_id: i64,
    event: &CapturedEvent,
    trade: &MarketTrade,
) -> Result<(), StorageError> {
    let trade_kind = if event
        .stream_name
        .to_ascii_lowercase()
        .ends_with("@aggtrade")
    {
        "agg_trade"
    } else {
        "trade"
    };
    transaction.execute(
        "INSERT INTO live_trades
            (event_id, capture_id, trade_id, trade_kind, price, quantity,
             trade_time_ms, is_buyer_maker, received_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (capture_id, trade_id, trade_kind) DO NOTHING",
        params![
            event_id,
            capture_id,
            sqlite_id(trade.trade_id)?,
            trade_kind,
            trade.price,
            trade.quantity,
            trade.trade_time,
            if trade.is_buyer_maker { 1_i64 } else { 0_i64 },
            event.received_at_ms,
        ],
    )?;
    Ok(())
}

impl CapturedEvent {
    fn event_type(&self) -> &'static str {
        match &self.event {
            MarketEvent::Candle(_) => "kline",
            MarketEvent::Quote(_) => "book_ticker",
            MarketEvent::OrderBook(_) => "depth",
            MarketEvent::Trade(_) => "trade",
        }
    }

    fn exchange_event_time_ms(&self) -> Option<i64> {
        let payload = serde_json::from_str::<serde_json::Value>(&self.raw_payload).ok()?;
        let data = payload.get("data")?;
        data.get("E")
            .and_then(serde_json::Value::as_i64)
            .or_else(|| data.get("T").and_then(serde_json::Value::as_i64))
            .or_else(|| match &self.event {
                MarketEvent::Candle(candle) => Some(candle.open_time),
                MarketEvent::Trade(trade) => Some(trade.trade_time),
                MarketEvent::Quote(_) | MarketEvent::OrderBook(_) => None,
            })
    }
}

fn sqlite_id(value: u64) -> Result<i64, StorageError> {
    value.try_into().map_err(|_| StorageError::ValueTooLarge)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}
