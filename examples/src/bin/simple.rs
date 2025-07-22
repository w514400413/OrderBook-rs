use orderbook_rs;
use orderbook_rs::OrderBook;
use orderbook_rs::orderbook::modifications::OrderQuantity;
use pricelevel::{OrderId, Side, TimeInForce};
use uuid::Uuid;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let book = OrderBook::new("BTC-USDT");
    book.add_limit_order(
        OrderId::from_uuid(Uuid::new_v4()),
        100000,
        200000,
        Side::Buy,
        TimeInForce::Gtc,
    )
    .unwrap();
    book.add_limit_order(
        OrderId::from_uuid(Uuid::new_v4()),
        5000,
        3002,
        Side::Buy,
        TimeInForce::Gtc,
    )
    .unwrap();
    // Matches the first Buy order. No match results returned/reported and both orders removed from the order book
    let sell = book
        .add_limit_order(
            OrderId::from_uuid(Uuid::new_v4()),
            100000,
            200000,
            Side::Sell,
            TimeInForce::Gtc,
        )
        .unwrap();
    // attempt to look up the traded order
    let lookup_result = book.get_order(sell.id());
    println!("{sell}");
    match lookup_result {
        Some(order) => println!("{:?}", order),
        None => println!("Order not found"), // None, the order was traded and removed from book
    }
    // attempt to match the removed order
    let result = book
        .match_limit_order(sell.id(), sell.quantity(), sell.side(), sell.price())
        .unwrap(); // No error even when the order is not present in book
    println!("{:?}", result);
    println!("Order count: {}", book.get_all_orders().len());
    println!("Bids: {}", book.best_bid().unwrap());
    println!("Asks: {}", book.best_ask().unwrap_or_else(|| 0));
    println!(
        "Market price: {}",
        book.last_trade_price().unwrap_or_else(|| 0)
    );
    println!("Snapshot: {:?}", book.create_snapshot(10));
    Ok(())
}
