//! # High-Performance Lock-Free Order Book Engine
//!
//! A high-performance, thread-safe limit order book implementation written in Rust. This project provides a comprehensive order matching engine designed for low-latency trading systems, with a focus on concurrent access patterns and lock-free data structures.
//!
//! ## Key Features
//!
//! - **Lock-Free Architecture**: Built using atomics and lock-free data structures to minimize contention and maximize throughput in high-frequency trading scenarios.
//!
//! - **Multiple Order Types**: Support for various order types including standard limit orders, iceberg orders, post-only, fill-or-kill, immediate-or-cancel, good-till-date, trailing stop, pegged, market-to-limit, and reserve orders with custom replenishment logic.
//!
//! - **Thread-Safe Price Levels**: Each price level can be independently and concurrently modified by multiple threads without blocking.
//!
//! - **Advanced Order Matching**: Efficient matching algorithm that correctly handles complex order types and partial fills.
//!
//! - **Performance Metrics**: Built-in statistics tracking for benchmarking and monitoring system performance.
//!
//! - **Memory Efficient**: Designed to scale to millions of orders with minimal memory overhead.
//!
//! ## Design Goals
//!
//! This order book engine is built with the following design principles:
//!
//! 1. **Correctness**: Ensure that all operations maintain the integrity of the order book, even under high concurrency.
//! 2. **Performance**: Optimize for low latency and high throughput in both write-heavy and read-heavy workloads.
//! 3. **Scalability**: Support for millions of orders and thousands of price levels without degradation.
//! 4. **Flexibility**: Easily extendable to support additional order types and matching algorithms.
//!
//! ## Use Cases
//!
//! - **Trading Systems**: Core component for building trading systems and exchanges
//! - **Market Simulation**: Tool for back-testing trading strategies with realistic market dynamics
//! - **Research**: Platform for studying market microstructure and order flow
//! - **Educational**: Reference implementation for understanding modern exchange architecture
//!
//! ## Status
//!
//! This project is currently in active development and is not yet suitable for production use.

mod orderbook;

mod utils;

pub use orderbook::{OrderBook, OrderBookError, OrderBookSnapshot};
pub use utils::current_time_millis;
