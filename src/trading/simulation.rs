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
pub struct ExecutionFill {
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
pub struct LiveTradeEvent {
    pub trade_id: u64,
    pub event_time_ms: i64,
    pub price: f64,
    pub quantity: f64,
}

#[derive(Clone, Debug)]
pub enum LiveExecutionEvent {
    Trade(LiveTradeEvent),
}

#[derive(Clone, Debug)]
struct ExecutionBook {
    assumptions: ExecutionAssumptions,
    pending: Vec<PendingOrder>,
}

impl ExecutionBook {
    fn new(assumptions: ExecutionAssumptions) -> Result<Self, String> {
        assumptions.validate()?;
        Ok(Self {
            assumptions,
            pending: Vec::new(),
        })
    }

    fn assumptions(&self) -> &ExecutionAssumptions {
        &self.assumptions
    }

    fn submit(
        &mut self,
        order_id: i64,
        submitted_at_ms: i64,
        intent: StrategyOrderIntent,
    ) -> Result<(), String> {
        validate_intent_for_execution(&intent)?;
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

    fn pending_orders(&self) -> &[PendingOrder] {
        &self.pending
    }

    fn expire_all(&mut self) -> Vec<PendingOrder> {
        self.pending.drain(..).collect()
    }

    fn retain_open(&mut self) {
        self.pending.retain(|order| order.remaining_quantity > 0.0);
    }
}

#[derive(Clone, Debug)]
pub struct HistoricalExecution {
    book: ExecutionBook,
}

impl HistoricalExecution {
    pub fn new(assumptions: ExecutionAssumptions) -> Result<Self, String> {
        Ok(Self {
            book: ExecutionBook::new(assumptions)?,
        })
    }

    pub fn assumptions(&self) -> &ExecutionAssumptions {
        self.book.assumptions()
    }

    pub fn submit(
        &mut self,
        order_id: i64,
        submitted_at_ms: i64,
        intent: StrategyOrderIntent,
    ) -> Result<(), String> {
        self.book.submit(order_id, submitted_at_ms, intent)
    }

    pub fn process_candle(&mut self, candle: &MarketCandle) -> Result<Vec<ExecutionFill>, String> {
        let mut fills = Vec::new();
        let assumptions = self.book.assumptions.clone();

        for order in &mut self.book.pending {
            if order.remaining_quantity <= 0.0 || order.eligible_from_ms > candle.open_time_ms {
                continue;
            }

            let maybe_price = match order.intent.order_type {
                OrderType::Market => Some(market_fill_price(
                    &assumptions,
                    order.intent.side,
                    candle.open,
                )?),
                OrderType::Limit => {
                    let limit = order.intent.price.expect("validated limit price");
                    historical_limit_is_fillable(
                        &assumptions,
                        order.intent.side,
                        limit,
                        candle,
                    )
                    .then_some(limit)
                }
                _ => None,
            };

            let Some(price) = maybe_price else {
                continue;
            };
            let event_time_ms = if order.intent.order_type == OrderType::Market {
                candle.open_time_ms
            } else {
                candle.close_time_ms
            };
            fills.push(fill_order(order, price, event_time_ms, &assumptions)?);
        }

        self.book.retain_open();
        Ok(fills)
    }

    pub fn pending_orders(&self) -> &[PendingOrder] {
        self.book.pending_orders()
    }

    pub fn expire_all(&mut self) -> Vec<PendingOrder> {
        self.book.expire_all()
    }
}

#[derive(Clone, Debug)]
pub struct LivePaperExecution {
    book: ExecutionBook,
}

impl LivePaperExecution {
    pub fn new(assumptions: ExecutionAssumptions) -> Result<Self, String> {
        Ok(Self {
            book: ExecutionBook::new(assumptions)?,
        })
    }

    pub fn assumptions(&self) -> &ExecutionAssumptions {
        self.book.assumptions()
    }

    pub fn submit(
        &mut self,
        order_id: i64,
        submitted_at_ms: i64,
        intent: StrategyOrderIntent,
    ) -> Result<(), String> {
        self.book.submit(order_id, submitted_at_ms, intent)
    }

    pub fn process_event(
        &mut self,
        event: &LiveExecutionEvent,
    ) -> Result<Vec<ExecutionFill>, String> {
        match event {
            LiveExecutionEvent::Trade(trade) => self.process_trade(trade),
        }
    }

    pub fn pending_orders(&self) -> &[PendingOrder] {
        self.book.pending_orders()
    }

    pub fn expire_all(&mut self) -> Vec<PendingOrder> {
        self.book.expire_all()
    }

    fn process_trade(&mut self, trade: &LiveTradeEvent) -> Result<Vec<ExecutionFill>, String> {
        validate_live_trade(trade)?;
        let mut fills = Vec::new();
        let assumptions = self.book.assumptions.clone();

        for order in &mut self.book.pending {
            if order.remaining_quantity <= 0.0 || order.eligible_from_ms > trade.event_time_ms {
                continue;
            }

            let maybe_price = match order.intent.order_type {
                OrderType::Market => Some(market_fill_price(
                    &assumptions,
                    order.intent.side,
                    trade.price,
                )?),
                OrderType::Limit => {
                    let limit = order.intent.price.expect("validated limit price");
                    live_limit_is_fillable(
                        &assumptions,
                        order.intent.side,
                        limit,
                        trade.price,
                    )
                    .then_some(limit)
                }
                _ => None,
            };

            let Some(price) = maybe_price else {
                continue;
            };
            fills.push(fill_order(order, price, trade.event_time_ms, &assumptions)?);
        }

        self.book.retain_open();
        Ok(fills)
    }
}

fn validate_intent_for_execution(intent: &StrategyOrderIntent) -> Result<(), String> {
    if !intent.quantity.is_finite() || intent.quantity <= 0.0 {
        return Err("order quantity must be finite and positive".into());
    }

    match intent.order_type {
        OrderType::Market => Ok(()),
        OrderType::Limit => {
            let price = intent
                .price
                .ok_or_else(|| "limit order requires price".to_string())?;
            if !price.is_finite() || price <= 0.0 {
                return Err("limit price must be finite and positive".into());
            }
            Ok(())
        }
        _ => Err("execution adapters support market and limit orders only".into()),
    }
}

fn validate_live_trade(trade: &LiveTradeEvent) -> Result<(), String> {
    if trade.event_time_ms < 0 {
        return Err("live trade event_time_ms cannot be negative".into());
    }
    if !trade.price.is_finite() || trade.price <= 0.0 {
        return Err("live trade price must be finite and positive".into());
    }
    if !trade.quantity.is_finite() || trade.quantity <= 0.0 {
        return Err("live trade quantity must be finite and positive".into());
    }
    Ok(())
}

fn fill_order(
    order: &mut PendingOrder,
    price: f64,
    event_time_ms: i64,
    assumptions: &ExecutionAssumptions,
) -> Result<ExecutionFill, String> {
    let chunk = (order.original_quantity * assumptions.partial_fill_ratio)
        .min(order.remaining_quantity);
    if chunk <= 0.0 {
        return Err("fillable order produced a non-positive fill quantity".into());
    }

    order.remaining_quantity -= chunk;
    if order.remaining_quantity.abs() < 1e-12 {
        order.remaining_quantity = 0.0;
    }

    let notional = chunk * price;
    order.filled_quantity += chunk;
    order.filled_notional += notional;
    let average_fill_price = order.filled_notional / order.filled_quantity;
    let fee = notional * assumptions.fee_bps / 10_000.0;
    let status = if order.remaining_quantity == 0.0 {
        OrderStatus::Filled
    } else {
        OrderStatus::PartiallyFilled
    };

    Ok(ExecutionFill {
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
    })
}

fn market_fill_price(
    assumptions: &ExecutionAssumptions,
    side: OrderSide,
    reference_price: f64,
) -> Result<f64, String> {
    if !reference_price.is_finite() || reference_price <= 0.0 {
        return Err("market reference price must be finite and positive".into());
    }
    let half_spread = assumptions.spread_bps / 2.0;
    let adverse_bps = half_spread + assumptions.slippage_bps;
    let multiplier = match side {
        OrderSide::Buy => 1.0 + adverse_bps / 10_000.0,
        OrderSide::Sell => 1.0 - adverse_bps / 10_000.0,
    };
    Ok(reference_price * multiplier)
}

fn historical_limit_is_fillable(
    assumptions: &ExecutionAssumptions,
    side: OrderSide,
    limit: f64,
    candle: &MarketCandle,
) -> bool {
    match (side, assumptions.limit_fill_policy) {
        (OrderSide::Buy, LimitFillPolicy::Touch) => candle.low <= limit,
        (OrderSide::Sell, LimitFillPolicy::Touch) => candle.high >= limit,
        (OrderSide::Buy, LimitFillPolicy::TradeThrough) => candle.low < limit,
        (OrderSide::Sell, LimitFillPolicy::TradeThrough) => candle.high > limit,
    }
}

fn live_limit_is_fillable(
    assumptions: &ExecutionAssumptions,
    side: OrderSide,
    limit: f64,
    trade_price: f64,
) -> bool {
    match (side, assumptions.limit_fill_policy) {
        (OrderSide::Buy, LimitFillPolicy::Touch) => trade_price <= limit,
        (OrderSide::Sell, LimitFillPolicy::Touch) => trade_price >= limit,
        (OrderSide::Buy, LimitFillPolicy::TradeThrough) => trade_price < limit,
        (OrderSide::Sell, LimitFillPolicy::TradeThrough) => trade_price > limit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        storage::{OrderSide, TimeInForce},
        trading::{
            GridAnchor, PortfolioView, StaticGridConfig, StaticGridStrategy, Strategy,
            StrategyStartContext,
        },
    };

    fn assumptions() -> ExecutionAssumptions {
        ExecutionAssumptions::default()
    }

    fn previous_candle() -> MarketCandle {
        MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 102.0,
            low: 99.0,
            close: 101.0,
            volume: 10.0,
        }
    }

    fn portfolio() -> PortfolioView {
        PortfolioView {
            cash: 100_000.0,
            position_quantity: 0.0,
            average_entry_price: 0.0,
            realized_pnl: 0.0,
            unrealized_pnl: 0.0,
            fees_paid: 0.0,
            equity: 100_000.0,
        }
    }

    #[test]
    fn historical_execution_preserves_resting_candle_touch_semantics() {
        let mut execution = HistoricalExecution::new(assumptions()).unwrap();
        execution
            .submit(
                1,
                0,
                StrategyOrderIntent {
                    intent_key: Some("buy".into()),
                    side: OrderSide::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: Some(TimeInForce::Gtc),
                    price: Some(99.5),
                    quantity: 1.0,
                    stop_price: None,
                    reduce_only: false,
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();

        let candle = MarketCandle {
            open_time_ms: 60_000,
            close_time_ms: 119_999,
            open: 100.0,
            high: 101.0,
            low: 99.5,
            close: 100.5,
            volume: 1.0,
        };
        let fills = execution.process_candle(&candle).unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].event_time_ms, candle.close_time_ms);
        assert_eq!(fills[0].price, 99.5);
    }

    #[test]
    fn live_paper_execution_accepts_real_time_trade_events() {
        let mut execution = LivePaperExecution::new(assumptions()).unwrap();
        execution
            .submit(
                1,
                1_000,
                StrategyOrderIntent {
                    intent_key: Some("buy".into()),
                    side: OrderSide::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: Some(TimeInForce::Gtc),
                    price: Some(99.5),
                    quantity: 1.0,
                    stop_price: None,
                    reduce_only: false,
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();

        let fills = execution
            .process_event(&LiveExecutionEvent::Trade(LiveTradeEvent {
                trade_id: 42,
                event_time_ms: 1_250,
                price: 99.5,
                quantity: 3.0,
            }))
            .unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].event_time_ms, 1_250);
        assert_eq!(fills[0].price, 99.5);
    }

    #[test]
    fn static_grid_intents_are_identical_when_submitted_to_both_execution_paths() {
        let previous = previous_candle();
        let mut strategy = StaticGridStrategy::new(StaticGridConfig {
            anchor: GridAnchor::PreviousClose,
            fixed_anchor_price: None,
            spacing_bps: 100.0,
            levels_per_side: 2,
            quantity_per_order: 1.25,
            time_in_force: TimeInForce::Gtc,
        })
        .unwrap();
        let output = strategy
            .on_start(&StrategyStartContext {
                now_ms: 60_000,
                previous_candle: Some(&previous),
                portfolio: portfolio(),
            })
            .unwrap();

        let mut historical = HistoricalExecution::new(assumptions()).unwrap();
        let mut live_paper = LivePaperExecution::new(assumptions()).unwrap();

        for (index, intent) in output.order_intents.iter().cloned().enumerate() {
            let order_id = i64::try_from(index + 1).unwrap();
            historical.submit(order_id, 60_000, intent.clone()).unwrap();
            live_paper.submit(order_id, 60_000, intent).unwrap();
        }

        assert_eq!(
            historical.pending_orders().len(),
            live_paper.pending_orders().len()
        );
        for (historical_order, live_order) in historical
            .pending_orders()
            .iter()
            .zip(live_paper.pending_orders())
        {
            assert_eq!(historical_order.order_id, live_order.order_id);
            assert_eq!(historical_order.submitted_at_ms, live_order.submitted_at_ms);
            assert_eq!(historical_order.eligible_from_ms, live_order.eligible_from_ms);
            assert_eq!(historical_order.intent.intent_key, live_order.intent.intent_key);
            assert_eq!(historical_order.intent.side, live_order.intent.side);
            assert_eq!(historical_order.intent.order_type, live_order.intent.order_type);
            assert_eq!(historical_order.intent.price, live_order.intent.price);
            assert_eq!(historical_order.intent.quantity, live_order.intent.quantity);
            assert_eq!(
                historical_order.intent.time_in_force,
                live_order.intent.time_in_force
            );
        }
    }
}
