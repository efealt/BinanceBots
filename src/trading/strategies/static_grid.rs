use super::super::{Strategy, StrategyOutput, StrategyStartContext, StrategyContext, StrategyDecision, StrategyOrderIntent};
use crate::storage::{OrderSide, OrderType, TimeInForce};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GridAnchor {
    PreviousClose,
    Fixed,
}

#[derive(Clone, Debug, Serialize)]
pub struct StaticGridConfig {
    pub anchor: GridAnchor,
    pub fixed_anchor_price: Option<f64>,
    pub spacing_bps: f64,
    pub levels_per_side: u32,
    pub quantity_per_order: f64,
    pub time_in_force: TimeInForce,
}

impl StaticGridConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.spacing_bps.is_finite() || self.spacing_bps <= 0.0 || self.spacing_bps >= 10_000.0 {
            return Err("grid spacing_bps must be finite and in (0, 10000)".into());
        }
        if self.levels_per_side == 0 {
            return Err("grid levels_per_side must be positive".into());
        }
        if !self.quantity_per_order.is_finite() || self.quantity_per_order <= 0.0 {
            return Err("grid quantity_per_order must be finite and positive".into());
        }
        match self.anchor {
            GridAnchor::PreviousClose => {}
            GridAnchor::Fixed => {
                let price = self
                    .fixed_anchor_price
                    .ok_or_else(|| "fixed grid anchor requires fixed_anchor_price".to_string())?;
                if !price.is_finite() || price <= 0.0 {
                    return Err("fixed_anchor_price must be finite and positive".into());
                }
            }
        }
        Ok(())
    }
}

pub struct StaticGridStrategy {
    config: StaticGridConfig,
    initialized: bool,
}

impl StaticGridStrategy {
    pub fn new(config: StaticGridConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(Self { config, initialized: false })
    }

    pub fn config(&self) -> &StaticGridConfig {
        &self.config
    }

    fn anchor_price(&self, context: &StrategyStartContext<'_>) -> Result<f64, String> {
        match self.config.anchor {
            GridAnchor::PreviousClose => context
                .previous_candle
                .map(|candle| candle.close)
                .ok_or_else(|| "previous_close grid anchor requires one completed pre-roll candle".to_string()),
            GridAnchor::Fixed => Ok(self
                .config
                .fixed_anchor_price
                .expect("validated fixed anchor price")),
        }
    }
}

impl Strategy for StaticGridStrategy {
    fn id(&self) -> &str {
        "static-grid-fixture"
    }

    fn version(&self) -> &str {
        "1"
    }

    fn parameters(&self) -> Value {
        serde_json::to_value(&self.config).expect("grid configuration is serializable")
    }

    fn requires_previous_candle(&self) -> bool {
        matches!(self.config.anchor, GridAnchor::PreviousClose)
    }

    fn on_start(&mut self, context: &StrategyStartContext<'_>) -> Result<StrategyOutput, String> {
        if self.initialized {
            return Err("static grid cannot be initialized twice".into());
        }
        self.config.validate()?;
        let anchor = self.anchor_price(context)?;
        let step = self.config.spacing_bps / 10_000.0;
        let mut intents = Vec::with_capacity((self.config.levels_per_side * 2) as usize);

        for level in 1..=self.config.levels_per_side {
            let offset = step * f64::from(level);
            let buy_price = anchor * (1.0 - offset);
            let sell_price = anchor * (1.0 + offset);
            if buy_price <= 0.0 {
                return Err(format!("grid buy level {level} is not positive"));
            }

            intents.push(StrategyOrderIntent {
                intent_key: Some(format!("grid-buy-{level}")),
                side: OrderSide::Buy,
                order_type: OrderType::Limit,
                time_in_force: Some(self.config.time_in_force),
                price: Some(buy_price),
                quantity: self.config.quantity_per_order,
                stop_price: None,
                reduce_only: false,
                metadata: json!({
                    "grid_level": level,
                    "grid_side": "buy",
                    "anchor_price": anchor,
                    "spacing_bps": self.config.spacing_bps,
                    "source": "on_start"
                }),
            });
            intents.push(StrategyOrderIntent {
                intent_key: Some(format!("grid-sell-{level}")),
                side: OrderSide::Sell,
                order_type: OrderType::Limit,
                time_in_force: Some(self.config.time_in_force),
                price: Some(sell_price),
                quantity: self.config.quantity_per_order,
                stop_price: None,
                reduce_only: false,
                metadata: json!({
                    "grid_level": level,
                    "grid_side": "sell",
                    "anchor_price": anchor,
                    "spacing_bps": self.config.spacing_bps,
                    "source": "on_start"
                }),
            });
        }

        self.initialized = true;
        Ok(StrategyOutput {
            decisions: vec![StrategyDecision {
                decision_type: "initialize_static_grid".into(),
                payload: json!({
                    "anchor_price": anchor,
                    "anchor": self.config.anchor,
                    "spacing_bps": self.config.spacing_bps,
                    "levels_per_side": self.config.levels_per_side,
                    "quantity_per_order": self.config.quantity_per_order,
                    "time_in_force": self.config.time_in_force
                }),
            }],
            order_intents: intents,
        })
    }

    fn on_candle(&mut self, _context: &StrategyContext<'_>) -> Result<StrategyOutput, String> {
        Ok(StrategyOutput::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{MarketCandle, PortfolioView};

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
    fn static_grid_uses_previous_completed_close_and_builds_symmetric_resting_orders() {
        let previous = MarketCandle {
            open_time_ms: 0,
            close_time_ms: 59_999,
            open: 100.0,
            high: 102.0,
            low: 99.0,
            close: 101.0,
            volume: 10.0,
        };
        let context = StrategyStartContext {
            now_ms: 60_000,
            previous_candle: Some(&previous),
            portfolio: portfolio(),
        };
        let mut strategy = StaticGridStrategy::new(StaticGridConfig {
            anchor: GridAnchor::PreviousClose,
            fixed_anchor_price: None,
            spacing_bps: 100.0,
            levels_per_side: 2,
            quantity_per_order: 1.0,
            time_in_force: TimeInForce::Gtc,
        }).unwrap();

        let output = strategy.on_start(&context).unwrap();
        assert_eq!(output.decisions.len(), 1);
        assert_eq!(output.order_intents.len(), 4);
        assert_eq!(output.order_intents[0].side, OrderSide::Buy);
        assert!((output.order_intents[0].price.unwrap() - 99.99).abs() < 1e-9);
        assert_eq!(output.order_intents[1].side, OrderSide::Sell);
        assert!((output.order_intents[1].price.unwrap() - 102.01).abs() < 1e-9);
        assert!((output.order_intents[2].price.unwrap() - 98.98).abs() < 1e-9);
        assert!((output.order_intents[3].price.unwrap() - 103.02).abs() < 1e-9);
    }
}
