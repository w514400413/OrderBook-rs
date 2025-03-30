//! Order book snapshot for market data

use pricelevel::PriceLevelSnapshot;
use serde::{Deserialize, Serialize};
use tracing::trace;

/// A snapshot of the order book state at a specific point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    /// The symbol or identifier for this order book
    pub symbol: String,

    /// Timestamp when the snapshot was created (milliseconds since epoch)
    pub timestamp: u64,

    /// Snapshot of bid price levels
    pub bids: Vec<PriceLevelSnapshot>,

    /// Snapshot of ask price levels
    pub asks: Vec<PriceLevelSnapshot>,
}

impl OrderBookSnapshot {
    /// Get the best bid price and quantity
    pub fn best_bid(&self) -> Option<(u64, u64)> {
        let bids = self
            .bids
            .first()
            .map(|level| (level.price, level.visible_quantity));
        trace!("best_bid: {:?}", bids);
        bids
    }

    /// Get the best ask price and quantity
    pub fn best_ask(&self) -> Option<(u64, u64)> {
        let ask = self
            .asks
            .first()
            .map(|level| (level.price, level.visible_quantity));
        trace!("best_ask: {:?}", ask);
        ask
    }

    /// Get the mid price (average of best bid and best ask)
    pub fn mid_price(&self) -> Option<f64> {
        let mid_price = match (self.best_bid(), self.best_ask()) {
            (Some((bid_price, _)), Some((ask_price, _))) => {
                Some((bid_price as f64 + ask_price as f64) / 2.0)
            }
            _ => None,
        };
        trace!("mid_price: {:?}", mid_price);
        mid_price
    }

    /// Get the spread (best ask - best bid)
    pub fn spread(&self) -> Option<u64> {
        let spread = match (self.best_bid(), self.best_ask()) {
            (Some((bid_price, _)), Some((ask_price, _))) => {
                Some(ask_price.saturating_sub(bid_price))
            }
            _ => None,
        };
        trace!("spread: {:?}", spread);
        spread
    }

    /// Calculate the total volume on the bid side
    pub fn total_bid_volume(&self) -> u64 {
        let volume = self.bids.iter().map(|level| level.total_quantity()).sum();
        trace!("total_bid_volume: {:?}", volume);
        volume
    }

    /// Calculate the total volume on the ask side
    pub fn total_ask_volume(&self) -> u64 {
        let volume = self.asks.iter().map(|level| level.total_quantity()).sum();
        trace!("total_ask_volume: {:?}", volume);
        volume
    }

    /// Calculate the total value on the bid side (price * quantity)
    pub fn total_bid_value(&self) -> u64 {
        let value = self
            .bids
            .iter()
            .map(|level| level.price * level.total_quantity())
            .sum();
        trace!("total_bid_value: {:?}", value);
        value
    }

    /// Calculate the total value on the ask side (price * quantity)
    pub fn total_ask_value(&self) -> u64 {
        let value = self
            .asks
            .iter()
            .map(|level| level.price * level.total_quantity())
            .sum();
        trace!("total_ask_value: {:?}", value);
        value
    }
}
