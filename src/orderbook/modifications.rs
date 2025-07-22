use crate::orderbook::book::OrderBook;
use crate::orderbook::error::OrderBookError;
use pricelevel::{OrderId, OrderType, OrderUpdate, PriceLevel, Side};
use std::sync::Arc;
use tracing::trace;

/// A trait to abstract quantity access and modification for different order types.
pub trait OrderQuantity {
    /// Returns the primary quantity used for display or simple matching.
    /// For iceberg orders, this is the visible quantity.
    fn quantity(&self) -> u64;

    /// Returns the total quantity of the order (e.g., visible + hidden).
    fn total_quantity(&self) -> u64;

    /// Sets the new quantity for an order, handling the logic for different types.
    /// For iceberg orders, it adjusts the visible and hidden parts correctly.
    fn set_quantity(&mut self, new_total_quantity: u64);
}

impl OrderQuantity for OrderType {
    fn quantity(&self) -> u64 {
        match self {
            OrderType::Standard { quantity, .. } => *quantity,
            OrderType::IcebergOrder {
                visible_quantity, ..
            } => *visible_quantity,
            OrderType::PostOnly { quantity, .. } => *quantity,
            OrderType::TrailingStop { quantity, .. } => *quantity,
            OrderType::PeggedOrder { quantity, .. } => *quantity,
            OrderType::MarketToLimit { quantity, .. } => *quantity,
            OrderType::ReserveOrder {
                visible_quantity, ..
            } => *visible_quantity,
        }
    }

    fn total_quantity(&self) -> u64 {
        match self {
            OrderType::Standard { quantity, .. } => *quantity,
            OrderType::IcebergOrder {
                visible_quantity,
                hidden_quantity,
                ..
            } => *visible_quantity + *hidden_quantity,
            OrderType::PostOnly { quantity, .. } => *quantity,
            OrderType::TrailingStop { quantity, .. } => *quantity,
            OrderType::PeggedOrder { quantity, .. } => *quantity,
            OrderType::MarketToLimit { quantity, .. } => *quantity,
            OrderType::ReserveOrder {
                visible_quantity,
                hidden_quantity,
                ..
            } => *visible_quantity + *hidden_quantity,
        }
    }

    fn set_quantity(&mut self, new_total_quantity: u64) {
        match self {
            OrderType::Standard { quantity, .. }
            | OrderType::PostOnly { quantity, .. }
            | OrderType::TrailingStop { quantity, .. }
            | OrderType::PeggedOrder { quantity, .. }
            | OrderType::MarketToLimit { quantity, .. } => *quantity = new_total_quantity,

            OrderType::IcebergOrder {
                visible_quantity,
                hidden_quantity,
                ..
            } => {
                let original_total = *visible_quantity + *hidden_quantity;
                let amount_to_reduce = original_total.saturating_sub(new_total_quantity);

                let filled_from_visible = amount_to_reduce.min(*visible_quantity);
                *visible_quantity -= filled_from_visible;

                let remaining_to_reduce = amount_to_reduce - filled_from_visible;
                *hidden_quantity = hidden_quantity.saturating_sub(remaining_to_reduce);
            }
            OrderType::ReserveOrder {
                visible_quantity,
                hidden_quantity,
                replenish_amount,
                ..
            } => {
                let original_total = *visible_quantity + *hidden_quantity;
                let amount_to_reduce = original_total.saturating_sub(new_total_quantity);

                let filled_from_visible = amount_to_reduce.min(*visible_quantity);
                *visible_quantity -= filled_from_visible;

                let remaining_to_reduce = amount_to_reduce - filled_from_visible;
                *hidden_quantity = hidden_quantity.saturating_sub(remaining_to_reduce);

                if *visible_quantity == 0 && *hidden_quantity > 0 {
                    let refresh = replenish_amount.unwrap_or(0).min(*hidden_quantity);
                    *visible_quantity = refresh;
                    *hidden_quantity -= refresh;
                }
            }
        }
    }
}

impl OrderBook {
    /// Update an order's price and/or quantity
    pub fn update_order(
        &self,
        update: OrderUpdate,
    ) -> Result<Option<Arc<OrderType>>, OrderBookError> {
        self.cache.invalidate();
        trace!("Order book {}: Updating order {:?}", self.symbol, update);
        match update {
            OrderUpdate::UpdatePrice {
                order_id,
                new_price,
            } => {
                // Get the order location without locking
                let location = self.order_locations.get(&order_id).map(|val| *val);

                if let Some((old_price, _)) = location {
                    // If price doesn't change, do nothing
                    if old_price == new_price {
                        return Err(OrderBookError::InvalidOperation {
                            message: "Cannot update price to the same value".to_string(),
                        });
                    }

                    // Get the original order without holding locks
                    let original_order = if let Some(order) = self.get_order(order_id) {
                        // Create a copy of the order
                        Arc::try_unwrap(order.clone()).unwrap_or_else(|arc| (*arc))
                    } else {
                        return Ok(None); // Order not found
                    };

                    // Cancel the original order
                    self.cancel_order(order_id)?;

                    // Create a new order with the updated price
                    let mut new_order = original_order;

                    // Update the price based on order type
                    match &mut new_order {
                        OrderType::Standard { price, .. } => *price = new_price,
                        OrderType::IcebergOrder { price, .. } => *price = new_price,
                        OrderType::PostOnly { price, .. } => *price = new_price,
                        OrderType::TrailingStop { price, .. } => *price = new_price,
                        OrderType::PeggedOrder { price, .. } => *price = new_price,
                        OrderType::MarketToLimit { price, .. } => *price = new_price,
                        OrderType::ReserveOrder { price, .. } => *price = new_price,
                    }

                    // Add the updated order
                    let result = self.add_order(new_order)?;
                    Ok(Some(result))
                } else {
                    Ok(None) // Order not found
                }
            }

            OrderUpdate::UpdateQuantity {
                order_id,
                new_quantity,
            } => {
                // Get order location without locking
                let location = self.order_locations.get(&order_id).map(|val| *val);

                if let Some((price, side)) = location {
                    // Get the appropriate price levels map
                    let price_levels = match side {
                        Side::Buy => &self.bids,
                        Side::Sell => &self.asks,
                    };

                    // Use entry() to safely modify the price level without deadlocks
                    let mut result = None;
                    let mut is_empty = false;

                    price_levels.entry(price).and_modify(|price_level| {
                        // Create update operation
                        let update = OrderUpdate::UpdateQuantity {
                            order_id,
                            new_quantity,
                        };

                        // Try to update the order
                        if let Ok(updated_order) = price_level.update_order(update) {
                            result = updated_order;
                            is_empty = price_level.order_count() == 0;
                        }
                        self.cache.invalidate();
                    });

                    // If the price level is now empty, remove it
                    if is_empty {
                        price_levels.remove(&price);
                        self.order_locations.remove(&order_id);
                    }

                    Ok(result)
                } else {
                    Ok(None) // Order not found
                }
            }

            OrderUpdate::UpdatePriceAndQuantity {
                order_id,
                new_price,
                new_quantity,
            } => {
                // Get order location without locking
                let location = self.order_locations.get(&order_id).map(|val| *val);

                if let Some((_, _)) = location {
                    // Get the original order without holding locks
                    let original_order = if let Some(order) = self.get_order(order_id) {
                        // Create a copy of the order
                        Arc::try_unwrap(order.clone()).unwrap_or_else(|arc| (*arc))
                    } else {
                        return Ok(None); // Order not found
                    };

                    // Cancel the original order
                    self.cancel_order(order_id)?;

                    // Create a new order with the updated price and quantity
                    let mut new_order = original_order;

                    // Update the price and quantity based on order type
                    match &mut new_order {
                        OrderType::Standard {
                            price, quantity, ..
                        } => {
                            *price = new_price;
                            *quantity = new_quantity;
                        }
                        OrderType::IcebergOrder {
                            price,
                            visible_quantity,
                            ..
                        } => {
                            *price = new_price;
                            *visible_quantity = new_quantity;
                        }
                        OrderType::PostOnly {
                            price, quantity, ..
                        } => {
                            *price = new_price;
                            *quantity = new_quantity;
                        }
                        OrderType::TrailingStop {
                            price, quantity, ..
                        } => {
                            *price = new_price;
                            *quantity = new_quantity;
                        }
                        OrderType::PeggedOrder {
                            price, quantity, ..
                        } => {
                            *price = new_price;
                            *quantity = new_quantity;
                        }
                        OrderType::MarketToLimit {
                            price, quantity, ..
                        } => {
                            *price = new_price;
                            *quantity = new_quantity;
                        }
                        OrderType::ReserveOrder {
                            price,
                            visible_quantity,
                            ..
                        } => {
                            *price = new_price;
                            *visible_quantity = new_quantity;
                        }
                    }

                    // Add the updated order
                    let result = self.add_order(new_order)?;
                    Ok(Some(result))
                } else {
                    Ok(None) // Order not found
                }
            }

            OrderUpdate::Cancel { order_id } => {
                // Get order location without locking
                let location = self.order_locations.get(&order_id).map(|val| *val);

                if let Some((price, side)) = location {
                    // Get the appropriate price levels map
                    let price_levels = match side {
                        Side::Buy => &self.bids,
                        Side::Sell => &self.asks,
                    };

                    // Use entry() to safely modify the price level
                    let mut result = None;
                    let mut is_empty = false;

                    price_levels.entry(price).and_modify(|price_level| {
                        // Create cancel operation
                        let update = OrderUpdate::Cancel { order_id };

                        // Try to cancel the order
                        if let Ok(cancelled_order) = price_level.update_order(update) {
                            result = cancelled_order;
                            is_empty = price_level.order_count() == 0;
                        }
                    });

                    // If we cancelled an order, remove it from tracking
                    if result.is_some() {
                        self.order_locations.remove(&order_id);

                        // If price level is empty, remove it
                        if is_empty {
                            price_levels.remove(&price);
                        }
                    }

                    Ok(result)
                } else {
                    Ok(None) // Order not found
                }
            }

            OrderUpdate::Replace {
                order_id,
                price,
                quantity,
                side,
            } => {
                // Get the original order without holding locks
                let original_opt = self.get_order(order_id);

                if let Some(original) = original_opt {
                    // Extract what we need from the original order
                    let timestamp = original.timestamp();
                    let time_in_force = original.time_in_force();

                    // Check which order type we need to create
                    let new_order = match &*original {
                        OrderType::Standard { .. } => OrderType::Standard {
                            id: order_id,
                            price,
                            quantity,
                            side,
                            timestamp,
                            time_in_force,
                        },
                        OrderType::IcebergOrder {
                            hidden_quantity, ..
                        } => OrderType::IcebergOrder {
                            id: order_id,
                            price,
                            visible_quantity: quantity,
                            hidden_quantity: *hidden_quantity,
                            side,
                            timestamp,
                            time_in_force,
                        },
                        OrderType::PostOnly { .. } => OrderType::PostOnly {
                            id: order_id,
                            price,
                            quantity,
                            side,
                            timestamp,
                            time_in_force,
                        },
                        OrderType::ReserveOrder {
                            hidden_quantity,
                            replenish_threshold,
                            replenish_amount,
                            auto_replenish,
                            ..
                        } => OrderType::ReserveOrder {
                            id: order_id,
                            price,
                            visible_quantity: quantity,
                            hidden_quantity: *hidden_quantity,
                            side,
                            timestamp,
                            time_in_force,
                            replenish_threshold: *replenish_threshold,
                            replenish_amount: *replenish_amount,
                            auto_replenish: *auto_replenish,
                        },
                        // Add cases for other order types if needed
                        _ => {
                            return Err(OrderBookError::InvalidOperation {
                                message: "Replace operation not supported for this order type"
                                    .to_string(),
                            });
                        }
                    };

                    // Cancel the original order
                    self.cancel_order(order_id)?;

                    // Add the new order
                    let result = self.add_order(new_order)?;
                    Ok(Some(result))
                } else {
                    Ok(None) // Original order not found
                }
            }
        }
    }

    /// Cancel an order by ID
    pub fn cancel_order(
        &self,
        order_id: OrderId,
    ) -> Result<Option<Arc<OrderType>>, OrderBookError> {
        self.cache.invalidate();
        // First, we find the order's location (price and side) without locking
        let location = self.order_locations.get(&order_id).map(|val| *val);

        if let Some((price, side)) = location {
            // Obtener el mapa de niveles de precio apropiado
            let price_levels = match side {
                Side::Buy => &self.bids,
                Side::Sell => &self.asks,
            };

            // Create the update to cancel
            let update = OrderUpdate::Cancel { order_id };

            // Use entry() to safely modify the price level
            let mut result = None;
            let mut empty_level = false;

            price_levels.entry(price).and_modify(|price_level| {
                // Try to cancel the order
                if let Ok(cancelled) = price_level.update_order(update) {
                    result = cancelled;

                    // Check if the level became empty
                    empty_level = price_level.order_count() == 0;
                }
            });

            self.cache.invalidate();
            // If we got a result and the order was canceled
            if result.is_some() {
                // Remove the order from the locations map
                self.order_locations.remove(&order_id);

                // If the level became empty, remove it
                if empty_level {
                    price_levels.remove(&price);
                }
            }

            Ok(result)
        } else {
            Ok(None)
        }
    }

    /// Add a new order to the book, automatically matching it if it's aggressive.
    pub fn add_order(&self, mut order: OrderType) -> Result<Arc<OrderType>, OrderBookError> {
        self.cache.invalidate();

        trace!(
            "Order book {}: Adding order {} at price {}",
            self.symbol,
            order.id(),
            order.price()
        );

        if self.has_expired(&order) {
            return Err(OrderBookError::InvalidOperation {
                message: "Order has already expired".to_string(),
            });
        }

        if order.is_post_only() && self.will_cross_market(order.price(), order.side()) {
            return Err(OrderBookError::PriceCrossing {
                price: order.price(),
                side: order.side(),
                opposite_price: if order.side() == Side::Buy {
                    self.best_ask().unwrap_or(0)
                } else {
                    self.best_bid().unwrap_or(0)
                },
            });
        }

        // For FOK orders, first check if the entire quantity can be matched without altering the book.
        if order.is_fill_or_kill() {
            let potential_match =
                self.peek_match(order.side(), order.total_quantity(), Some(order.price()));
            if potential_match < order.total_quantity() {
                return Err(OrderBookError::InsufficientLiquidity {
                    side: order.side(),
                    requested: order.total_quantity(),
                    available: potential_match,
                });
            }
        }

        self.cache.invalidate();
        // Attempt to match the order immediately
        let match_result = self.match_order(
            order.id(),
            order.side(),
            order.total_quantity(), // Use total quantity for matching
            Some(order.price()),
        )?;

        if !match_result.transactions.transactions.is_empty() {
            if let Some(ref listener) = self.trade_listener {
                listener(&match_result) // emit trade events to listener
            }
        }

        // If the order was not fully filled, add the remainder to the book
        if match_result.remaining_quantity > 0 {
            if order.is_immediate() {
                // IOC/FOK orders should not have a resting part.
                // If FOK, it should have been fully filled or cancelled before this point.
                // If IOC, this is the remaining part that couldn't be filled, so we just drop it.
                return Err(OrderBookError::InsufficientLiquidity {
                    side: order.side(),
                    requested: order.quantity(), // Now uses the trait method
                    available: order.quantity() - match_result.remaining_quantity,
                });
            }

            // Update the order with the remaining quantity
            order.set_quantity(match_result.remaining_quantity); // Now uses the trait method

            let price = order.price();
            let side = order.side();

            let price_levels = match side {
                Side::Buy => &self.bids,
                Side::Sell => &self.asks,
            };

            let price_level = price_levels
                .entry(price)
                .or_insert_with(|| Arc::new(PriceLevel::new(price)));

            let order_arc = price_level.add_order(order);
            self.order_locations.insert(order_arc.id(), (price, side));

            Ok(order_arc)
        } else {
            // The order was fully matched, create an Arc from the matched result
            // Note: The original order object is consumed, but we can reconstruct its essence if needed.
            // For now, we return a representation of the completed order.
            Ok(Arc::new(order))
        }
    }
}
