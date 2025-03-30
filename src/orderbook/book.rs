//! Core OrderBook implementation for managing price levels and orders

use super::error::OrderBookError;
use super::snapshot::OrderBookSnapshot;
use crate::utils::current_time_millis;
use dashmap::DashMap;
use pricelevel::{MatchResult, OrderId, OrderType, PriceLevel, Side, UuidGenerator};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tracing::trace;
use uuid::Uuid;

/// The OrderBook manages a collection of price levels for both bid and ask sides.
/// It supports adding, cancelling, and matching orders with lock-free operations where possible.
pub struct OrderBook {
    /// The symbol or identifier for this order book
    pub(super) symbol: String,

    /// Bid side price levels (buy orders), stored in a concurrent map for lock-free access
    /// The map is keyed by price levels and stores Arc references to PriceLevel instances
    pub(super) bids: DashMap<u64, Arc<PriceLevel>>,

    /// Ask side price levels (sell orders), stored in a concurrent map for lock-free access
    /// The map is keyed by price levels and stores Arc references to PriceLevel instances
    pub(super) asks: DashMap<u64, Arc<PriceLevel>>,

    /// A concurrent map from order ID to (price, side) for fast lookups
    /// This avoids having to search through all price levels to find an order
    pub(super) order_locations: DashMap<OrderId, (u64, Side)>,

    /// Generator for unique transaction IDs
    transaction_id_generator: UuidGenerator,

    /// The last price at which a trade occurred
    pub(super) last_trade_price: AtomicU64,

    /// Flag indicating if there was a trade
    pub(super) has_traded: AtomicBool,

    /// The timestamp of market close, if applicable (for DAY orders)
    pub(super) market_close_timestamp: AtomicU64,

    /// Flag indicating if market close is set
    pub(super) has_market_close: AtomicBool,
}

impl OrderBook {
    /// Create a new order book for the given symbol
    pub fn new(symbol: &str) -> Self {
        // Create a unique namespace for this order book's transaction IDs
        let namespace = Uuid::new_v4();

        Self {
            symbol: symbol.to_string(),
            bids: DashMap::new(),
            asks: DashMap::new(),
            order_locations: DashMap::new(),
            transaction_id_generator: UuidGenerator::new(namespace),
            last_trade_price: AtomicU64::new(0),
            has_traded: AtomicBool::new(false),
            market_close_timestamp: AtomicU64::new(0),
            has_market_close: AtomicBool::new(false),
        }
    }

    /// Get the symbol of this order book
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Set the market close timestamp for DAY orders
    pub fn set_market_close_timestamp(&self, timestamp: u64) {
        self.market_close_timestamp
            .store(timestamp, Ordering::SeqCst);
        self.has_market_close.store(true, Ordering::SeqCst);
        trace!(
            "Order book {}: Set market close timestamp to {}",
            self.symbol, timestamp
        );
    }

    /// Clear the market close timestamp
    pub fn clear_market_close_timestamp(&self) {
        self.has_market_close.store(false, Ordering::SeqCst);
    }

    /// Get the best bid price, if any
    pub fn best_bid(&self) -> Option<u64> {
        let mut best_price = None;

        // Find the highest price in bids
        for item in self.bids.iter() {
            let price = *item.key();
            if best_price.is_none() || price > best_price.unwrap() {
                best_price = Some(price);
            }
        }

        best_price
    }

    /// Get the best ask price, if any
    pub fn best_ask(&self) -> Option<u64> {
        let mut best_price = None;

        // Find the lowest price in asks
        for item in self.asks.iter() {
            let price = *item.key();
            if best_price.is_none() || price < best_price.unwrap() {
                best_price = Some(price);
            }
        }

        best_price
    }

    /// Get the mid price (average of best bid and best ask)
    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid as f64 + ask as f64) / 2.0),
            _ => None,
        }
    }

    /// Get the last trade price, if any
    pub fn last_trade_price(&self) -> Option<u64> {
        if self.has_traded.load(Ordering::Relaxed) {
            Some(self.last_trade_price.load(Ordering::Relaxed))
        } else {
            None
        }
    }

    /// Get the spread (best ask - best bid)
    pub fn spread(&self) -> Option<u64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.saturating_sub(bid)),
            _ => None,
        }
    }

    /// Get all orders at a specific price level
    pub fn get_orders_at_price(&self, price: u64, side: Side) -> Vec<Arc<OrderType>> {
        trace!(
            "Order book {}: Getting orders at price {} for side {:?}",
            self.symbol, price, side
        );
        let price_levels = match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };

        if let Some(price_level) = price_levels.get(&price) {
            price_level.iter_orders()
        } else {
            Vec::new()
        }
    }

    /// Get all orders in the book
    pub fn get_all_orders(&self) -> Vec<Arc<OrderType>> {
        trace!("Order book {}: Getting all orders", self.symbol);
        let mut result = Vec::new();

        // Get all bid orders
        for item in self.bids.iter() {
            let price_level = item.value();
            result.extend(price_level.iter_orders());
        }

        // Get all ask orders
        for item in self.asks.iter() {
            let price_level = item.value();
            result.extend(price_level.iter_orders());
        }

        result
    }

    /// Get an order by ID
    pub fn get_order(&self, order_id: OrderId) -> Option<Arc<OrderType>> {
        // Check if we know where this order is
        if let Some(location) = self.order_locations.get(&order_id) {
            let (price, side) = *location;

            // Get appropriate price level
            let price_levels = match side {
                Side::Buy => &self.bids,
                Side::Sell => &self.asks,
            };

            if let Some(price_level) = price_levels.get(&price) {
                // Look through orders at this price level
                for order in price_level.iter_orders() {
                    if order.id() == order_id {
                        return Some(order);
                    }
                }
            }
        }

        None
    }

    /// Match a market order against the book
    pub fn match_market_order(
        &self,
        order_id: OrderId,
        quantity: u64,
        side: Side,
    ) -> Result<MatchResult, OrderBookError> {
        trace!(
            "Order book {}: Matching market order {} for {} at side {:?}",
            self.symbol, order_id, quantity, side
        );
        // Determine which side of the book to match against
        let opposite_side = side.opposite();
        let match_side = match opposite_side {
            Side::Buy => &self.bids,  // Market sell matches against bids
            Side::Sell => &self.asks, // Market buy matches against asks
        };

        let mut remaining_quantity = quantity;
        let mut match_result = MatchResult::new(order_id, quantity);
        let mut filled_orders = Vec::new();

        // Keep matching until we've filled the order or run out of liquidity
        while remaining_quantity > 0 {
            // Get the best price to match against
            let best_price = match opposite_side {
                Side::Buy => self.best_bid(), // For sell orders, match against highest bid
                Side::Sell => self.best_ask(), // For buy orders, match against lowest ask
            };

            if let Some(price) = best_price {
                // Match against this price level
                if let Some(entry) = match_side.get_mut(&price) {
                    let price_level = entry.value();

                    let price_level_match = price_level.match_order(
                        remaining_quantity,
                        order_id,
                        &self.transaction_id_generator,
                    );

                    // Update last trade price if we had matches
                    if !price_level_match.transactions.is_empty() {
                        self.last_trade_price.store(price, Ordering::SeqCst);
                        self.has_traded.store(true, Ordering::SeqCst);
                    }

                    // Update the match result with transactions
                    for transaction in price_level_match.transactions.as_vec() {
                        match_result.add_transaction(*transaction);
                    }

                    // Track filled orders to remove from tracking later
                    for filled_order_id in &price_level_match.filled_order_ids {
                        match_result.add_filled_order_id(*filled_order_id);
                        filled_orders.push(*filled_order_id);
                    }

                    // Update remaining quantity
                    remaining_quantity = price_level_match.remaining_quantity;

                    // If the price level is now empty, remove it
                    if price_level.order_count() == 0 {
                        // We must drop the mutable reference before removing
                        drop(entry);
                        match_side.remove(&price);
                    }

                    if remaining_quantity == 0 {
                        break; // Order fully matched
                    }
                } else {
                    // This shouldn't happen since we just got the price from the map
                    break;
                }
            } else {
                // No more price levels to match against
                break;
            }
        }

        // Remove all filled orders from tracking
        for order_id in filled_orders {
            self.order_locations.remove(&order_id);
        }

        // Update final match result
        match_result.remaining_quantity = remaining_quantity;
        match_result.is_complete = remaining_quantity == 0;

        if match_result.transactions.as_vec().is_empty() {
            // Order couldn't be matched at all
            Err(OrderBookError::InsufficientLiquidity {
                side,
                requested: quantity,
                available: 0,
            })
        } else {
            Ok(match_result)
        }
    }

    /// Create a snapshot of the current order book state
    pub fn create_snapshot(&self, depth: usize) -> OrderBookSnapshot {
        // Get all bid prices and sort them in descending order
        let mut bid_prices: Vec<u64> = self.bids.iter().map(|item| *item.key()).collect();
        bid_prices.sort_by(|a, b| b.cmp(a)); // Descending order
        bid_prices.truncate(depth);

        // Get all ask prices and sort them in ascending order
        let mut ask_prices: Vec<u64> = self.asks.iter().map(|item| *item.key()).collect();
        ask_prices.sort(); // Ascending order
        ask_prices.truncate(depth);

        let mut bid_levels = Vec::with_capacity(bid_prices.len());
        let mut ask_levels = Vec::with_capacity(ask_prices.len());

        // Create snapshots for each bid level
        for price in bid_prices {
            if let Some(price_level) = self.bids.get(&price) {
                bid_levels.push(price_level.snapshot());
            }
        }

        // Create snapshots for each ask level
        for price in ask_prices {
            if let Some(price_level) = self.asks.get(&price) {
                ask_levels.push(price_level.snapshot());
            }
        }

        OrderBookSnapshot {
            symbol: self.symbol.clone(),
            timestamp: current_time_millis(),
            bids: bid_levels,
            asks: ask_levels,
        }
    }

    /// Get the total volume at each price level
    pub fn get_volume_by_price(&self) -> (HashMap<u64, u64>, HashMap<u64, u64>) {
        let mut bid_volumes = HashMap::new();
        let mut ask_volumes = HashMap::new();

        // Calculate bid volumes
        for item in self.bids.iter() {
            let price = *item.key();
            let price_level = item.value();
            bid_volumes.insert(price, price_level.total_quantity());
        }

        // Calculate ask volumes
        for item in self.asks.iter() {
            let price = *item.key();
            let price_level = item.value();
            ask_volumes.insert(price, price_level.total_quantity());
        }

        (bid_volumes, ask_volumes)
    }
}
