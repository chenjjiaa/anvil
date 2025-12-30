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

use crate::orderbook::OrderBook;
use crate::types::{MatchResult, MatchingError, Order};
use anvil_sdk::types::{Side, Trade};
use dashmap::DashMap;

/// Matching engine that applies deterministic price-time priority
///
/// The matcher maintains order books per market and applies
/// deterministic matching rules to produce replayable results.
/// Uses DashMap for concurrent access without locks.
pub struct Matcher {
	/// Market -> OrderBook mapping (concurrent)
	order_books: DashMap<String, OrderBook>,
}

impl Matcher {
	/// Create a new matching engine
	pub fn new() -> Self {
		Self {
			order_books: DashMap::new(),
		}
	}

	/// Get or create an order book for a market
	#[allow(dead_code)]
	fn get_or_create_orderbook(&self, market: &str) -> OrderBook {
		self.order_books
			.entry(market.to_string())
			.or_insert_with(|| OrderBook::new(market.to_string()))
			.clone()
	}

	/// Process an incoming order and match it against the book
	///
	/// This function applies deterministic price-time priority:
	/// - Price priority: better prices match first
	/// - Time priority: earlier orders at the same price match first
	///
	/// Returns a MatchResult containing all trades generated.
	pub fn match_order(&self, mut order: Order) -> Result<MatchResult, MatchingError> {
		// Get or create orderbook
		let orderbook = self
			.order_books
			.entry(order.market.clone())
			.or_insert_with(|| OrderBook::new(order.market.clone()))
			.clone();

		let mut trades = Vec::new();
		let mut remaining_size = order.remaining_size;

		// Match against the opposite side
		while remaining_size > 0 {
			let matched = match order.side {
				Side::Buy => Self::match_buy_order(&orderbook, &order, remaining_size),
				Side::Sell => Self::match_sell_order(&orderbook, &order, remaining_size),
			};

			match matched {
				Some(trade) => {
					trades.push(trade.clone());
					remaining_size -= trade.size;
				}
				None => break,
			}
		}

		order.remaining_size = remaining_size;

		let fully_filled = remaining_size == 0;
		let partially_filled = !trades.is_empty() && remaining_size > 0;

		// If order is not fully filled, add it to the book
		if !fully_filled {
			orderbook.add_order(order.clone());
		}

		Ok(MatchResult {
			order,
			trades,
			fully_filled,
			partially_filled,
		})
	}

	/// Match a buy order against the ask side
	fn match_buy_order(orderbook: &OrderBook, order: &Order, remaining_size: u64) -> Option<Trade> {
		let best_ask = orderbook.best_ask()?;

		// Check if buy order price is acceptable
		if order.price < best_ask {
			return None; // No match possible
		}

		// Get the best ask level
		let mut ask_level = orderbook.best_ask_level()?;
		let maker_order = ask_level.get_first_order()?.clone();
		let match_price = best_ask;
		let match_size = remaining_size.min(maker_order.remaining_size);

		// Update or remove maker order
		if maker_order.remaining_size == match_size {
			// Fully filled, remove it
			ask_level.remove_first_order();
			// Also remove from orderbook if level is now empty
			if ask_level.is_empty() {
				drop(ask_level); // Release lock
				orderbook.remove_order(Side::Sell, &maker_order.order_id);
			}
		} else {
			// Partially filled, update size
			ask_level.update_order_size(
				&maker_order.order_id,
				maker_order.remaining_size - match_size,
			);
		}

		// Create trade
		Some(Trade {
			trade_id: format!("trade_{}", uuid::Uuid::new_v4()),
			market: order.market.clone(),
			price: match_price,
			size: match_size,
			side: Side::Buy,
			timestamp: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			maker_order_id: maker_order.order_id.clone(),
			taker_order_id: order.order_id.clone(),
		})
	}

	/// Match a sell order against the bid side
	fn match_sell_order(
		orderbook: &OrderBook,
		order: &Order,
		remaining_size: u64,
	) -> Option<Trade> {
		let best_bid = orderbook.best_bid()?;

		// Check if sell order price is acceptable
		if order.price > best_bid {
			return None; // No match possible
		}

		// Get the best bid level
		let mut bid_level = orderbook.best_bid_level()?;
		let maker_order = bid_level.get_first_order()?.clone();
		let match_price = best_bid;
		let match_size = remaining_size.min(maker_order.remaining_size);

		// Update or remove maker order
		if maker_order.remaining_size == match_size {
			// Fully filled, remove it
			bid_level.remove_first_order();
			// Also remove from orderbook if level is now empty
			if bid_level.is_empty() {
				drop(bid_level); // Release lock
				orderbook.remove_order(Side::Buy, &maker_order.order_id);
			}
		} else {
			// Partially filled, update size
			bid_level.update_order_size(
				&maker_order.order_id,
				maker_order.remaining_size - match_size,
			);
		}

		// Create trade
		Some(Trade {
			trade_id: format!("trade_{}", uuid::Uuid::new_v4()),
			market: order.market.clone(),
			price: match_price,
			size: match_size,
			side: Side::Sell,
			timestamp: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			maker_order_id: maker_order.order_id.clone(),
			taker_order_id: order.order_id.clone(),
		})
	}

	/// Cancel an order from the book
	pub fn cancel_order(
		&self,
		market: &str,
		side: Side,
		order_id: &str,
	) -> Result<Option<Order>, MatchingError> {
		let orderbook = self
			.order_books
			.get(market)
			.ok_or_else(|| MatchingError::MarketNotFound(market.to_string()))?;

		Ok(orderbook.remove_order(side, order_id))
	}
}

impl Default for Matcher {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anvil_sdk::types::Side;

	#[test]
	fn test_match_buy_order() {
		let matcher = Matcher::new();

		// Add a sell order
		let sell_order = Order {
			order_id: "sell_1".to_string(),
			market: "BTC-USDT".to_string(),
			side: Side::Sell,
			price: 50000,
			size: 1,
			remaining_size: 1,
			timestamp: 1000,
			public_key: "user1".to_string(),
		};

		let _ = matcher.match_order(sell_order);

		// Add a buy order that matches
		let buy_order = Order {
			order_id: "buy_1".to_string(),
			market: "BTC-USDT".to_string(),
			side: Side::Buy,
			price: 50000,
			size: 1,
			remaining_size: 1,
			timestamp: 2000,
			public_key: "user2".to_string(),
		};

		let result = matcher.match_order(buy_order).unwrap();
		assert!(result.fully_filled);
		assert_eq!(result.trades.len(), 1);
		assert_eq!(result.trades[0].price, 50000);
		assert_eq!(result.trades[0].size, 1);
	}

	#[test]
	fn test_price_time_priority() {
		let matcher = Matcher::new();

		// Add multiple sell orders at same price
		for i in 0..3 {
			let sell_order = Order {
				order_id: format!("sell_{}", i),
				market: "BTC-USDT".to_string(),
				side: Side::Sell,
				price: 50000,
				size: 1,
				remaining_size: 1,
				timestamp: 1000 + i,
				public_key: format!("user_{}", i),
			};
			let _ = matcher.match_order(sell_order);
		}

		// Add a buy order that matches all
		let buy_order = Order {
			order_id: "buy_1".to_string(),
			market: "BTC-USDT".to_string(),
			side: Side::Buy,
			price: 50000,
			size: 3,
			remaining_size: 3,
			timestamp: 2000,
			public_key: "user_buyer".to_string(),
		};

		let result = matcher.match_order(buy_order).unwrap();
		assert!(result.fully_filled);
		assert_eq!(result.trades.len(), 3);
		// Should match in time order (sell_0, sell_1, sell_2)
		assert_eq!(result.trades[0].maker_order_id, "sell_0");
		assert_eq!(result.trades[1].maker_order_id, "sell_1");
		assert_eq!(result.trades[2].maker_order_id, "sell_2");
	}
}
