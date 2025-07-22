//! Core OrderBook implementation for managing price levels and orders

use super::cache::PriceLevelCache;
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
    pub(super) transaction_id_generator: UuidGenerator,

    /// The last price at which a trade occurred
    pub(super) last_trade_price: AtomicU64,

    /// Flag indicating if there was a trade
    pub(super) has_traded: AtomicBool,

    /// The timestamp of market close, if applicable (for DAY orders)
    pub(super) market_close_timestamp: AtomicU64,

    /// Flag indicating if market close is set
    pub(super) has_market_close: AtomicBool,

    /// A cache for storing best bid/ask prices to avoid recalculation
    pub(super) cache: PriceLevelCache,

    /// listens to possible trades when an order is added
    pub trade_listener: Option<TradeListener>,
}

/// trade listener specification
pub type TradeListener = fn(&MatchResult);

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
            cache: PriceLevelCache::new(),
            trade_listener: None,
        }
    }

    /// Create a new order book for the given symbol with a trade listner
    pub fn with_trade_listener(symbol: &str, trade_listener: TradeListener) -> Self {
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
            cache: PriceLevelCache::new(),
            trade_listener: Some(trade_listener),
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
        if let Some(cached_bid) = self.cache.get_cached_best_bid() {
            return Some(cached_bid);
        }

        let best_price = self.bids.iter().map(|item| *item.key()).max();

        self.cache.update_best_prices(best_price, None);

        best_price
    }

    /// Get the best ask price, if any
    pub fn best_ask(&self) -> Option<u64> {
        if let Some(cached_ask) = self.cache.get_cached_best_ask() {
            return Some(cached_ask);
        }

        let best_price = self.asks.iter().map(|item| *item.key()).min();

        self.cache.update_best_prices(None, best_price);

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

    /// Get an order by its ID
    pub fn get_order(&self, order_id: OrderId) -> Option<Arc<OrderType>> {
        // Get the order location without locking
        if let Some(location) = self.order_locations.get(&order_id) {
            let (price, side) = *location;

            let price_levels = match side {
                Side::Buy => &self.bids,
                Side::Sell => &self.asks,
            };

            // Get the price level
            if let Some(price_level) = price_levels.get(&price) {
                // Iterate through the orders at this level to find the one with the matching ID
                for order in price_level.iter_orders() {
                    if order.id() == order_id {
                        return Some(order.clone());
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
        self.match_order(order_id, side, quantity, None)
    }

    /// Attempts to match a limit order in the order book.
    ///
    /// # Parameters
    /// - `order_id`: The unique identifier of the order to be matched.
    /// - `quantity`: The quantity of the order to be matched.
    /// - `side`: The side of the order book (e.g., Buy or Sell) on which the order resides.
    /// - `limit_price`: The maximum (for Buy) or minimum (for Sell) acceptable price
    ///   for the order.
    ///
    /// # Returns
    /// - `Ok(MatchResult)`: If the order is successfully matched, returning information
    ///   about the match, including possibly filled quantities and pricing details.
    /// - `Err(OrderBookError)`: If the order cannot be matched due to an error, such as
    ///   invalid parameters or an existing order book issue.
    ///
    /// # Behavior
    /// - Logs a trace message with details about the order and its intended match parameters.
    /// - Internally delegates to the `match_order` function, passing the provided parameters,
    ///   including the optional `limit_price` which specifies the price constraint.
    ///
    /// # Errors
    /// This function returns an error in cases such as:
    /// - The specified `order_id` is not found in the order book.
    /// - The provided parameters are invalid (e.g., negative quantity).
    /// - The attempted match is not feasible within the order book's current state.
    ///
    /// # Notes
    /// - The `limit_price` parameter sets a constraint on the match price:
    ///   - For Buy orders, it specifies the maximum acceptable price.
    ///   - For Sell orders, it specifies the minimum acceptable price.
    /// - If `limit_price` is not met during the matching process, the order will not be executed.
    pub fn match_limit_order(
        &self,
        order_id: OrderId,
        quantity: u64,
        side: Side,
        limit_price: u64,
    ) -> Result<MatchResult, OrderBookError> {
        trace!(
            "Order book {}: Matching limit order {} for {} at side {:?} with limit price {}",
            self.symbol, order_id, quantity, side, limit_price
        );
        self.match_order(order_id, side, quantity, Some(limit_price))
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
