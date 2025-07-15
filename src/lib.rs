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
//! - **Advanced Order Matching**: Efficient matching algorithm for both market and limit orders, correctly handling complex order types and partial fills.
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
//! ## What's New in Version 0.2.0
//!
//! This version introduces significant performance optimizations and architectural improvements:
//!
//! - **Performance Boost**: Reintroduced `PriceLevelCache` for faster best bid/ask lookups and a `MatchingPool` to reduce memory allocations in the matching engine, leading to lower latency.
//! - **Cleaner Architecture**: Refactored modification and matching logic for better separation of concerns and maintainability.
//! - **Enhanced Concurrency**: Improved thread-safe operations, ensuring robustness under heavy load.
//! - **Improved Documentation**: All code comments have been translated to English, and crate-level documentation has been expanded for clarity.
//!
//! ## Status
//! This project is currently in active development and is not yet suitable for production use.
//!
//! # Performance Analysis of the OrderBook System
//!
//! This analyzes the performance of the OrderBook system based on tests conducted on an Apple M4 Max processor. The data comes from two types of tests: a High-Frequency Trading (HFT) simulation and contention pattern tests.
//!
//! ## 1. High-Frequency Trading (HFT) Simulation
//!
//! ### Test Configuration
//! - **Symbol:** BTC/USD
//! - **Duration:** 5000 ms (5 seconds)
//! - **Threads:** 30 threads total
//!   - 10 maker threads (order creators)
//!   - 10 taker threads (order executors)
//!   - 10 canceller threads (order cancellers)
//! - **Initial orders:** 1020 pre-loaded orders
//!
//! ### Performance Results
//!
//! | Metric | Total Operations | Operations/Second |
//! |---------|---------------------|---------------------|
//! | Orders Added | 559,266 | 111,844.44 |
//! | Orders Matched | 330,638 | 66,122.42 |
//! | Orders Cancelled | 4,106,360 | 821,207.71 |
//! | **Total Operations** | **4,996,264** | **999,174.58** |
//!
//! ### Initial vs. Final OrderBook State
//!
//! | Metric | Initial State | Final State |
//! |---------|----------------|--------------|
//! | Best Bid | 9,900 | 9,880 |
//! | Best Ask | 10,000 | 10,050 |
//! | Spread | 100 | 170 |
//! | Mid Price | 9,950.00 | 9,965.00 |
//! | Total Orders | 1,020 | 138,295 |
//! | Bid Price Levels | 21 | 11 |
//! | Ask Price Levels | 21 | 11 |
//! | Total Bid Quantity | 7,750 | 1,037,923 |
//! | Total Ask Quantity | 7,750 | 1,488,201 |
//!
//! ## 2. Contention Pattern Tests
//!
//! ### Configuration
//! - **Threads:** 12
//! - **Duration per test:** 3000 ms (3 seconds)
//!
//! ### Read/Write Ratio Test
//!
//! | Read % | Operations/Second |
//! |------------|---------------------|
//! | 0% | 716,117.91 |
//! | 25% | 32,470.83 |
//! | 50% | 29,525.75 |
//! | 75% | 35,949.69 |
//! | 95% | 73,484.17 |
//!
//! ### Hot Spot Contention Test
//!
//! | % Operations on Hot Spot | Operations/Second |
//! |----------------------------------|---------------------|
//! | 0% | 8,166,484.48 |
//! | 25% | 10,277,423.77 |
//! | 50% | 13,767,842.77 |
//! | 75% | 19,322,454.84 |
//! | 100% | 28,327,212.19 |
//!
//! ## 3. Analysis and Conclusions
//!
//! ### Overall Performance
//! The system demonstrates an impressive capability to handle nearly **1 million operations per second** in the high-frequency trading simulation, distributed across order creations, matches, and cancellations.
//!
//! ### Read/Write Behavior
//! - **Notable observation:** Performance is highest with 0% and 95% read operations, showing a U-shaped curve.
//! - Pure write operations (0% reads) are extremely fast (716,117 ops/s).
//! - Performance significantly improves when most operations are reads (95% reads = 73,484 ops/s).
//! - Performance is lowest in the middle range (50% reads = 29,525 ops/s), indicating that the mix of reads and writes creates more contention.
//!
//! ### Hot Spot Contention
//! - Surprisingly, performance **increases** as more operations concentrate on a hot spot, reaching its maximum with 100% concentration (28,327,212 ops/s).
//! - This counter-intuitive behavior might indicate:
//!   1. Very efficient cache effects when operations are concentrated in one memory area
//!   2. Internal optimizations to handle high-contention cases
//!   3. Benefits of the system's lock-free architecture
//!
//! ### OrderBook State Behavior
//! - During the HFT simulation, the order book handled a massive increase in order volume (from 1,020 to 87,155).
//! - The spread increased from 100 to 270, reflecting realistic market behavior under pressure.
//! - The concentration of orders changed significantly, with fewer price levels but higher volume at each level.
//!
//! ## 4. Practical Implications
//!
//! - The system is suitable for high-frequency trading environments with the capacity to process nearly 1 million operations per second.
//! - The lock-free architecture proves to be extremely effective at handling contention, especially at hot spots.
//! - Optimal performance is achieved when the workload is dominated by a single type of operation (mostly reads or mostly writes).
//! - For real-world use cases, it would be advisable to design the workload distribution to avoid intermediate read/write ratios (25-75%), which show the lowest performance.
//!
//! This analysis confirms that the system design is highly scalable and appropriate for demanding financial applications requiring high-speed processing with data consistency.

pub mod orderbook;

mod utils;

pub use orderbook::{OrderBook, OrderBookError, OrderBookSnapshot};
pub use utils::current_time_millis;
