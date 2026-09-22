mod grid;
mod portfolio;
mod simulation;
mod types;

pub use grid::{GridAnchor, StaticGridConfig, StaticGridStrategy};
pub use portfolio::PortfolioState;
pub use simulation::{PendingOrder, SimulatedExecution, SimulatedFill};
pub use types::{
    ExecutionAssumptions, LimitFillPolicy, MarketCandle, PortfolioView, Strategy,
    StrategyContext, StrategyDecision, StrategyOrderIntent, StrategyOutput, StrategyStartContext,
};

pub fn decimal_string(value: f64) -> Result<String, String> {
    if !value.is_finite() {
        return Err("numeric value must be finite".into());
    }
    let normalized = if value.abs() < 0.0000000000005 { 0.0 } else { value };
    Ok(format!("{normalized:.12}"))
}
