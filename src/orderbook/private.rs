use crate::{OrderBook, current_time_millis};
use pricelevel::{OrderType, Side};
use std::sync::atomic::Ordering;

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
}

#[cfg(test)]
mod test_orderbook_private {
    use crate::{OrderBook, OrderBookError};
    use pricelevel::{OrderId, OrderType, Side, TimeInForce};
    use uuid::Uuid;

    // Helper function to create a unique order ID
    fn create_order_id() -> OrderId {
        OrderId(Uuid::new_v4())
    }

    #[test]
    fn test_has_expired_with_no_market_close() {
        let book = OrderBook::new("TEST");

        // Create a day order
        let order = OrderType::Standard {
            id: create_order_id(),
            price: 1000,
            quantity: 10,
            side: Side::Buy,
            timestamp: crate::utils::current_time_millis(),
            time_in_force: TimeInForce::Day,
        };

        // Day order should not expire if market close is not set
        assert!(!book.has_expired(&order));
    }

    #[test]
    fn test_has_expired_with_market_close() {
        let book = OrderBook::new("TEST");

        // Set market close to a past time
        let current_time = crate::utils::current_time_millis();
        book.set_market_close_timestamp(current_time - 1000); // 1 second ago

        // Create a day order
        let order = OrderType::Standard {
            id: create_order_id(),
            price: 1000,
            quantity: 10,
            side: Side::Buy,
            timestamp: current_time,
            time_in_force: TimeInForce::Day,
        };

        // Day order should expire if market close is in the past
        assert!(book.has_expired(&order));
    }

    #[test]
    fn test_will_cross_market_buy_no_ask() {
        let book = OrderBook::new("TEST");

        // No ask orders yet, should not cross
        assert!(!book.will_cross_market(1000, Side::Buy));
    }

    #[test]
    fn test_will_cross_market_sell_no_bid() {
        let book = OrderBook::new("TEST");

        // No bid orders yet, should not cross
        assert!(!book.will_cross_market(1000, Side::Sell));
    }

    #[test]
    fn test_will_cross_market_buy_with_cross() {
        let book = OrderBook::new("TEST");

        // Add a sell order at 1000
        let id = create_order_id();
        let result = book.add_limit_order(id, 1000, 10, Side::Sell, TimeInForce::Gtc);
        assert!(result.is_ok());

        // Buy at 1000 should cross
        assert!(book.will_cross_market(1000, Side::Buy));

        // Buy at 1001 should cross
        assert!(book.will_cross_market(1001, Side::Buy));

        // Buy at 999 should not cross
        assert!(!book.will_cross_market(999, Side::Buy));
    }

    #[test]
    fn test_will_cross_market_sell_with_cross() {
        let book = OrderBook::new("TEST");

        // Add a buy order at 1000
        let id = create_order_id();
        let result = book.add_limit_order(id, 1000, 10, Side::Buy, TimeInForce::Gtc);
        assert!(result.is_ok());

        // Sell at 1000 should cross
        assert!(book.will_cross_market(1000, Side::Sell));

        // Sell at 999 should cross
        assert!(book.will_cross_market(999, Side::Sell));

        // Sell at 1001 should not cross
        assert!(!book.will_cross_market(1001, Side::Sell));
    }

    #[test]
    fn test_match_market_order_partial_availability() {
        let book = OrderBook::new("TEST");

        // Add an ask with only 5 units available
        let sell_id = create_order_id();
        let _ = book.add_limit_order(sell_id, 1000, 5, Side::Sell, TimeInForce::Gtc);

        // Try to execute a buy for 10 units
        let buy_id = create_order_id();
        let result = book.match_market_order(buy_id, 10, Side::Buy);

        // Should execute partially
        assert!(result.is_ok());
        let match_result = result.unwrap();

        // Check the match result
        assert_eq!(match_result.executed_quantity(), 5);
        assert_eq!(match_result.remaining_quantity, 5);
        assert!(!match_result.is_complete);

        // Ask side should be empty now
        assert_eq!(book.best_ask(), None);
    }

    #[test]
    fn test_match_market_order_no_matches() {
        let book = OrderBook::new("TEST");

        // Attempt to match a market order on an empty book
        let id = create_order_id();
        let result = book.match_market_order(id, 10, Side::Buy);

        // Should return an error since there are no matching orders
        assert!(result.is_err());
        match result {
            Err(OrderBookError::InsufficientLiquidity {
                side,
                requested,
                available,
            }) => {
                assert_eq!(side, Side::Buy);
                assert_eq!(requested, 10);
                assert_eq!(available, 0);
            }
            _ => panic!("Expected InsufficientLiquidity error"),
        }
    }
}
