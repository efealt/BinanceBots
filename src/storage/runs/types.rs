use super::super::StorageError;
use serde::Serialize;
use serde_json::Value;
use std::fmt;

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $value),+ }
            }

            pub(crate) fn parse(value: &str) -> Option<Self> {
                match value { $($value => Some(Self::$variant),)+ _ => None }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

string_enum!(RunMode {
    Backtest => "backtest",
    Paper => "paper",
    Live => "live",
});

string_enum!(RunStatus {
    Created => "created",
    Running => "running",
    Completed => "completed",
    Failed => "failed",
    Stopped => "stopped",
});

impl RunStatus {
    pub(crate) fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }
}

string_enum!(RunEventKind {
    RunStatus => "run_status",
    Decision => "decision",
    OrderIntent => "order_intent",
    OrderState => "order_state",
    Fill => "fill",
    Position => "position",
    Equity => "equity",
});

string_enum!(OrderSide {
    Buy => "buy",
    Sell => "sell",
});

string_enum!(OrderType {
    Market => "market",
    Limit => "limit",
    StopMarket => "stop_market",
    StopLimit => "stop_limit",
});

string_enum!(TimeInForce {
    Gtc => "gtc",
    Ioc => "ioc",
    Fok => "fok",
    Gtx => "gtx",
});

string_enum!(OrderStatus {
    Created => "created",
    Submitted => "submitted",
    Accepted => "accepted",
    PartiallyFilled => "partially_filled",
    Filled => "filled",
    Cancelled => "cancelled",
    Rejected => "rejected",
    Expired => "expired",
});

string_enum!(LiquidityRole {
    Maker => "maker",
    Taker => "taker",
});

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ExactDecimal(String);

impl ExactDecimal {
    pub fn new(value: impl AsRef<str>) -> Result<Self, StorageError> {
        let raw = value.as_ref();
        canonical_decimal(raw)
            .map(Self)
            .ok_or_else(|| StorageError::InvalidTradingDecimal(raw.to_string()))
    }

    pub fn zero() -> Self {
        Self("0".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExactDecimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn canonical_decimal(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > 128 || raw.trim() != raw || raw.starts_with('+') {
        return None;
    }
    let (negative, body) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    if body.is_empty() {
        return None;
    }
    let mut pieces = body.split('.');
    let integer = pieces.next()?;
    let fraction = pieces.next();
    if pieces.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    if let Some(fraction) = fraction
        && (fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.unwrap_or("").trim_end_matches('0');
    let is_zero = integer == "0" && fraction.is_empty();
    let sign = if negative && !is_zero { "-" } else { "" };
    if fraction.is_empty() {
        Some(format!("{sign}{integer}"))
    } else {
        Some(format!("{sign}{integer}.{fraction}"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct EventTimes {
    pub event_time_ms: i64,
    pub exchange_time_ms: Option<i64>,
    pub received_at_ms: Option<i64>,
}

impl EventTimes {
    pub fn new(event_time_ms: i64) -> Self {
        Self { event_time_ms, exchange_time_ms: None, received_at_ms: None }
    }
}

#[derive(Clone, Debug)]
pub struct TradingRunSpec {
    pub comparison_id: Option<String>,
    pub mode: RunMode,
    pub strategy_id: String,
    pub strategy_version: String,
    pub strategy_params: Value,
    pub instrument_id: i64,
    pub initial_capital: ExactDecimal,
    pub run_config: Value,
    pub data_source: Value,
    pub execution_assumptions: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct TradingRun {
    pub run_id: i64,
    pub comparison_id: Option<String>,
    pub mode: RunMode,
    pub status: RunStatus,
    pub strategy_id: String,
    pub strategy_version: String,
    pub strategy_params: Value,
    pub instrument_id: i64,
    pub initial_capital: ExactDecimal,
    pub run_config: Value,
    pub data_source: Value,
    pub execution_assumptions: Value,
    pub started_at_ms: Option<i64>,
    pub ended_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TradingRunEvent {
    pub event_id: i64,
    pub run_id: i64,
    pub run_sequence: i64,
    pub event_kind: RunEventKind,
    pub event_time_ms: i64,
    pub exchange_time_ms: Option<i64>,
    pub received_at_ms: Option<i64>,
    pub persisted_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct DecisionInput {
    pub run_id: i64,
    pub times: EventTimes,
    pub decision_type: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct DecisionRecord {
    pub event: TradingRunEvent,
    pub decision_type: String,
    pub payload: Value,
}

#[derive(Clone, Debug)]
pub struct OrderIntentInput {
    pub run_id: i64,
    pub times: EventTimes,
    pub intent_key: Option<String>,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub time_in_force: Option<TimeInForce>,
    pub price: Option<ExactDecimal>,
    pub quantity: ExactDecimal,
    pub stop_price: Option<ExactDecimal>,
    pub reduce_only: bool,
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderIntentRecord {
    pub event: TradingRunEvent,
    pub intent_key: Option<String>,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub time_in_force: Option<TimeInForce>,
    pub price: Option<ExactDecimal>,
    pub quantity: ExactDecimal,
    pub stop_price: Option<ExactDecimal>,
    pub reduce_only: bool,
    pub metadata: Value,
}

#[derive(Clone, Debug)]
pub struct CreateOrderInput {
    pub run_id: i64,
    pub times: EventTimes,
    pub intent_event_id: Option<i64>,
    pub client_order_id: Option<String>,
    pub exchange_order_id: Option<String>,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub time_in_force: Option<TimeInForce>,
    pub price: Option<ExactDecimal>,
    pub quantity: ExactDecimal,
    pub stop_price: Option<ExactDecimal>,
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct TradingOrder {
    pub order_id: i64,
    pub run_id: i64,
    pub created_event: TradingRunEvent,
    pub intent_event_id: Option<i64>,
    pub client_order_id: Option<String>,
    pub exchange_order_id: Option<String>,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub time_in_force: Option<TimeInForce>,
    pub price: Option<ExactDecimal>,
    pub quantity: ExactDecimal,
    pub stop_price: Option<ExactDecimal>,
    pub metadata: Value,
}

#[derive(Clone, Debug)]
pub struct OrderStateInput {
    pub order_id: i64,
    pub times: EventTimes,
    pub status: OrderStatus,
    pub filled_quantity: ExactDecimal,
    pub average_fill_price: Option<ExactDecimal>,
    pub reject_reason: Option<String>,
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderStateRecord {
    pub event: TradingRunEvent,
    pub order_id: i64,
    pub status: OrderStatus,
    pub filled_quantity: ExactDecimal,
    pub average_fill_price: Option<ExactDecimal>,
    pub reject_reason: Option<String>,
    pub metadata: Value,
}

#[derive(Clone, Debug)]
pub struct FillInput {
    pub order_id: i64,
    pub times: EventTimes,
    pub exchange_trade_id: Option<String>,
    pub price: ExactDecimal,
    pub quantity: ExactDecimal,
    pub fee: Option<ExactDecimal>,
    pub fee_asset: Option<String>,
    pub liquidity_role: Option<LiquidityRole>,
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct FillRecord {
    pub event: TradingRunEvent,
    pub order_id: i64,
    pub exchange_trade_id: Option<String>,
    pub price: ExactDecimal,
    pub quantity: ExactDecimal,
    pub fee: Option<ExactDecimal>,
    pub fee_asset: Option<String>,
    pub liquidity_role: Option<LiquidityRole>,
    pub metadata: Value,
}

#[derive(Clone, Debug)]
pub struct PositionSnapshotInput {
    pub run_id: i64,
    pub times: EventTimes,
    pub position_quantity: ExactDecimal,
    pub average_entry_price: Option<ExactDecimal>,
    pub mark_price: Option<ExactDecimal>,
    pub realized_pnl: Option<ExactDecimal>,
    pub unrealized_pnl: Option<ExactDecimal>,
    pub cash_balance: Option<ExactDecimal>,
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct PositionSnapshotRecord {
    pub event: TradingRunEvent,
    pub position_quantity: ExactDecimal,
    pub average_entry_price: Option<ExactDecimal>,
    pub mark_price: Option<ExactDecimal>,
    pub realized_pnl: Option<ExactDecimal>,
    pub unrealized_pnl: Option<ExactDecimal>,
    pub cash_balance: Option<ExactDecimal>,
    pub metadata: Value,
}

#[derive(Clone, Debug)]
pub struct EquitySnapshotInput {
    pub run_id: i64,
    pub times: EventTimes,
    pub equity: ExactDecimal,
    pub cash_balance: Option<ExactDecimal>,
    pub realized_pnl: Option<ExactDecimal>,
    pub unrealized_pnl: Option<ExactDecimal>,
    pub fees_paid: Option<ExactDecimal>,
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct EquitySnapshotRecord {
    pub event: TradingRunEvent,
    pub equity: ExactDecimal,
    pub cash_balance: Option<ExactDecimal>,
    pub realized_pnl: Option<ExactDecimal>,
    pub unrealized_pnl: Option<ExactDecimal>,
    pub fees_paid: Option<ExactDecimal>,
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunStatusRecord {
    pub event: TradingRunEvent,
    pub status: RunStatus,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TradingRunHistory {
    pub run: TradingRun,
    pub events: Vec<TradingRunEvent>,
    pub status_events: Vec<RunStatusRecord>,
    pub decisions: Vec<DecisionRecord>,
    pub order_intents: Vec<OrderIntentRecord>,
    pub orders: Vec<TradingOrder>,
    pub order_states: Vec<OrderStateRecord>,
    pub fills: Vec<FillRecord>,
    pub positions: Vec<PositionSnapshotRecord>,
    pub equity: Vec<EquitySnapshotRecord>,
}
