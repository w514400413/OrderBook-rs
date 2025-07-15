//! Contains the core matching engine logic for the order book.

use crate::{OrderBook, OrderBookError};
use pricelevel::{MatchResult, OrderId, Side};
use std::sync::atomic::Ordering;

impl OrderBook {
    /// Optimized internal matching function with minimal allocations and iterations
    pub fn match_order(
        &self,
        order_id: OrderId,
        side: Side,
        quantity: u64,
        limit_price: Option<u64>,
    ) -> Result<MatchResult, OrderBookError> {
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

        // Use iterator with early termination instead of collecting all prices
        let price_iterator: Box<dyn Iterator<Item = u64>> = if side == Side::Buy {
            // For buy orders, we want the lowest ask prices first
            let mut prices: Vec<u64> = match_side.iter().map(|item| *item.key()).collect();
            prices.sort_unstable();
            Box::new(prices.into_iter())
        } else {
            // For sell orders, we want the highest bid prices first
            let mut prices: Vec<u64> = match_side.iter().map(|item| *item.key()).collect();
            prices.sort_unstable_by(|a, b| b.cmp(a));
            Box::new(prices.into_iter())
        };

        // Vector to collect orders that need to be removed from tracking
        // Pre-allocate with reasonable capacity to avoid reallocations
        let mut filled_orders = Vec::with_capacity(16);
        let mut empty_price_levels = Vec::with_capacity(8);

        for price in price_iterator {
            // Check price limit constraint early
            if let Some(limit) = limit_price {
                match side {
                    Side::Buy if price > limit => break,
                    Side::Sell if price < limit => break,
                    _ => {}
                }
            }

            // Try to get the price level, skip if it was removed by another thread
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
        for price in empty_price_levels {
            match_side.remove(&price);
        }

        // Batch remove filled orders from tracking
        for order_id in filled_orders {
            self.order_locations.remove(&order_id);
        }

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

    /// Optimized peek match implementation with minimal memory allocation
    pub(super) fn peek_match(&self, side: Side, quantity: u64, price_limit: Option<u64>) -> u64 {
        let price_levels = match side {
            Side::Buy => &self.asks,
            Side::Sell => &self.bids,
        };

        if price_levels.is_empty() {
            return 0;
        }

        let mut matched_quantity = 0u64;

        // Use iterator approach to avoid collecting all keys unnecessarily
        let price_iter: Box<dyn Iterator<Item = u64>> = if side == Side::Buy {
            let mut prices: Vec<u64> = price_levels.iter().map(|r| *r.key()).collect();
            prices.sort_unstable();
            Box::new(prices.into_iter())
        } else {
            let mut prices: Vec<u64> = price_levels.iter().map(|r| *r.key()).collect();
            prices.sort_unstable_by(|a, b| b.cmp(a));
            Box::new(prices.into_iter())
        };

        for price in price_iter {
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

        matched_quantity
    }
}

// impl OrderBook {
//     /// Simulates matching an order to determine the potential outcome without modifying the book.
//     /// This is used for Fill-Or-Kill orders to check if they can be fully matched before executing.
//     pub(super) fn peek_match(&self, side: Side, quantity: u64, price_limit: Option<u64>) -> u64 {
//         let price_levels = match side {
//             Side::Buy => &self.asks,  // Buyers match against asks
//             Side::Sell => &self.bids, // Sellers match against bids
//         };
//
//         let mut matched_quantity = 0;
//
//         let keys: Vec<u64> = if side == Side::Buy {
//             price_levels.iter().map(|r| *r.key()).collect()
//         } else {
//             let mut keys: Vec<u64> = price_levels.iter().map(|r| *r.key()).collect();
//             keys.sort_unstable_by(|a, b| b.cmp(a)); // Bids need descending order for matching
//             keys
//         };
//
//         for price in keys {
//             if matched_quantity >= quantity {
//                 break;
//             }
//
//             if let Some(limit) = price_limit {
//                 if (side == Side::Buy && price > limit) || (side == Side::Sell && price < limit) {
//                     continue; // Skip levels that don't meet the price limit
//                 }
//             }
//
//             if let Some(price_level) = price_levels.get(&price) {
//                 let available_quantity = price_level.total_quantity();
//                 let quantity_to_match = (quantity - matched_quantity).min(available_quantity);
//                 matched_quantity += quantity_to_match;
//             }
//         }
//
//         matched_quantity
//     }
//
//     /// Internal matching function that handles both limit and market orders.
//     ///
//     /// This function iterates through the opposite side of the book, matching the incoming
//     /// order against resting orders as long as the price is compatible.
//     ///
//     /// # Arguments
//     /// * `order_id` - The ID of the incoming order to be matched.
//     /// * `side` - The side of the incoming order (Buy or Sell).
//     /// * `quantity` - The total quantity of the incoming order.
//     /// * `limit_price` - An optional limit price. If `None`, it's a market order. If `Some`, it's a limit order,
//     ///   and matching will stop if the market price is no longer favorable.
//     ///
//     /// # Returns
//     /// A `MatchResult` detailing the trades executed, any remaining quantity, and whether the order
//     /// was fully filled.
//     pub fn match_order(
//         &self,
//         order_id: OrderId,
//         side: Side,
//         quantity: u64,
//         limit_price: Option<u64>,
//     ) -> Result<MatchResult, OrderBookError> {
//         let mut match_result = MatchResult::new(order_id, quantity);
//         let mut remaining_quantity = quantity;
//         let mut filled_orders = Vec::new();
//
//         let match_side = match side {
//             Side::Buy => &self.asks,  // Match a buy order against asks
//             Side::Sell => &self.bids, // Match a sell order against bids
//         };
//
//         // Get a sorted list of prices to iterate through
//         let mut prices: Vec<u64> = match_side.iter().map(|item| *item.key()).collect();
//         if side == Side::Buy {
//             prices.sort_unstable(); // Ascending for asks
//         } else {
//             prices.sort_unstable_by(|a, b| b.cmp(a)); // Descending for bids
//         }
//
//         for price in prices {
//             // For limit orders, check if the market price is still valid
//             if let Some(limit) = limit_price {
//                 match side {
//                     Side::Buy if price > limit => break, // Ask price is higher than buy limit
//                     Side::Sell if price < limit => break, // Bid price is lower than sell limit
//                     _ => {}
//                 }
//             }
//
//             if let Some(mut price_level_entry) = match_side.get_mut(&price) {
//                 let price_level = &mut *price_level_entry;
//                 let price_level_match = price_level.match_order(
//                     remaining_quantity,
//                     order_id,
//                     &self.transaction_id_generator,
//                 );
//
//                 if !price_level_match.transactions.as_vec().is_empty() {
//                     self.last_trade_price
//                         .store(price, std::sync::atomic::Ordering::SeqCst);
//                     self.has_traded
//                         .store(true, std::sync::atomic::Ordering::SeqCst);
//                 }
//
//                 for transaction in price_level_match.transactions.as_vec() {
//                     match_result.add_transaction(*transaction);
//                 }
//                 for filled_order_id in &price_level_match.filled_order_ids {
//                     match_result.add_filled_order_id(*filled_order_id);
//                     filled_orders.push(*filled_order_id);
//                 }
//
//                 remaining_quantity = price_level_match.remaining_quantity;
//
//                 if price_level.order_count() == 0 {
//                     // Must drop the mutable reference before removing from the DashMap
//                     drop(price_level_entry);
//                     match_side.remove(&price);
//                 }
//
//                 if remaining_quantity == 0 {
//                     break; // Order fully matched
//                 }
//             } else {
//                 // Price level was removed by another thread, continue to the next
//                 continue;
//             }
//         }
//
//         // Remove all filled orders from tracking
//         for order_id in filled_orders {
//             self.order_locations.remove(&order_id);
//         }
//
//         // If a market order (no limit price) was not filled at all, return an error.
//         if limit_price.is_none() && remaining_quantity == quantity {
//             return Err(OrderBookError::InsufficientLiquidity {
//                 side,
//                 requested: quantity,
//                 available: 0,
//             });
//         }
//
//         match_result.remaining_quantity = remaining_quantity;
//         match_result.is_complete = remaining_quantity == 0;
//
//         Ok(match_result)
//     }
// }
