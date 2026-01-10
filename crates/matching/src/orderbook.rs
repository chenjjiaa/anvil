// Copyright 2025 itscheems
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::BTreeMap;

use anvil_sdk::types::Side;
use serde::{Deserialize, Serialize};

use crate::types::Order;

/// Price level in the order book
///
/// A price level contains all orders at a specific price, maintained
/// in time priority order (first-in-first-out).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
	price: u64,
	/// Orders at this price level in time priority order
	orders: Vec<Order>,
	/// Total size of all orders at this level
	total_size: u64,
}

impl PriceLevel {
	fn new(price: u64) -> Self {
		Self {
			price,
			orders: Vec::new(),
			total_size: 0,
		}
	}

	pub fn add_order(&mut self, order: Order) {
		self.total_size += order.remaining_size;
		self.orders.push(order);
	}

	pub fn remove_order(&mut self, order_id: &str) -> Option<Order> {
		if let Some(pos) = self.orders.iter().position(|o| o.order_id == order_id) {
			let order = self.orders.remove(pos);
			self.total_size -= order.remaining_size;
			Some(order)
		} else {
			None
		}
	}

	pub fn update_order_size(&mut self, order_id: &str, new_size: u64) -> bool {
		if let Some(order) = self.orders.iter_mut().find(|o| o.order_id == order_id) {
			let old_size = order.remaining_size;
			self.total_size = self.total_size - old_size + new_size;
			order.remaining_size = new_size;
			true
		} else {
			false
		}
	}

	pub fn get_first_order(&self) -> Option<&Order> {
		self.orders.first()
	}

	pub fn get_first_order_mut(&mut self) -> Option<&mut Order> {
		self.orders.first_mut()
	}

	pub fn remove_first_order(&mut self) -> Option<Order> {
		if !self.orders.is_empty() {
			let order = self.orders.remove(0);
			self.total_size -= order.remaining_size;
			Some(order)
		} else {
			None
		}
	}

	pub fn is_empty(&self) -> bool {
		self.orders.is_empty()
	}

	pub fn total_size(&self) -> u64 {
		self.total_size
	}

	pub fn order_count(&self) -> usize {
		self.orders.len()
	}
}

/// Limit order book maintaining buy and sell sides (single-threaded)
///
/// This is a deterministic, single-threaded order book implementation
/// using BTreeMap for price-sorted levels. All operations are designed
/// to be called from a single thread (the matching loop).
///
/// Design characteristics:
/// - No concurrent access (no locks, no Arc, no DashMap)
/// - Deterministic iteration order
/// - Price-time priority enforced
/// - Buy side: highest price first (descending order via Reverse wrapper)
/// - Sell side: lowest price first (ascending order, natural BTreeMap order)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
	market: String,
	/// Buy side: price (high to low) -> PriceLevel
	/// We use BTreeMap in reverse order for bids
	bids: BTreeMap<std::cmp::Reverse<u64>, PriceLevel>,
	/// Sell side: price (low to high) -> PriceLevel
	asks: BTreeMap<u64, PriceLevel>,
}

impl OrderBook {
	/// Create a new order book for a market
	pub fn new(market: String) -> Self {
		Self {
			market,
			bids: BTreeMap::new(),
			asks: BTreeMap::new(),
		}
	}

	/// Get the market identifier
	pub fn market(&self) -> &str {
		&self.market
	}

	/// Add an order to the book
	pub fn add_order(&mut self, order: Order) {
		match order.side {
			Side::Buy => {
				self.bids
					.entry(std::cmp::Reverse(order.price))
					.or_insert_with(|| PriceLevel::new(order.price))
					.add_order(order);
			}
			Side::Sell => {
				self.asks
					.entry(order.price)
					.or_insert_with(|| PriceLevel::new(order.price))
					.add_order(order);
			}
		}
	}

	/// Remove an order from the book
	pub fn remove_order(&mut self, side: Side, order_id: &str) -> Option<Order> {
		let mut result = None;

		match side {
			Side::Buy => {
				let mut price_to_remove = None;

				for (price_key, level) in self.bids.iter_mut() {
					if let Some(order) = level.remove_order(order_id) {
						result = Some(order);
						if level.is_empty() {
							price_to_remove = Some(*price_key);
						}
						break;
					}
				}

				if let Some(price) = price_to_remove {
					self.bids.remove(&price);
				}
			}
			Side::Sell => {
				let mut price_to_remove = None;

				for (price_key, level) in self.asks.iter_mut() {
					if let Some(order) = level.remove_order(order_id) {
						result = Some(order);
						if level.is_empty() {
							price_to_remove = Some(*price_key);
						}
						break;
					}
				}

				if let Some(price) = price_to_remove {
					self.asks.remove(&price);
				}
			}
		}

		result
	}

	/// Get the best bid price
	pub fn best_bid(&self) -> Option<u64> {
		self.bids.first_key_value().map(|(key, _)| key.0)
	}

	/// Get the best ask price
	pub fn best_ask(&self) -> Option<u64> {
		self.asks.first_key_value().map(|(key, _)| *key)
	}

	/// Get mutable reference to the best bid level
	pub fn best_bid_level_mut(&mut self) -> Option<&mut PriceLevel> {
		self.bids.first_entry().map(|entry| entry.into_mut())
	}

	/// Get mutable reference to the best ask level
	pub fn best_ask_level_mut(&mut self) -> Option<&mut PriceLevel> {
		self.asks.first_entry().map(|entry| entry.into_mut())
	}

	/// Get the level depth at a specific price level
	pub fn get_level_depth(&self, side: Side, price: u64) -> Option<u64> {
		match side {
			Side::Buy => self
				.bids
				.get(&std::cmp::Reverse(price))
				.map(|l| l.total_size()),
			Side::Sell => self.asks.get(&price).map(|l| l.total_size()),
		}
	}

	/// Get total number of orders in the book
	pub fn order_count(&self) -> usize {
		let bid_count: usize = self.bids.values().map(|l| l.order_count()).sum();
		let ask_count: usize = self.asks.values().map(|l| l.order_count()).sum();
		bid_count + ask_count
	}

	/// Clear all orders from the book
	pub fn clear(&mut self) {
		self.bids.clear();
		self.asks.clear();
	}

	/// Find an order by ID and return a mutable reference (for replay/update)
	///
	/// This method is primarily used during event replay to update order state.
	/// It searches both bid and ask sides to locate the order.
	pub fn find_order_mut(&mut self, order_id: &str) -> Option<&mut Order> {
		// Search in bids first
		for level in self.bids.values_mut() {
			for order in &mut level.orders {
				if order.order_id == order_id {
					return Some(order);
				}
			}
		}
		// Then search in asks
		for level in self.asks.values_mut() {
			for order in &mut level.orders {
				if order.order_id == order_id {
					return Some(order);
				}
			}
		}
		None
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn create_test_order(order_id: &str, side: Side, price: u64, size: u64) -> Order {
		Order {
			order_id: order_id.to_string(),
			market: "BTC-USDT".to_string(),
			side,
			price,
			size,
			remaining_size: size,
			timestamp: 1000,
			public_key: "test_key".to_string(),
		}
	}

	#[test]
	fn test_add_and_remove_order() {
		let mut book = OrderBook::new("BTC-USDT".to_string());

		let order = create_test_order("order_1", Side::Buy, 50000, 1);
		book.add_order(order.clone());

		assert_eq!(book.best_bid(), Some(50000));
		assert_eq!(book.order_count(), 1);

		let removed = book.remove_order(Side::Buy, "order_1");
		assert!(removed.is_some());
		assert_eq!(book.order_count(), 0);
		assert_eq!(book.best_bid(), None);
	}

	#[test]
	fn test_price_priority() {
		let mut book = OrderBook::new("BTC-USDT".to_string());

		book.add_order(create_test_order("order_1", Side::Buy, 50000, 1));
		book.add_order(create_test_order("order_2", Side::Buy, 51000, 1));
		book.add_order(create_test_order("order_3", Side::Buy, 49000, 1));

		// Best bid should be highest price
		assert_eq!(book.best_bid(), Some(51000));

		// Remove best bid
		book.remove_order(Side::Buy, "order_2");
		assert_eq!(book.best_bid(), Some(50000));
	}

	#[test]
	fn test_time_priority_at_same_price() {
		let mut book = OrderBook::new("BTC-USDT".to_string());

		book.add_order(create_test_order("order_1", Side::Sell, 50000, 1));
		book.add_order(create_test_order("order_2", Side::Sell, 50000, 1));
		book.add_order(create_test_order("order_3", Side::Sell, 50000, 1));

		let level = book.best_ask_level_mut().unwrap();
		let first_order = level.get_first_order().unwrap();
		assert_eq!(first_order.order_id, "order_1");

		level.remove_first_order();
		let second_order = level.get_first_order().unwrap();
		assert_eq!(second_order.order_id, "order_2");
	}

	#[test]
	fn test_level_depth() {
		let mut book = OrderBook::new("BTC-USDT".to_string());

		book.add_order(create_test_order("order_1", Side::Buy, 50000, 1));
		book.add_order(create_test_order("order_2", Side::Buy, 50000, 2));
		book.add_order(create_test_order("order_3", Side::Buy, 50000, 3));

		assert_eq!(book.get_level_depth(Side::Buy, 50000), Some(6));
	}
}
