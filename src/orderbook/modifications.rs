use crate::{OrderBook, OrderBookError};
use pricelevel::{OrderId, OrderType, OrderUpdate, PriceLevel, Side};
use std::sync::Arc;
use tracing::trace;

impl OrderBook {
    /// Update an order's price and/or quantity
    pub fn update_order(
        &self,
        update: OrderUpdate,
    ) -> Result<Option<Arc<OrderType>>, OrderBookError> {
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
        // Primero encontramos la ubicación de la orden (precio y lado) sin bloquear
        let location = self.order_locations.get(&order_id).map(|val| *val);

        if let Some((price, side)) = location {
            // Obtener el mapa de niveles de precio apropiado
            let price_levels = match side {
                Side::Buy => &self.bids,
                Side::Sell => &self.asks,
            };

            // Crear la actualización para cancelar
            let update = OrderUpdate::Cancel { order_id };

            // Utilizar entry() para modificar el nivel de precio de manera segura
            let mut result = None;
            let mut empty_level = false;

            price_levels.entry(price).and_modify(|price_level| {
                // Intentar cancelar la orden
                if let Ok(cancelled) = price_level.update_order(update) {
                    result = cancelled;

                    // Verificar si el nivel quedó vacío
                    empty_level = price_level.order_count() == 0;
                }
            });

            // Si obtuvimos un resultado y la orden fue cancelada
            if result.is_some() {
                // Eliminar la orden del mapa de ubicaciones
                self.order_locations.remove(&order_id);

                // Si el nivel quedó vacío, eliminarlo
                if empty_level {
                    price_levels.remove(&price);
                }
            }

            Ok(result)
        } else {
            // La orden no se encontró
            Ok(None)
        }
    }

    /// Add a new order to the book
    pub fn add_order(&self, order: OrderType) -> Result<Arc<OrderType>, OrderBookError> {
        trace!(
            "Order book {}: Adding order {} at price {}",
            self.symbol,
            order.id(),
            order.price()
        );
        let price = order.price();
        let side = order.side();

        // Check if the order has expired before adding
        if self.has_expired(&order) {
            return Err(OrderBookError::InvalidOperation {
                message: "Order has already expired".to_string(),
            });
        }

        // For post-only orders, check for price crossing
        if order.is_post_only() && self.will_cross_market(price, side) {
            let opposite_price = match side {
                Side::Buy => self.best_ask().unwrap(),
                Side::Sell => self.best_bid().unwrap(),
            };

            return Err(OrderBookError::PriceCrossing {
                price,
                side,
                opposite_price,
            });
        }

        // Handle immediate-or-cancel and fill-or-kill orders
        if order.is_immediate() {
            return self.handle_immediate_order(order);
        }

        // Standard limit order processing
        let price_levels = match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };

        // Get or create the price level
        let price_level = price_levels
            .entry(price)
            .or_insert_with(|| Arc::new(PriceLevel::new(price)));

        // Add the order to the price level
        let order_arc = price_level.add_order(order);

        // Track the order's location
        self.order_locations.insert(order_arc.id(), (price, side));

        Ok(order_arc)
    }
}
