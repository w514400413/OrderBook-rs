use criterion::{Criterion, black_box, criterion_group, criterion_main};
use orderbook_rs::orderbook::book::OrderBook;
use pricelevel::{OrderId, OrderType, Side, TimeInForce};

/// Sets up a deep order book for benchmarking.
/// Populates the book with a significant number of orders on the ask side.
fn setup_deep_book() -> OrderBook {
    let book = OrderBook::new("BENCH_SYMBOL");
    // Create 100 price levels, from 10001 to 10100
    for i in 0..100 {
        let price = 10001 + i;
        // Add 10 orders at each price level
        for _ in 0..10 {
            let order = OrderType::Standard {
                id: OrderId::new(),
                side: Side::Sell,
                price,
                quantity: 10,
                time_in_force: TimeInForce::Gtc,
                timestamp: 0,
            };
            book.add_order(order).unwrap();
        }
    }
    book
}

/// Benchmark for matching a large market order that consumes a significant portion of the book.
fn match_order_benchmark(c: &mut Criterion) {
    let book = setup_deep_book();

    c.bench_function("match_order_deep_book", |b| {
        b.iter(|| {
            // The order to match. Its quantity (505) is chosen to match across
            // multiple price levels (50 levels + 5 from the 51st).
            let taker_order_id = OrderId::new();
            book.match_order(
                black_box(taker_order_id),
                black_box(Side::Buy),
                black_box(505),
                black_box(None), // Market order
            )
        })
    });
}

criterion_group!(benches, match_order_benchmark);
criterion_main!(benches);
