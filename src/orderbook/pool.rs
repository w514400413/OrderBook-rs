/******************************************************************************
   Author: Joaquín Béjar García
   Email: jb@taunais.com
   Date: 15/7/25
******************************************************************************/

use pricelevel::OrderId;
use std::sync::Mutex;

/// Memory pool for reusing allocations in matching operations
pub struct MatchingPool {
    filled_orders_pool: Mutex<Vec<Vec<OrderId>>>,
    price_levels_pool: Mutex<Vec<Vec<u64>>>,
}

impl MatchingPool {
    pub fn new() -> Self {
        Self {
            filled_orders_pool: Mutex::new(Vec::new()),
            price_levels_pool: Mutex::new(Vec::new()),
        }
    }

    pub fn get_filled_orders_vec(&self) -> Vec<OrderId> {
        if let Ok(mut pool) = self.filled_orders_pool.try_lock() {
            pool.pop().unwrap_or_else(|| Vec::with_capacity(16))
        } else {
            Vec::with_capacity(16)
        }
    }

    pub fn return_filled_orders_vec(&self, mut vec: Vec<OrderId>) {
        vec.clear();
        if vec.capacity() <= 64 {
            if let Ok(mut pool) = self.filled_orders_pool.try_lock() {
                pool.push(vec);
            }
        }
    }

    pub fn get_price_vec(&self) -> Vec<u64> {
        if let Ok(mut pool) = self.price_levels_pool.try_lock() {
            pool.pop().unwrap_or_else(|| Vec::with_capacity(32))
        } else {
            Vec::with_capacity(32)
        }
    }

    pub fn return_price_vec(&self, mut vec: Vec<u64>) {
        vec.clear();
        if vec.capacity() <= 128 {
            if let Ok(mut pool) = self.price_levels_pool.try_lock() {
                pool.push(vec);
            }
        }
    }
}
