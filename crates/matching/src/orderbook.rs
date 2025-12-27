// Copyright 2025 chenjjiaa
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

use crate::types::Order;
use anvil_sdk::types::Side;
use dashmap::DashMap;
use std::sync::Arc;

/// Price level in the order book
#[derive(Debug, Clone)]
pub struct PriceLevel {
	#[allow(dead_code)]
	price: u64,
	orders: Vec<Order>,
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

	fn add_order(&mut self, order: Order) {
		self.total_size += order.remaining_size;
		self.orders.push(order);
	}

	fn remove_order(&mut self, order_id: &str) -> Option<Order> {
		if let Some(pos) = self.orders.iter().position(|o| o.order_id == order_id) {
			let order = self.orders.remove(pos);
			self.total_size -= order.remaining_size;
			Some(order)
		} else {
			None
		}
	}

	pub(crate) fn update_order_size(&mut self, order_id: &str, new_size: u64) -> bool {
		if let Some(order) = self.orders.iter_mut().find(|o| o.order_id == order_id) {
			let old_size = order.remaining_size;
			self.total_size = self.total_size - old_size + new_size;
			order.remaining_size = new_size;
			true
		} else {
			false
		}
	}

	pub(crate) fn get_first_order(&self) -> Option<&Order> {
		self.orders.first()
	}

	pub(crate) fn remove_first_order(&mut self) -> Option<Order> {
		if !self.orders.is_empty() {
			let order = self.orders.remove(0);
			self.total_size -= order.remaining_size;
			Some(order)
		} else {
			None
		}
	}

	pub(crate) fn is_empty(&self) -> bool {
		self.orders.is_empty()
	}
}

/// Limit order book maintaining buy and sell sides
///
/// The order book uses DashMap for concurrent access and BTreeMap for ordering:
/// - Buy side: highest price first (descending)
/// - Sell side: lowest price first (ascending)
#[derive(Debug, Clone)]
pub struct OrderBook {
	market: String,
	/// Buy side: price -> PriceLevel (concurrent map, sorted by price on access)
	bids: Arc<DashMap<u64, PriceLevel>>,
	/// Sell side: price -> PriceLevel (concurrent map, sorted by price on access)
	asks: Arc<DashMap<u64, PriceLevel>>,
}

impl OrderBook {
	/// Create a new order book for a market
	pub fn new(market: String) -> Self {
		Self {
			market,
			bids: Arc::new(DashMap::new()),
			asks: Arc::new(DashMap::new()),
		}
	}

	/// Get the market identifier
	pub fn market(&self) -> &str {
		&self.market
	}

	/// Add an order to the book
	pub fn add_order(&self, order: Order) {
		let side_map = match order.side {
			Side::Buy => &self.bids,
			Side::Sell => &self.asks,
		};

		side_map
			.entry(order.price)
			.or_insert_with(|| PriceLevel::new(order.price))
			.add_order(order);
	}

	/// Remove an order from the book
	pub fn remove_order(&self, side: Side, order_id: &str) -> Option<Order> {
		let side_map = match side {
			Side::Buy => &self.bids,
			Side::Sell => &self.asks,
		};

		// Find and remove the order
		let mut result = None;
		let mut price_to_remove = None;

		for mut entry in side_map.iter_mut() {
			let price = *entry.key();
			let level = entry.value_mut();
			if let Some(order) = level.remove_order(order_id) {
				result = Some(order);
				if level.orders.is_empty() {
					price_to_remove = Some(price);
				}
				break;
			}
		}

		// Remove empty price levels
		if let Some(price) = price_to_remove {
			side_map.remove(&price);
		}

		result
	}

	/// Get the best bid price
	pub fn best_bid(&self) -> Option<u64> {
		// DashMap doesn't maintain order, so we need to iterate
		self.bids.iter().map(|entry| *entry.key()).max()
	}

	/// Get the best ask price
	pub fn best_ask(&self) -> Option<u64> {
		// DashMap doesn't maintain order, so we need to iterate
		self.asks.iter().map(|entry| *entry.key()).min()
	}

	/// Get the best bid price level (for matching)
	pub fn best_bid_level(&self) -> Option<dashmap::mapref::one::RefMut<'_, u64, PriceLevel>> {
		if let Some(price) = self.best_bid() {
			self.bids.get_mut(&price)
		} else {
			None
		}
	}

	/// Get the best ask price level (for matching)
	pub fn best_ask_level(&self) -> Option<dashmap::mapref::one::RefMut<'_, u64, PriceLevel>> {
		if let Some(price) = self.best_ask() {
			self.asks.get_mut(&price)
		} else {
			None
		}
	}

	/// Get all orders at a price level (for matching)
	pub fn get_orders_at_price(&self, side: Side, price: u64) -> Option<Vec<Order>> {
		let side_map = match side {
			Side::Buy => &self.bids,
			Side::Sell => &self.asks,
		};
		side_map.get(&price).map(|level| level.orders.clone())
	}
}
