#[cfg(test)]
mod test_order_modifications {
    use crate::{OrderBook, OrderBookError};
    use pricelevel::{OrderId, OrderType, OrderUpdate, Side, TimeInForce};
    use uuid::Uuid;

    // Helper function to create a unique order ID
    fn create_order_id() -> OrderId {
        OrderId(Uuid::new_v4())
    }

    #[test]
    fn test_update_price_same_value() {
        let book = OrderBook::new("TEST");

        // Add a limit order
        let id = create_order_id();
        let price = 1000;
        let quantity = 10;
        let side = Side::Buy;

        let result = book.add_limit_order(id, price, quantity, side, TimeInForce::Gtc);
        assert!(result.is_ok());

        // Try to update to the same price
        let update = OrderUpdate::UpdatePrice {
            order_id: id,
            new_price: price,
        };

        let result = book.update_order(update);
        assert!(result.is_err());
        match result {
            Err(OrderBookError::InvalidOperation { message }) => {
                assert!(message.contains("Cannot update price to the same value"));
            }
            _ => panic!("Expected InvalidOperation error"),
        }
    }

    #[test]
    fn test_update_price_and_quantity() {
        let book = OrderBook::new("TEST");

        // Add a limit order
        let id = create_order_id();
        let price = 1000;
        let quantity = 10;
        let side = Side::Buy;

        let result = book.add_limit_order(id, price, quantity, side, TimeInForce::Gtc);
        assert!(result.is_ok());

        // Update price and quantity
        let new_price = 1100;
        let new_quantity = 15;
        let update = OrderUpdate::UpdatePriceAndQuantity {
            order_id: id,
            new_price,
            new_quantity,
        };

        let result = book.update_order(update);
        assert!(result.is_ok());

        // Verify the order was updated
        let updated_order = book.get_order(id);
        assert!(updated_order.is_some());
        let updated_order = updated_order.unwrap();
        assert_eq!(updated_order.price(), new_price);
        assert_eq!(updated_order.visible_quantity(), new_quantity);
    }

    #[test]
    fn test_cancel_nonexistent_order() {
        let book = OrderBook::new("TEST");

        // Try to cancel a non-existent order
        let id = create_order_id();
        let result = book.cancel_order(id);

        // Should return Ok(None)
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_update_order_cancel() {
        let book = OrderBook::new("TEST");

        // Add a limit order
        let id = create_order_id();
        let price = 1000;
        let quantity = 10;
        let side = Side::Buy;

        let result = book.add_limit_order(id, price, quantity, side, TimeInForce::Gtc);
        assert!(result.is_ok());

        // Cancel using the OrderUpdate enum
        let update = OrderUpdate::Cancel { order_id: id };

        let result = book.update_order(update);
        assert!(result.is_ok());

        // Order should be removed
        let order = book.get_order(id);
        assert!(order.is_none());
    }

    #[test]
    fn test_update_order_replace() {
        let book = OrderBook::new("TEST");

        // Add a limit order
        let id = create_order_id();
        let price = 1000;
        let quantity = 10;
        let side = Side::Buy;

        let result = book.add_limit_order(id, price, quantity, side, TimeInForce::Gtc);
        assert!(result.is_ok());

        // Replace the order
        let new_price = 1100;
        let new_quantity = 15;
        let update = OrderUpdate::Replace {
            order_id: id,
            price: new_price,
            quantity: new_quantity,
            side: Side::Buy,
        };

        let result = book.update_order(update);
        assert!(result.is_ok());

        // Verify the order was replaced
        let replaced_order = book.get_order(id);
        assert!(replaced_order.is_some());
        let replaced_order = replaced_order.unwrap();
        assert_eq!(replaced_order.price(), new_price);
        assert_eq!(replaced_order.visible_quantity(), new_quantity);
    }

    #[test]
    fn test_replace_with_different_side() {
        let book = OrderBook::new("TEST");

        // Add a limit order
        let id = create_order_id();
        let price = 1000;
        let quantity = 10;
        let side = Side::Buy;

        let result = book.add_limit_order(id, price, quantity, side, TimeInForce::Gtc);
        assert!(result.is_ok());

        // Replace with different side
        let update = OrderUpdate::Replace {
            order_id: id,
            price: 1100,
            quantity: 15,
            side: Side::Sell, // Different side
        };

        let result = book.update_order(update);
        assert!(result.is_ok());

        // Verify the order side was changed
        let replaced_order = book.get_order(id);
        assert!(replaced_order.is_some());
        assert_eq!(replaced_order.unwrap().side(), Side::Sell);
    }

    #[test]
    fn test_iceberg_order_update_quantity() {
        let book = OrderBook::new("TEST");

        // Add an iceberg order
        let id = create_order_id();
        let price = 1000;
        let visible = 10;
        let hidden = 90;
        let side = Side::Buy;

        let result = book.add_iceberg_order(id, price, visible, hidden, side, TimeInForce::Gtc);
        assert!(result.is_ok());

        // Update visible quantity
        let new_quantity = 15;
        let update = OrderUpdate::UpdateQuantity {
            order_id: id,
            new_quantity,
        };

        let result = book.update_order(update);
        assert!(result.is_ok());

        // Verify the order's visible quantity was updated
        let updated_order = book.get_order(id);
        assert!(updated_order.is_some());
        let updated_order = updated_order.unwrap();
        assert_eq!(updated_order.visible_quantity(), new_quantity);

        // Hidden quantity should remain the same
        match &*updated_order {
            OrderType::IcebergOrder {
                hidden_quantity, ..
            } => {
                assert_eq!(*hidden_quantity, hidden);
            }
            _ => panic!("Expected IcebergOrder"),
        }
    }
}

#[cfg(test)]
mod test_modifications_remaining {
    use crate::OrderBook;
    use pricelevel::{OrderId, OrderType, OrderUpdate, PegReferenceType, Side, TimeInForce};
    use uuid::Uuid;

    fn create_order_id() -> OrderId {
        OrderId(Uuid::new_v4())
    }

    #[test]
    fn test_update_price_error_cases() {
        let book = OrderBook::new("TEST");

        // Update a non-existent order
        let id = create_order_id();
        let update = OrderUpdate::UpdatePrice {
            order_id: id,
            new_price: 1000,
        };

        let result = book.update_order(update);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_update_price_and_quantity_nonexistent() {
        let book = OrderBook::new("TEST");

        // Update a non-existent order
        let id = create_order_id();
        let update = OrderUpdate::UpdatePriceAndQuantity {
            order_id: id,
            new_price: 1000,
            new_quantity: 10,
        };

        let result = book.update_order(update);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_update_order_with_all_types() {
        let book = OrderBook::new("TEST");

        // Add different order types

        // 1. Add a trailing stop order
        let id1 = create_order_id();
        let timestamp = crate::utils::current_time_millis();
        let trail_order = OrderType::TrailingStop {
            id: id1,
            price: 1000,
            quantity: 10,
            side: Side::Buy,
            timestamp,
            time_in_force: TimeInForce::Gtc,
            trail_amount: 5,
            last_reference_price: 995,
        };

        // 2. Add a pegged order
        let id2 = create_order_id();
        let peg_order = OrderType::PeggedOrder {
            id: id2,
            price: 1000,
            quantity: 10,
            side: Side::Buy,
            timestamp,
            time_in_force: TimeInForce::Gtc,
            reference_price_offset: 5,
            reference_price_type: PegReferenceType::BestBid,
        };

        // 3. Add a market to limit order
        let id3 = create_order_id();
        let mtl_order = OrderType::MarketToLimit {
            id: id3,
            price: 1000,
            quantity: 10,
            side: Side::Buy,
            timestamp,
            time_in_force: TimeInForce::Gtc,
        };

        // 4. Add a reserve order
        let id4 = create_order_id();
        let reserve_order = OrderType::ReserveOrder {
            id: id4,
            price: 1000,
            visible_quantity: 5,
            hidden_quantity: 5,
            side: Side::Buy,
            timestamp,
            time_in_force: TimeInForce::Gtc,
            replenish_threshold: 2,
            replenish_amount: Some(3),
            auto_replenish: true,
        };

        // Add all orders to the book
        let _ = book.add_order(trail_order);
        let _ = book.add_order(peg_order);
        let _ = book.add_order(mtl_order);
        let _ = book.add_order(reserve_order);

        // Test updating all order types

        // 1. Update trailing stop
        let update1 = OrderUpdate::UpdatePriceAndQuantity {
            order_id: id1,
            new_price: 1010,
            new_quantity: 15,
        };

        // 2. Update pegged order
        let update2 = OrderUpdate::UpdatePriceAndQuantity {
            order_id: id2,
            new_price: 1010,
            new_quantity: 15,
        };

        // 3. Update market to limit
        let update3 = OrderUpdate::UpdatePriceAndQuantity {
            order_id: id3,
            new_price: 1010,
            new_quantity: 15,
        };

        // 4. Update reserve order
        let update4 = OrderUpdate::UpdatePriceAndQuantity {
            order_id: id4,
            new_price: 1010,
            new_quantity: 15,
        };

        // Execute all updates
        let result1 = book.update_order(update1);
        let result2 = book.update_order(update2);
        let result3 = book.update_order(update3);
        let result4 = book.update_order(update4);

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert!(result3.is_ok());
        assert!(result4.is_ok());

        // Verify the orders were updated
        let order1 = book.get_order(id1);
        let order2 = book.get_order(id2);
        let order3 = book.get_order(id3);
        let order4 = book.get_order(id4);

        assert!(order1.is_some());
        assert!(order2.is_some());
        assert!(order3.is_some());
        assert!(order4.is_some());

        assert_eq!(order1.unwrap().price(), 1010);
        assert_eq!(order2.unwrap().price(), 1010);
        assert_eq!(order3.unwrap().price(), 1010);
        assert_eq!(order4.unwrap().price(), 1010);
    }

    #[test]
    fn test_replace_with_special_order_types() {
        let book = OrderBook::new("TEST");

        // Add a reserve order
        let id = create_order_id();
        let timestamp = crate::utils::current_time_millis();
        let reserve_order = OrderType::ReserveOrder {
            id,
            price: 1000,
            visible_quantity: 5,
            hidden_quantity: 5,
            side: Side::Buy,
            timestamp,
            time_in_force: TimeInForce::Gtc,
            replenish_threshold: 2,
            replenish_amount: Some(3),
            auto_replenish: true,
        };

        let _ = book.add_order(reserve_order);

        // Try to replace with an unsupported type via Replace operation
        let update = OrderUpdate::Replace {
            order_id: id,
            price: 1010,
            quantity: 15,
            side: Side::Buy,
        };

        let result = book.update_order(update);

        // Should succeed since we're replacing with a standard order
        assert!(result.is_ok());

        // Verify the order was updated
        let updated_order = book.get_order(id);
        assert!(updated_order.is_some());
        assert_eq!(updated_order.clone().unwrap().price(), 1010);
        assert_eq!(updated_order.unwrap().visible_quantity(), 15);
    }

    #[test]
    fn test_cancel_order_removes_price_level() {
        let book = OrderBook::new("TEST");

        // Add a limit order
        let id = create_order_id();
        let _ = book.add_limit_order(id, 1000, 10, Side::Buy, TimeInForce::Gtc);

        // Cancel the order
        let update = OrderUpdate::Cancel { order_id: id };

        let result = book.update_order(update);
        assert!(result.is_ok());

        // Price level should be removed
        assert_eq!(book.best_bid(), None);

        // Order should be removed from tracking
        assert!(book.get_order(id).is_none());
    }
}
