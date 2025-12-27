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

use crate::orderbook::OrderBook;
use crate::types::{MatchResult, MatchingError, Order};
use anvil_sdk::types::{Side, Trade};
use std::collections::HashMap;

/// Matching engine that applies deterministic price-time priority
///
/// The matcher maintains order books per market and applies
/// deterministic matching rules to produce replayable results.
pub struct Matcher {
	/// Market -> OrderBook mapping
	order_books: HashMap<String, OrderBook>,
}

impl Matcher {
	/// Create a new matching engine
	pub fn new() -> Self {
		Self {
			order_books: HashMap::new(),
		}
	}

	/// Get or create an order book for a market
	fn get_or_create_orderbook(&mut self, market: &str) -> &mut OrderBook {
		self.order_books
			.entry(market.to_string())
			.or_insert_with(|| OrderBook::new(market.to_string()))
	}

	/// Process an incoming order and match it against the book
	///
	/// This function applies deterministic price-time priority:
	/// - Price priority: better prices match first
	/// - Time priority: earlier orders at the same price match first
	///
	/// Returns a MatchResult containing all trades generated.
	pub fn match_order(&mut self, mut order: Order) -> Result<MatchResult, MatchingError> {
		let orderbook = self
			.order_books
			.get_mut(&order.market)
			.ok_or_else(|| MatchingError::MarketNotFound(order.market.clone()))?;

		let mut trades = Vec::new();
		let mut remaining_size = order.remaining_size;

		// Match against the opposite side
		while remaining_size > 0 {
			let matched = match order.side {
				Side::Buy => Self::match_buy_order(orderbook, &order, remaining_size),
				Side::Sell => Self::match_sell_order(orderbook, &order, remaining_size),
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
	fn match_buy_order(
		orderbook: &mut OrderBook,
		order: &Order,
		remaining_size: u64,
	) -> Option<Trade> {
		let best_ask = orderbook.best_ask()?;

		// Check if buy order price is acceptable
		if order.price < best_ask {
			return None; // No match possible
		}

		// Get the best ask level
		let ask_level = orderbook.best_ask_level()?;
		let maker_order = ask_level.get_first_order()?.clone();
		let match_price = best_ask;
		let match_size = remaining_size.min(maker_order.remaining_size);

		// Update or remove maker order
		if maker_order.remaining_size == match_size {
			// Fully filled, remove it
			ask_level.remove_first_order();
			// Also remove from orderbook if level is now empty
			if ask_level.is_empty() {
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
		orderbook: &mut OrderBook,
		order: &Order,
		remaining_size: u64,
	) -> Option<Trade> {
		let best_bid = orderbook.best_bid()?;

		// Check if sell order price is acceptable
		if order.price > best_bid {
			return None; // No match possible
		}

		// Get the best bid level
		let bid_level = orderbook.best_bid_level()?;
		let maker_order = bid_level.get_first_order()?.clone();
		let match_price = best_bid;
		let match_size = remaining_size.min(maker_order.remaining_size);

		// Update or remove maker order
		if maker_order.remaining_size == match_size {
			// Fully filled, remove it
			bid_level.remove_first_order();
			// Also remove from orderbook if level is now empty
			if bid_level.is_empty() {
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
		&mut self,
		market: &str,
		side: Side,
		order_id: &str,
	) -> Result<Option<Order>, MatchingError> {
		let orderbook = self
			.order_books
			.get_mut(market)
			.ok_or_else(|| MatchingError::MarketNotFound(market.to_string()))?;

		Ok(orderbook.remove_order(side, order_id))
	}
}

impl Default for Matcher {
	fn default() -> Self {
		Self::new()
	}
}
