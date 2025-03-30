use crate::{OrderBook, OrderBookError, current_time_millis};
use pricelevel::{OrderType, Side};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tracing::trace;

impl OrderBook {
    /// Check if an order has expired
    pub(super) fn has_expired(&self, order: &OrderType) -> bool {
        let time_in_force = order.time_in_force();
        let current_time = current_time_millis();

        // Only check market close timestamp if we have one set
        let market_close = if self.has_market_close.load(Ordering::Relaxed) {
            Some(self.market_close_timestamp.load(Ordering::Relaxed))
        } else {
            None
        };

        time_in_force.is_expired(current_time, market_close)
    }

    /// Check if there would be a price crossing
    pub(super) fn will_cross_market(&self, price: u64, side: Side) -> bool {
        match side {
            Side::Buy => {
                if let Some(best_ask) = self.best_ask() {
                    price >= best_ask
                } else {
                    false
                }
            }
            Side::Sell => {
                if let Some(best_bid) = self.best_bid() {
                    price <= best_bid
                } else {
                    false
                }
            }
        }
    }

    /// Handle immediate-or-cancel and fill-or-kill orders
    pub(super) fn handle_immediate_order(
        &self,
        order: OrderType,
    ) -> Result<Arc<OrderType>, OrderBookError> {
        trace!(
            "Order book {}: Handling immediate order {} at price {}",
            self.symbol,
            order.id(),
            order.price()
        );
        let id = order.id();
        let quantity = order.visible_quantity();
        let side = order.side();
        let is_fok = order.is_fill_or_kill();

        // Match the order immediately
        let match_result = self.match_market_order(id, quantity, side)?;

        // For FOK orders, if not fully filled, cancel everything
        if is_fok && !match_result.is_complete {
            return Err(OrderBookError::InsufficientLiquidity {
                side,
                requested: quantity,
                available: match_result.executed_quantity(),
            });
        }

        // For IOC orders, any remaining quantity is discarded
        // Create an Arc for the order (even though it's not added to the book)
        let order_arc = Arc::new(order);

        // Update the last trade price if there were transactions
        if !match_result.transactions.is_empty() {
            let transactions = match_result.transactions.as_vec();
            if let Some(last_transaction) = transactions.last() {
                self.last_trade_price
                    .store(last_transaction.price, Ordering::SeqCst);
                self.has_traded.store(true, Ordering::SeqCst);
            }
        }

        Ok(order_arc)
    }
}
