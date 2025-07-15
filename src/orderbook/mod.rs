//! OrderBook implementation for managing multiple price levels and order matching.

pub mod book;
pub mod error;
pub mod matching;

pub mod modifications;
pub mod operations;
mod private;
pub mod snapshot;
mod tests;

pub use book::OrderBook;
pub use error::OrderBookError;
pub use snapshot::OrderBookSnapshot;
