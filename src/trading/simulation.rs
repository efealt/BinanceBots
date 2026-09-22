use super::types::{ExecutionAssumptions, LimitFillPolicy, MarketCandle, StrategyOrderIntent};
use crate::storage::{OrderSide, OrderStatus, OrderType};

#[derive(Clone, Debug)]
pub struct PendingOrder {
    pub order_id: i64,
    pub intent: StrategyOrderIntent,
    pub submitted_at_ms: i64,
    pub eligible_from_ms: i64,
    pub original_quantity: f64,
    pub remaining_quantity: f64,
    pub filled_quantity: f64,
    pub filled_notional: f64,
}

#[derive(Clone, Debug)]
pub struct SimulatedFill {
    pub order_id: i64,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: f64,
    pub price: f64,
    pub fee: f64,
    pub event_time_ms: i64,
    pub status: OrderStatus,
    pub cumulative_filled_quantity: f64,
    pub average_fill_price: f64,
}

#[derive(Clone, Debug)]
pub struct SimulatedExecution {
    assumptions: ExecutionAssumptions,
    pending: Vec<PendingOrder>,
}

impl SimulatedExecution {
    pub fn new(assumptions: ExecutionAssumptions) -> Result<Self, String> {
        assumptions.validate()?;
        Ok(Self { assumptions, pending: Vec::new() })
    }

    pub fn assumptions(&self) -> &ExecutionAssumptions {
        &self.assumptions
    }

    pub fn submit(
        &mut self,
        order_id: i64,
        submitted_at_ms: i64,
        intent: StrategyOrderIntent,
    ) -> Result<(), String> {
        if !intent.quantity.is_finite() || intent.quantity <= 0.0 {
            return Err("order quantity must be finite and positive".into());
        }
        match intent.order_type {
            OrderType::Market => {}
            OrderType::Limit => {
                let price = intent.price.ok_or_else(|| "limit order requires price".to_string())?;
                if !price.is_finite() || price <= 0.0 {
                    return Err("limit price must be finite and positive".into());
                }
            }
            _ => return Err("Phase 2 simulator supports market and limit orders only".into()),
        }

        let eligible_from_ms = submitted_at_ms
            .checked_add(self.assumptions.latency_ms)
            .ok_or_else(|| "order eligibility timestamp overflow".to_string())?;

        self.pending.push(PendingOrder {
            order_id,
            original_quantity: intent.quantity,
            remaining_quantity: intent.quantity,
            filled_quantity: 0.0,
            filled_notional: 0.0,
            submitted_at_ms,
            eligible_from_ms,
            intent,
        });
        Ok(())
    }

    pub fn process_candle(&mut self, candle: &MarketCandle) -> Result<Vec<SimulatedFill>, String> {
        let mut fills = Vec::new();

        for order in &mut self.pending {
            if order.remaining_quantity <= 0.0 || order.eligible_from_ms > candle.open_time_ms {
                continue;
            }

            let maybe_price = match order.intent.order_type {
                OrderType::Market => Some(self.market_fill_price(order.intent.side, candle.open)?),
                OrderType::Limit => {
                    let limit = order.intent.price.expect("validated limit price");
                    if self.limit_is_fillable(order.intent.side, limit, candle) {
                        Some(limit)
                    } else {
                        None
                    }
                }
                _ => None,
            };

            let Some(price) = maybe_price else { continue };
            let chunk = (order.original_quantity * self.assumptions.partial_fill_ratio)
                .min(order.remaining_quantity);
            if chunk <= 0.0 {
                continue;
            }
            order.remaining_quantity -= chunk;
            if order.remaining_quantity.abs() < 1e-12 {
                order.remaining_quantity = 0.0;
            }
            let notional = chunk * price;
            order.filled_quantity += chunk;
            order.filled_notional += notional;
            let average_fill_price = order.filled_notional / order.filled_quantity;
            let fee = notional * self.assumptions.fee_bps / 10_000.0;
            let status = if order.remaining_quantity == 0.0 {
                OrderStatus::Filled
            } else {
                OrderStatus::PartiallyFilled
            };
            let event_time_ms = if order.intent.order_type == OrderType::Market {
                candle.open_time_ms
            } else {
                candle.close_time_ms
            };
            fills.push(SimulatedFill {
                order_id: order.order_id,
                side: order.intent.side,
                order_type: order.intent.order_type,
                quantity: chunk,
                price,
                fee,
                event_time_ms,
                status,
                cumulative_filled_quantity: order.filled_quantity,
                average_fill_price,
            });
        }

        self.pending.retain(|order| order.remaining_quantity > 0.0);
        Ok(fills)
    }

    pub fn expire_all(&mut self) -> Vec<PendingOrder> {
        self.pending.drain(..).collect()
    }

    fn market_fill_price(&self, side: OrderSide, open: f64) -> Result<f64, String> {
        if !open.is_finite() || open <= 0.0 {
            return Err("market open must be finite and positive".into());
        }
        let half_spread = self.assumptions.spread_bps / 2.0;
        let adverse_bps = half_spread + self.assumptions.slippage_bps;
        let multiplier = match side {
            OrderSide::Buy => 1.0 + adverse_bps / 10_000.0,
            OrderSide::Sell => 1.0 - adverse_bps / 10_000.0,
        };
        Ok(open * multiplier)
    }

    fn limit_is_fillable(&self, side: OrderSide, limit: f64, candle: &MarketCandle) -> bool {
        match (side, self.assumptions.limit_fill_policy) {
            (OrderSide::Buy, LimitFillPolicy::Touch) => candle.low <= limit,
            (OrderSide::Sell, LimitFillPolicy::Touch) => candle.high >= limit,
            (OrderSide::Buy, LimitFillPolicy::TradeThrough) => candle.low < limit,
            (OrderSide::Sell, LimitFillPolicy::TradeThrough) => candle.high > limit,
        }
    }
}
