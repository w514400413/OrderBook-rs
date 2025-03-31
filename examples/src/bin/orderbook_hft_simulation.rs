use orderbook_rs::OrderBook;
use pricelevel::{OrderId, Side, TimeInForce, setup_logger};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};
use tracing::info;
use uuid::Uuid;

// Simulation parameters
const SYMBOL: &str = "BTC/USD";
const SIMULATION_DURATION_MS: u64 = 5000; // 5 seconds
const MAKER_THREAD_COUNT: usize = 10;
const TAKER_THREAD_COUNT: usize = 10;
const CANCELLER_THREAD_COUNT: usize = 10;
const TOTAL_THREAD_COUNT: usize = MAKER_THREAD_COUNT + TAKER_THREAD_COUNT + CANCELLER_THREAD_COUNT;

// Price levels for simulation
const BASE_BID_PRICE: u64 = 9900;
const BASE_ASK_PRICE: u64 = 10000;
const PRICE_LEVELS: u64 = 20;

fn main() {
    // Set up logging
    setup_logger();
    info!("OrderBook High-Frequency Trading Simulation");
    info!("===========================================");
    info!("Symbol: {}", SYMBOL);
    info!("Duration: {} ms", SIMULATION_DURATION_MS);
    info!(
        "Threads: {} (Makers: {}, Takers: {}, Cancellers: {})",
        TOTAL_THREAD_COUNT, MAKER_THREAD_COUNT, TAKER_THREAD_COUNT, CANCELLER_THREAD_COUNT
    );

    // Create a shared order book
    let order_book = Arc::new(OrderBook::new(SYMBOL));

    // Counters for operations
    let orders_added = Arc::new(AtomicU64::new(0));
    let orders_matched = Arc::new(AtomicU64::new(0));
    let orders_cancelled = Arc::new(AtomicU64::new(0));

    // Flag to signal when to stop the simulation
    let running = Arc::new(AtomicBool::new(true));

    // Barrier to synchronize thread start
    let barrier = Arc::new(Barrier::new(TOTAL_THREAD_COUNT + 1)); // +1 for main thread

    // Pre-populate the order book with initial orders
    info!("Pre-populating order book with initial orders...");
    preload_order_book(&order_book, 1000);

    // Print initial state
    info!("\nInitial OrderBook State:");
    print_order_book_state(&order_book);

    // Spawn threads
    let mut handles = Vec::with_capacity(TOTAL_THREAD_COUNT);

    // Spawn maker threads
    for i in 0..MAKER_THREAD_COUNT {
        spawn_maker_thread(
            i,
            &mut handles,
            Arc::clone(&order_book),
            Arc::clone(&orders_added),
            Arc::clone(&barrier),
            Arc::clone(&running),
        );
    }

    // Spawn taker threads
    for i in 0..TAKER_THREAD_COUNT {
        spawn_taker_thread(
            MAKER_THREAD_COUNT + i,
            &mut handles,
            Arc::clone(&order_book),
            Arc::clone(&orders_matched),
            Arc::clone(&barrier),
            Arc::clone(&running),
        );
    }

    // Spawn canceller threads
    for i in 0..CANCELLER_THREAD_COUNT {
        spawn_canceller_thread(
            MAKER_THREAD_COUNT + TAKER_THREAD_COUNT + i,
            &mut handles,
            Arc::clone(&order_book),
            Arc::clone(&orders_cancelled),
            Arc::clone(&barrier),
            Arc::clone(&running),
        );
    }

    // Start the simulation
    info!(
        "\nStarting HFT simulation for {} ms...",
        SIMULATION_DURATION_MS
    );
    let start_time = Instant::now();

    // Start all threads simultaneously
    barrier.wait();

    // Run for the specified duration
    thread::sleep(Duration::from_millis(SIMULATION_DURATION_MS));

    // Signal threads to stop
    running.store(false, Ordering::Relaxed);
    info!("Stopping simulation...");

    // Wait for all threads to finish
    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start_time.elapsed();
    info!("Simulation completed in {:?}", elapsed);

    // Calculate statistics
    let total_added = orders_added.load(Ordering::Relaxed);
    let total_matched = orders_matched.load(Ordering::Relaxed);
    let total_cancelled = orders_cancelled.load(Ordering::Relaxed);
    let total_operations = total_added + total_matched + total_cancelled;

    let elapsed_seconds = elapsed.as_secs_f64();
    let operations_per_second = total_operations as f64 / elapsed_seconds;

    // Print performance statistics
    info!("\nPerformance Statistics:");
    info!("======================");
    info!(
        "Orders Added: {} ({:.2} per second)",
        total_added,
        total_added as f64 / elapsed_seconds
    );
    info!(
        "Orders Matched: {} ({:.2} per second)",
        total_matched,
        total_matched as f64 / elapsed_seconds
    );
    info!(
        "Orders Cancelled: {} ({:.2} per second)",
        total_cancelled,
        total_cancelled as f64 / elapsed_seconds
    );
    info!(
        "Total Operations: {} ({:.2} per second)",
        total_operations, operations_per_second
    );

    // Print final order book state
    info!("\nFinal OrderBook State:");
    print_order_book_state(&order_book);
}

fn preload_order_book(order_book: &OrderBook, count: usize) {
    // Add limit buy orders at different price levels
    for i in 0..(count / 2) {
        let price_level = i % PRICE_LEVELS as usize;
        let price = BASE_BID_PRICE - price_level as u64 * 10; // Decreasing prices for bids

        let id = OrderId(Uuid::new_v4());
        let quantity = 10 + (i % 10) as u64; // 10-19 units

        let _ = order_book.add_limit_order(id, price, quantity, Side::Buy, TimeInForce::Gtc);
    }

    // Add limit sell orders at different price levels
    for i in 0..(count / 2) {
        let price_level = i % PRICE_LEVELS as usize;
        let price = BASE_ASK_PRICE + price_level as u64 * 10; // Increasing prices for asks

        let id = OrderId(Uuid::new_v4());
        let quantity = 10 + (i % 10) as u64; // 10-19 units

        let _ = order_book.add_limit_order(id, price, quantity, Side::Sell, TimeInForce::Gtc);
    }

    // Add a few iceberg orders
    for i in 0..20 {
        let is_buy = i % 2 == 0;
        let side = if is_buy { Side::Buy } else { Side::Sell };
        let price = if is_buy {
            BASE_BID_PRICE - 5
        } else {
            BASE_ASK_PRICE + 5
        };

        let id = OrderId(Uuid::new_v4());
        let _ = order_book.add_iceberg_order(id, price, 5, 45, side, TimeInForce::Gtc);
    }
}

fn print_order_book_state(order_book: &OrderBook) {
    // Best prices
    match (order_book.best_bid(), order_book.best_ask()) {
        (Some(bid), Some(ask)) => {
            info!("Best Bid: {}", bid);
            info!("Best Ask: {}", ask);
            info!("Spread: {}", ask - bid);
            if let Some(mid) = order_book.mid_price() {
                info!("Mid Price: {:.2}", mid);
            }
        }
        (Some(bid), None) => {
            info!("Best Bid: {}", bid);
            info!("No asks in book");
        }
        (None, Some(ask)) => {
            info!("No bids in book");
            info!("Best Ask: {}", ask);
        }
        (None, None) => {
            info!("Order book is empty");
            return;
        }
    }

    // Order counts and volumes
    let all_orders = order_book.get_all_orders();
    info!("Total Orders: {}", all_orders.len());

    let (bid_volumes, ask_volumes) = order_book.get_volume_by_price();
    info!("Bid Price Levels: {}", bid_volumes.len());
    info!("Ask Price Levels: {}", ask_volumes.len());

    // Calculate total visible and hidden quantities
    let mut total_bid_visible = 0;
    let mut total_bid_hidden = 0;
    let mut total_ask_visible = 0;
    let mut total_ask_hidden = 0;

    let snapshot = order_book.create_snapshot(100); // Get a deep snapshot

    for level in &snapshot.bids {
        total_bid_visible += level.visible_quantity;
        total_bid_hidden += level.hidden_quantity;
    }

    for level in &snapshot.asks {
        total_ask_visible += level.visible_quantity;
        total_ask_hidden += level.hidden_quantity;
    }

    info!(
        "Total Bid Quantity: {} (Visible: {}, Hidden: {})",
        total_bid_visible + total_bid_hidden,
        total_bid_visible,
        total_bid_hidden
    );
    info!(
        "Total Ask Quantity: {} (Visible: {}, Hidden: {})",
        total_ask_visible + total_ask_hidden,
        total_ask_visible,
        total_ask_hidden
    );

    // Last trade price
    if let Some(price) = order_book.last_trade_price() {
        info!("Last Trade Price: {}", price);
    }

    // Print top levels from each side
    info!("\nTop 5 Bid Levels:");
    for (i, level) in snapshot.bids.iter().take(5).enumerate() {
        info!(
            "  [{}] Price: {}, Quantity: {} (Visible: {}, Hidden: {}), Orders: {}",
            i + 1,
            level.price,
            level.visible_quantity + level.hidden_quantity,
            level.visible_quantity,
            level.hidden_quantity,
            level.order_count
        );
    }

    info!("\nTop 5 Ask Levels:");
    for (i, level) in snapshot.asks.iter().take(5).enumerate() {
        info!(
            "  [{}] Price: {}, Quantity: {} (Visible: {}, Hidden: {}), Orders: {}",
            i + 1,
            level.price,
            level.visible_quantity + level.hidden_quantity,
            level.visible_quantity,
            level.hidden_quantity,
            level.order_count
        );
    }
}

fn spawn_maker_thread(
    thread_id: usize,
    handles: &mut Vec<thread::JoinHandle<()>>,
    order_book: Arc<OrderBook>,
    counter: Arc<AtomicU64>,
    barrier: Arc<Barrier>,
    running: Arc<AtomicBool>,
) {
    let handle = thread::spawn(move || {
        barrier.wait(); // Wait for start signal

        let mut local_count = 0;

        while running.load(Ordering::Relaxed) {
            // Randomly choose between buy and sell
            let is_buy = local_count % 2 == 0;
            let side = if is_buy { Side::Buy } else { Side::Sell };

            // Choose a price level within a range around the best prices
            let price_base = if is_buy {
                BASE_BID_PRICE
            } else {
                BASE_ASK_PRICE
            };
            let price_offset = (local_count % PRICE_LEVELS as u64) * 10;
            let price = if is_buy {
                price_base - price_offset
            } else {
                price_base + price_offset
            };

            // Generate a random quantity
            let quantity = 5 + (local_count % 20); // 5-24 units

            // Choose order type based on iteration
            match local_count % 5 {
                0 => {
                    // Standard limit order
                    let id = OrderId(Uuid::new_v4());
                    let _ = order_book.add_limit_order(id, price, quantity, side, TimeInForce::Gtc);
                }
                1 => {
                    // Post-only order
                    let id = OrderId(Uuid::new_v4());
                    let _ =
                        order_book.add_post_only_order(id, price, quantity, side, TimeInForce::Gtc);
                }
                2 => {
                    // Iceberg order
                    let id = OrderId(Uuid::new_v4());
                    let _ = order_book.add_iceberg_order(
                        id,
                        price,
                        quantity / 4,
                        quantity * 3 / 4,
                        side,
                        TimeInForce::Gtc,
                    );
                }
                3 => {
                    // IOC order
                    let id = OrderId(Uuid::new_v4());
                    let cross_price = if is_buy {
                        BASE_ASK_PRICE + 10
                    } else {
                        BASE_BID_PRICE - 10
                    };
                    let _ = order_book.add_limit_order(
                        id,
                        cross_price,
                        quantity,
                        side,
                        TimeInForce::Ioc,
                    );
                }
                _ => {
                    // FOK order
                    let id = OrderId(Uuid::new_v4());
                    let cross_price = if is_buy {
                        BASE_ASK_PRICE + 5
                    } else {
                        BASE_BID_PRICE - 5
                    };
                    let _ = order_book.add_limit_order(
                        id,
                        cross_price,
                        quantity,
                        side,
                        TimeInForce::Fok,
                    );
                }
            }

            local_count += 1;

            // Update global counter periodically
            if local_count % 100 == 0 {
                counter.fetch_add(100, Ordering::Relaxed);
            }

            // Small delay to prevent CPU hogging
            thread::sleep(Duration::from_micros(50));
        }

        // Add any remaining count
        let remainder = local_count % 100;
        if remainder > 0 {
            counter.fetch_add(remainder, Ordering::Relaxed);
        }

        info!(
            "Maker thread {} completed with {} orders added",
            thread_id, local_count
        );
    });

    handles.push(handle);
}

fn spawn_taker_thread(
    thread_id: usize,
    handles: &mut Vec<thread::JoinHandle<()>>,
    order_book: Arc<OrderBook>,
    counter: Arc<AtomicU64>,
    barrier: Arc<Barrier>,
    running: Arc<AtomicBool>,
) {
    let handle = thread::spawn(move || {
        barrier.wait(); // Wait for start signal

        let mut local_count = 0;

        while running.load(Ordering::Relaxed) {
            // Randomly choose between buy and sell
            let is_buy = local_count % 2 == 0;
            let side = if is_buy { Side::Buy } else { Side::Sell };

            // Choose a quantity
            let quantity = 1 + (local_count % 10); // 1-10 units

            // Submit a market order
            let id = OrderId(Uuid::new_v4());
            let result = order_book.submit_market_order(id, quantity, side);

            // Only count successful matches
            if let Ok(match_result) = result {
                if match_result.executed_quantity() > 0 {
                    local_count += 1;
                }
            }

            // Update global counter periodically
            if local_count % 50 == 0 {
                counter.fetch_add(50, Ordering::Relaxed);
            }

            // Small delay to prevent CPU hogging
            thread::sleep(Duration::from_micros(100));
        }

        // Add any remaining count
        let remainder = local_count % 50;
        if remainder > 0 {
            counter.fetch_add(remainder, Ordering::Relaxed);
        }

        info!(
            "Taker thread {} completed with {} market orders executed",
            thread_id, local_count
        );
    });

    handles.push(handle);
}

fn spawn_canceller_thread(
    thread_id: usize,
    handles: &mut Vec<thread::JoinHandle<()>>,
    order_book: Arc<OrderBook>,
    counter: Arc<AtomicU64>,
    barrier: Arc<Barrier>,
    running: Arc<AtomicBool>,
) {
    let handle = thread::spawn(move || {
        barrier.wait(); // Wait for start signal

        let mut local_count = 0;

        while running.load(Ordering::Relaxed) {
            // Generate a somewhat random order ID
            // We'll use a mix of thread ID, iteration count, and current time
            // to increase the chance of hitting real orders sometimes
            let _rand_component = Instant::now().elapsed().as_nanos() as u64 % 1000;
            let id = OrderId(Uuid::new_v4());

            // Try to cancel the order
            let result = order_book.cancel_order(id);

            // Only count successful cancellations
            if let Ok(Some(_)) = result {
                local_count += 1;
            }

            // Update global counter periodically
            if local_count % 20 == 0 {
                counter.fetch_add(20, Ordering::Relaxed);
            }

            // Small delay to prevent CPU hogging
            thread::sleep(Duration::from_micros(200));
        }

        // Add any remaining count
        let remainder = local_count % 20;
        if remainder > 0 {
            counter.fetch_add(remainder, Ordering::Relaxed);
        }

        info!(
            "Canceller thread {} completed with {} orders cancelled",
            thread_id, local_count
        );
    });

    handles.push(handle);
}
