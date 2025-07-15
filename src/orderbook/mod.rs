//! OrderBook implementation for managing multiple price levels and order matching.

pub mod book;
mod error;
mod modifications;
mod operations;
mod private;
mod snapshot;
mod tests;

pub mod matching;

pub use book::OrderBook;
pub use error::OrderBookError;
pub use snapshot::OrderBookSnapshot;
