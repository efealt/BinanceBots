use super::types::PortfolioView;
use crate::storage::OrderSide;

#[derive(Clone, Debug)]
pub struct PortfolioState {
    cash: f64,
    position_quantity: f64,
    average_entry_price: f64,
    realized_pnl: f64,
    fees_paid: f64,
    mark_price: f64,
}

impl PortfolioState {
    pub fn new(initial_cash: f64) -> Result<Self, String> {
        if !initial_cash.is_finite() || initial_cash < 0.0 {
            return Err("initial cash must be finite and non-negative".into());
        }
        Ok(Self {
            cash: initial_cash,
            position_quantity: 0.0,
            average_entry_price: 0.0,
            realized_pnl: 0.0,
            fees_paid: 0.0,
            mark_price: 0.0,
        })
    }

    pub fn apply_fill(
        &mut self,
        side: OrderSide,
        quantity: f64,
        price: f64,
        fee: f64,
    ) -> Result<(), String> {
        if !quantity.is_finite() || quantity <= 0.0 || !price.is_finite() || price <= 0.0 {
            return Err("fill quantity and price must be finite and positive".into());
        }
        if !fee.is_finite() || fee < 0.0 {
            return Err("fill fee must be finite and non-negative".into());
        }

        let signed_delta = match side {
            OrderSide::Buy => quantity,
            OrderSide::Sell => -quantity,
        };
        let old_qty = self.position_quantity;
        let new_qty = old_qty + signed_delta;

        if old_qty == 0.0 || old_qty.signum() == signed_delta.signum() {
            let old_notional = old_qty.abs() * self.average_entry_price;
            let added_notional = quantity * price;
            let total_abs = old_qty.abs() + quantity;
            self.average_entry_price = if total_abs > 0.0 {
                (old_notional + added_notional) / total_abs
            } else {
                0.0
            };
        } else {
            let closed_qty = old_qty.abs().min(quantity);
            self.realized_pnl += closed_qty * (price - self.average_entry_price) * old_qty.signum();
            if new_qty == 0.0 {
                self.average_entry_price = 0.0;
            } else if old_qty.signum() != new_qty.signum() {
                self.average_entry_price = price;
            }
        }

        let notional = quantity * price;
        match side {
            OrderSide::Buy => self.cash -= notional + fee,
            OrderSide::Sell => self.cash += notional - fee,
        }
        self.position_quantity = new_qty;
        self.fees_paid += fee;
        self.mark_price = price;
        Ok(())
    }

    pub fn mark(&mut self, price: f64) -> Result<(), String> {
        if !price.is_finite() || price <= 0.0 {
            return Err("mark price must be finite and positive".into());
        }
        self.mark_price = price;
        Ok(())
    }

    pub fn view(&self) -> PortfolioView {
        let unrealized_pnl = if self.position_quantity == 0.0 {
            0.0
        } else {
            (self.mark_price - self.average_entry_price) * self.position_quantity
        };
        let equity = self.cash + self.position_quantity * self.mark_price;
        PortfolioView {
            cash: self.cash,
            position_quantity: self.position_quantity,
            average_entry_price: self.average_entry_price,
            realized_pnl: self.realized_pnl,
            unrealized_pnl,
            fees_paid: self.fees_paid,
            equity,
        }
    }
}
