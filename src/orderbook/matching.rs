//! Contains the core matching engine logic for the order book.

use crate::orderbook::pool::MatchingPool;
use crate::{OrderBook, OrderBookError};
use pricelevel::{MatchResult, OrderId, Side};
use std::sync::atomic::Ordering;

impl OrderBook {
    /// Highly optimized internal matching function
    pub fn match_order(
        &self,
        order_id: OrderId,
        side: Side,
        quantity: u64,
        limit_price: Option<u64>,
    ) -> Result<MatchResult, OrderBookError> {
        self.cache.invalidate();
        let mut match_result = MatchResult::new(order_id, quantity);
        let mut remaining_quantity = quantity;

        // Choose the appropriate side for matching
        let match_side = match side {
            Side::Buy => &self.asks,
            Side::Sell => &self.bids,
        };

        // Early exit if the opposite side is empty
        if match_side.is_empty() {
            if limit_price.is_none() {
                return Err(OrderBookError::InsufficientLiquidity {
                    side,
                    requested: quantity,
                    available: 0,
                });
            }
            match_result.remaining_quantity = remaining_quantity;
            return Ok(match_result);
        }

        // Use static memory pool for better performance
        thread_local! {
            static MATCHING_POOL: MatchingPool = MatchingPool::new();
        }

        // Get reusable vectors from pool
        let (mut filled_orders, mut empty_price_levels, mut sorted_prices) =
            MATCHING_POOL.with(|pool| {
                let filled = pool.get_filled_orders_vec();
                let empty = pool.get_price_vec();
                let prices = pool.get_price_vec();
                (filled, empty, prices)
            });

        // Collect and sort prices efficiently
        sorted_prices.extend(match_side.iter().map(|item| *item.key()));

        if side == Side::Buy {
            sorted_prices.sort_unstable(); // Ascending for asks
        } else {
            sorted_prices.sort_unstable_by(|a, b| b.cmp(a)); // Descending for bids
        }

        // Process each price level
        for &price in &sorted_prices {
            // Check price limit constraint early
            if let Some(limit) = limit_price {
                match side {
                    Side::Buy if price > limit => break,
                    Side::Sell if price < limit => break,
                    _ => {}
                }
            }

            // Try to get the price level, skip if removed by another thread
            let mut price_level_entry = match match_side.get_mut(&price) {
                Some(entry) => entry,
                None => continue,
            };

            // Perform the match at this price level
            let price_level_match = {
                let price_level = &mut *price_level_entry;
                price_level.match_order(
                    remaining_quantity,
                    order_id,
                    &self.transaction_id_generator,
                )
            };

            // Process transactions if any occurred
            if !price_level_match.transactions.as_vec().is_empty() {
                // Update last trade price atomically
                self.last_trade_price.store(price, Ordering::Relaxed);
                self.has_traded.store(true, Ordering::Relaxed);

                // Add transactions to result
                for transaction in price_level_match.transactions.as_vec() {
                    match_result.add_transaction(*transaction);
                }
            }

            // Collect filled orders for batch removal
            for &filled_order_id in &price_level_match.filled_order_ids {
                match_result.add_filled_order_id(filled_order_id);
                filled_orders.push(filled_order_id);
            }

            // Update remaining quantity
            remaining_quantity = price_level_match.remaining_quantity;

            // Check if price level is empty and mark for removal
            if price_level_entry.order_count() == 0 {
                empty_price_levels.push(price);
            }

            // Drop the mutable reference before potential removal
            drop(price_level_entry);

            // Early exit if order is fully matched
            if remaining_quantity == 0 {
                break;
            }
        }

        // Batch remove empty price levels
        for price in &empty_price_levels {
            match_side.remove(price);
        }

        // Batch remove filled orders from tracking
        for order_id in &filled_orders {
            self.order_locations.remove(order_id);
        }

        // Return vectors to pool for reuse
        MATCHING_POOL.with(|pool| {
            pool.return_filled_orders_vec(filled_orders);
            pool.return_price_vec(empty_price_levels);
            pool.return_price_vec(sorted_prices);
        });

        // Check for insufficient liquidity in market orders
        if limit_price.is_none() && remaining_quantity == quantity {
            return Err(OrderBookError::InsufficientLiquidity {
                side,
                requested: quantity,
                available: 0,
            });
        }

        // Set final result properties
        match_result.remaining_quantity = remaining_quantity;
        match_result.is_complete = remaining_quantity == 0;

        Ok(match_result)
    }

    /// Optimized peek match with memory pooling
    pub(super) fn peek_match(&self, side: Side, quantity: u64, price_limit: Option<u64>) -> u64 {
        let price_levels = match side {
            Side::Buy => &self.asks,
            Side::Sell => &self.bids,
        };

        if price_levels.is_empty() {
            return 0;
        }

        let mut matched_quantity = 0u64;

        // Use thread-local pool for price vector
        thread_local! {
            static PEEK_POOL: MatchingPool = MatchingPool::new();
        }

        let mut sorted_prices = PEEK_POOL.with(|pool| pool.get_price_vec());

        // Collect and sort prices
        sorted_prices.extend(price_levels.iter().map(|r| *r.key()));

        if side == Side::Buy {
            sorted_prices.sort_unstable(); // Ascending for asks
        } else {
            sorted_prices.sort_unstable_by(|a, b| b.cmp(a)); // Descending for bids
        }

        // Process each price level
        for &price in &sorted_prices {
            // Early termination when we have enough quantity
            if matched_quantity >= quantity {
                break;
            }

            // Check price limit
            if let Some(limit) = price_limit {
                match side {
                    Side::Buy if price > limit => continue,
                    Side::Sell if price < limit => continue,
                    _ => {}
                }
            }

            // Get available quantity at this level
            if let Some(price_level) = price_levels.get(&price) {
                let available_quantity = price_level.total_quantity();
                let needed_quantity = quantity.saturating_sub(matched_quantity);
                let quantity_to_match = needed_quantity.min(available_quantity);
                matched_quantity = matched_quantity.saturating_add(quantity_to_match);
            }
        }

        // Return vector to pool
        PEEK_POOL.with(|pool| pool.return_price_vec(sorted_prices));

        matched_quantity
    }

    /// Batch operation for multiple order matches (additional optimization)
    pub fn match_orders_batch(
        &self,
        orders: &[(OrderId, Side, u64, Option<u64>)],
    ) -> Vec<Result<MatchResult, OrderBookError>> {
        let mut results = Vec::with_capacity(orders.len());

        for &(order_id, side, quantity, limit_price) in orders {
            let result = self.match_order(order_id, side, quantity, limit_price);
            results.push(result);
        }

        results
    }
}
