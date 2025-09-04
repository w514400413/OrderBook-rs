use crate::{OrderBook, OrderBookError, current_time_millis};
use pricelevel::{OrderType, PriceLevel, Side};
use std::sync::Arc;
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
            Side::Buy => self.best_ask().is_some_and(|best_ask| price >= best_ask),
            Side::Sell => self.best_bid().is_some_and(|best_bid| price <= best_bid),
        }
    }

    /// Places a resting order in the book, updates its location.
    #[allow(dead_code)]
    pub(super) fn place_order_in_book(
        &self,
        order: Arc<OrderType>,
    ) -> Result<Arc<OrderType>, OrderBookError> {
        let (side, price, order_id) = (order.side(), order.price(), order.id());

        let book_side = match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };

        // Get or create the price level
        let price_level = book_side
            .entry(price)
            .or_insert_with(|| PriceLevel::new(price).into())
            .value()
            .clone();

        // The `add_order` method on PriceLevel expects an `OrderType`, not an `Arc`.
        price_level.add_order(*order.clone());
        // The location is stored as (price, side) for efficient retrieval in cancel_order
        self.order_locations.insert(order_id, (price, side));

        Ok(order)
    }
}

#[cfg(test)]
mod tests {
    use crate::OrderBookError; // Import the error type
    use crate::orderbook::book::OrderBook;
    use crate::utils::current_time_millis; // Import the time utility
    use pricelevel::{OrderId, OrderType, Side, TimeInForce};
    use std::sync::Arc;
    use uuid::Uuid;

    // Helper function to create a unique order ID
    fn create_order_id() -> OrderId {
        OrderId(Uuid::new_v4())
    }

    #[test]
    fn test_private_place_order_in_book() {
        let order_book = OrderBook::new("TEST");
        let order_id = create_order_id();
        let order = Arc::new(OrderType::Standard {
            id: order_id,
            price: 100,
            quantity: 10,
            side: Side::Buy,
            timestamp: crate::utils::current_time_millis(),
            time_in_force: TimeInForce::Gtc,
        });

        assert!(order_book.place_order_in_book(order).is_ok());

        // Verify order location
        let location = order_book.order_locations.get(&order_id).unwrap();
        assert_eq!(*location.value(), (100, Side::Buy));

        // Verify order in price level by checking its properties
        let price_level = order_book.bids.get(&100).unwrap();
        assert_eq!(price_level.order_count(), 1);
        assert_eq!(price_level.total_quantity(), 10); // Check if quantity matches the added order
    }

    #[test]
    fn test_will_cross_market_buy_no_ask() {
        let book = OrderBook::new("TEST");

        // No ask orders yet, should not cross
        assert!(!book.will_cross_market(1000, Side::Buy));
    }

    // This test was missing its function definition
    #[test]
    fn test_has_expired_day_order() {
        let book = OrderBook::new("TEST");
        let current_time = current_time_millis();
        book.set_market_close_timestamp(current_time - 1000); // Set market close in the past

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
