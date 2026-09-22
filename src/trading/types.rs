use crate::storage::{OrderSide, OrderType, TimeInForce};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct MarketCandle {
    pub open_time_ms: i64,
    pub close_time_ms: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PortfolioView {
    pub cash: f64,
    pub position_quantity: f64,
    pub average_entry_price: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
    pub fees_paid: f64,
    pub equity: f64,
}

#[derive(Clone, Debug)]
pub struct StrategyContext<'a> {
    pub now_ms: i64,
    pub candle: &'a MarketCandle,
    pub portfolio: PortfolioView,
}

#[derive(Clone, Debug)]
pub struct StrategyDecision {
    pub decision_type: String,
    pub payload: Value,
}

#[derive(Clone, Debug)]
pub struct StrategyOrderIntent {
    pub intent_key: Option<String>,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub time_in_force: Option<TimeInForce>,
    pub price: Option<f64>,
    pub quantity: f64,
    pub stop_price: Option<f64>,
    pub reduce_only: bool,
    pub metadata: Value,
}

#[derive(Clone, Debug, Default)]
pub struct StrategyOutput {
    pub decisions: Vec<StrategyDecision>,
    pub order_intents: Vec<StrategyOrderIntent>,
}

pub trait Strategy {
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    fn parameters(&self) -> Value;
    fn on_candle(&mut self, context: &StrategyContext<'_>) -> Result<StrategyOutput, String>;
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LimitFillPolicy {
    Touch,
    TradeThrough,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionAssumptions {
    pub fee_bps: f64,
    pub spread_bps: f64,
    pub slippage_bps: f64,
    pub latency_ms: i64,
    pub limit_fill_policy: LimitFillPolicy,
    pub partial_fill_ratio: f64,
}

impl Default for ExecutionAssumptions {
    fn default() -> Self {
        Self {
            fee_bps: 0.0,
            spread_bps: 0.0,
            slippage_bps: 0.0,
            latency_ms: 0,
            limit_fill_policy: LimitFillPolicy::Touch,
            partial_fill_ratio: 1.0,
        }
    }
}

impl ExecutionAssumptions {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("fee_bps", self.fee_bps),
            ("spread_bps", self.spread_bps),
            ("slippage_bps", self.slippage_bps),
            ("partial_fill_ratio", self.partial_fill_ratio),
        ] {
            if !value.is_finite() {
                return Err(format!("{name} must be finite"));
            }
        }
        if self.fee_bps < 0.0 || self.spread_bps < 0.0 || self.slippage_bps < 0.0 {
            return Err("fee/spread/slippage bps cannot be negative".into());
        }
        if self.latency_ms < 0 {
            return Err("latency_ms cannot be negative".into());
        }
        if !(0.0 < self.partial_fill_ratio && self.partial_fill_ratio <= 1.0) {
            return Err("partial_fill_ratio must be in (0, 1]".into());
        }
        Ok(())
    }
}
