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
        let price = order.price();

        // For FOK orders, pre-check if there's enough liquidity before attempting execution
        if is_fok {
            // Calculate total available liquidity at or better than the limit price
            // Note: We pass the order's own side, not the opposite, because the method
            // handles the side mapping internally
            let available_liquidity = self.calculate_available_liquidity(side, Some(price));

            // Check if there's enough liquidity to fully fill the order
            if available_liquidity < quantity {
                return Err(OrderBookError::InsufficientLiquidity {
                    side,
                    requested: quantity,
                    available: available_liquidity,
                });
            }
        }

        // Match the order immediately
        let match_result = self.match_market_order(id, quantity, side)?;

        // For FOK orders, if not fully filled, cancel everything
        // This is now just a safety check, as we've already pre-checked the liquidity
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

    /// Calculate the available liquidity for a given side at or better than a limit price
    ///
    /// For buy orders, we need to check sell orders with price <= the buy price limit
    /// For sell orders, we need to check buy orders with price >= the sell price limit
    ///
    /// Returns the total quantity available
    fn calculate_available_liquidity(&self, side: Side, price_limit: Option<u64>) -> u64 {
        let mut total_available = 0;

        match side {
            Side::Buy => {
                // For buy orders, look at the ask side (sell orders)
                // Consider all sell orders with price <= the limit price
                let mut ask_prices: Vec<u64> = self.asks.iter().map(|item| *item.key()).collect();
                ask_prices.sort(); // Ascending order by price

                for price in ask_prices {
                    // If we've gone past the price limit, stop
                    if let Some(limit) = price_limit {
                        if price > limit {
                            break;
                        }
                    }

                    if let Some(price_level) = self.asks.get(&price) {
                        total_available += price_level.visible_quantity();
                    }
                }
            }
            Side::Sell => {
                // For sell orders, look at the bid side (buy orders)
                // Consider all buy orders with price >= the limit price
                let mut bid_prices: Vec<u64> = self.bids.iter().map(|item| *item.key()).collect();
                bid_prices.sort_by(|a, b| b.cmp(a)); // Descending order by price

                for price in bid_prices {
                    // If we've gone past the price limit, stop
                    if let Some(limit) = price_limit {
                        if price < limit {
                            break;
                        }
                    }

                    if let Some(price_level) = self.bids.get(&price) {
                        total_available += price_level.visible_quantity();
                    }
                }
            }
        }

        total_available
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
    fn test_handle_immediate_order_fok_insufficient_liquidity() {
        let book = OrderBook::new("TEST");

        // Add a sell order with 5 quantity
        let sell_id = create_order_id();
        let _ = book.add_limit_order(sell_id, 1000, 5, Side::Sell, TimeInForce::Gtc);

        // Create a FOK buy order with 10 quantity (more than available)
        let buy_id = create_order_id();
        let buy_order = OrderType::Standard {
            id: buy_id,
            price: 1000,
            quantity: 10,
            side: Side::Buy,
            timestamp: crate::utils::current_time_millis(),
            time_in_force: TimeInForce::Fok,
        };

        // Attempt to handle the FOK order
        let result = book.handle_immediate_order(buy_order);

        // Should fail with insufficient liquidity
        assert!(result.is_err());
        match result {
            Err(OrderBookError::InsufficientLiquidity {
                side,
                requested,
                available,
            }) => {
                assert_eq!(side, Side::Buy);
                assert_eq!(requested, 10);
                assert_eq!(available, 5);
            }
            _ => panic!("Expected InsufficientLiquidity error"),
        }
    }

    #[test]
    fn test_handle_immediate_order_fok_sufficient_liquidity() {
        let book = OrderBook::new("TEST");

        // Add a sell order with 10 quantity
        let sell_id = create_order_id();
        let _ = book.add_limit_order(sell_id, 1000, 10, Side::Sell, TimeInForce::Gtc);

        // Create a FOK buy order with 10 quantity (equal to available)
        let buy_id = create_order_id();
        let buy_order = OrderType::Standard {
            id: buy_id,
            price: 1000,
            quantity: 10,
            side: Side::Buy,
            timestamp: crate::utils::current_time_millis(),
            time_in_force: TimeInForce::Fok,
        };

        // Handle the FOK order
        let result = book.handle_immediate_order(buy_order);

        // Should succeed
        assert!(result.is_ok());

        // Original sell order should be fully matched and removed
        assert!(book.get_order(sell_id).is_none());
    }

    #[test]
    fn test_calculate_available_liquidity_for_buy() {
        let book = OrderBook::new("TEST");

        // Add sell orders at different price levels
        let id1 = create_order_id();
        let _ = book.add_limit_order(id1, 1000, 10, Side::Sell, TimeInForce::Gtc);

        let id2 = create_order_id();
        let _ = book.add_limit_order(id2, 1010, 15, Side::Sell, TimeInForce::Gtc);

        let id3 = create_order_id();
        let _ = book.add_limit_order(id3, 990, 5, Side::Sell, TimeInForce::Gtc);

        // Calculate liquidity for buy side with price limit
        let liquidity = book.calculate_available_liquidity(Side::Buy, Some(1000));

        // Should include orders at 1000 and below
        assert_eq!(liquidity, 15); // 10 + 5

        // Calculate liquidity with higher price limit
        let liquidity = book.calculate_available_liquidity(Side::Buy, Some(1010));
        assert_eq!(liquidity, 30); // 10 + 5 + 15

        // Calculate liquidity with no price limit
        let liquidity = book.calculate_available_liquidity(Side::Buy, None);
        assert_eq!(liquidity, 30); // All orders
    }

    #[test]
    fn test_calculate_available_liquidity_for_sell() {
        let book = OrderBook::new("TEST");

        // Add buy orders at different price levels
        let id1 = create_order_id();
        let _ = book.add_limit_order(id1, 1000, 10, Side::Buy, TimeInForce::Gtc);

        let id2 = create_order_id();
        let _ = book.add_limit_order(id2, 1010, 15, Side::Buy, TimeInForce::Gtc);

        let id3 = create_order_id();
        let _ = book.add_limit_order(id3, 990, 5, Side::Buy, TimeInForce::Gtc);

        // Calculate liquidity for sell side with price limit
        let liquidity = book.calculate_available_liquidity(Side::Sell, Some(1000));

        // Should include orders at 1000 and above
        assert_eq!(liquidity, 25); // 10 + 15

        // Calculate liquidity with lower price limit
        let liquidity = book.calculate_available_liquidity(Side::Sell, Some(990));
        assert_eq!(liquidity, 30); // 10 + 15 + 5

        // Calculate liquidity with no price limit
        let liquidity = book.calculate_available_liquidity(Side::Sell, None);
        assert_eq!(liquidity, 30); // All orders
    }

    #[test]
    fn test_handle_immediate_order_ioc_partial_fill() {
        let book = OrderBook::new("TEST");

        // Add a sell order with 10 quantity
        let sell_id = create_order_id();
        let _ = book.add_limit_order(sell_id, 1000, 10, Side::Sell, TimeInForce::Gtc);

        // Create an IOC buy order with 15 quantity (more than available)
        let buy_id = create_order_id();
        let buy_order = OrderType::Standard {
            id: buy_id,
            price: 1000,
            quantity: 15,
            side: Side::Buy,
            timestamp: crate::utils::current_time_millis(),
            time_in_force: TimeInForce::Ioc,
        };

        // Handle the IOC order
        let result = book.handle_immediate_order(buy_order);

        // Should succeed (partially filled)
        assert!(result.is_ok());

        // Original sell order should be fully matched and removed
        assert!(book.get_order(sell_id).is_none());

        // Buy order should not be in the book (IOC)
        assert!(book.get_order(buy_id).is_none());

        // Last trade price should be updated
        assert_eq!(book.last_trade_price(), Some(1000));
    }
}

#[cfg(test)]
mod test_private_remaining {
    use crate::OrderBook;
    use pricelevel::{OrderId, OrderType, Side, TimeInForce};
    use uuid::Uuid;

    fn create_order_id() -> OrderId {
        OrderId(Uuid::new_v4())
    }

    #[test]
    fn test_handle_immediate_order_match_ioc_partial() {
        let book = OrderBook::new("TEST");

        // Add orders on the ask side
        let sell_id = create_order_id();
        let _ = book.add_limit_order(sell_id, 1000, 5, Side::Sell, TimeInForce::Gtc);

        // Create an IOC order for 10 (more than available)
        let buy_id = create_order_id();
        let buy_order = OrderType::Standard {
            id: buy_id,
            price: 1000,
            quantity: 10,
            side: Side::Buy,
            timestamp: crate::utils::current_time_millis(),
            time_in_force: TimeInForce::Ioc,
        };

        // Execute the order - should partially fill
        let result = book.handle_immediate_order(buy_order);
        assert!(result.is_ok());

        // Check trade price was set
        assert_eq!(book.last_trade_price(), Some(1000));
        assert!(book.has_traded.load(std::sync::atomic::Ordering::SeqCst));
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
}
