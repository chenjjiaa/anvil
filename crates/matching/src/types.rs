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

use anvil_sdk::types::{Side, Trade};
use serde::{Deserialize, Serialize};

/// Order command received from RPC layer
///
/// This represents an incoming order request that has been validated
/// by the RPC layer and is ready to enter the matching engine pipeline.
/// The command includes all necessary information for matching and
/// serves as the idempotency key holder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderCommand {
	/// Unique order ID (serves as idempotency key)
	pub order_id: String,
	/// Market identifier
	pub market: String,
	/// Order side
	pub side: Side,
	/// Price (for limit orders)
	pub price: u64,
	/// Size/quantity
	pub size: u64,
	/// Timestamp when order was received (for time priority)
	pub timestamp: u64,
	/// Cryptographic principal identifier (hex-encoded public key)
	///
	/// This is NOT a business user ID. Gateway only understands cryptographic
	/// identity, not business user identity. The matching engine receives
	/// the principal identifier (public key) from Gateway.
	pub public_key: String,
}

/// Internal order representation for the matching engine
///
/// This represents an order that is currently in the orderbook
/// or has been processed. It extends OrderCommand with execution state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
	/// Unique order ID
	pub order_id: String,
	/// Market identifier
	pub market: String,
	/// Order side
	pub side: Side,
	/// Price (for limit orders)
	pub price: u64,
	/// Size/quantity
	pub size: u64,
	/// Remaining size
	pub remaining_size: u64,
	/// Timestamp when order was received (for time priority)
	pub timestamp: u64,
	/// Cryptographic principal identifier (hex-encoded public key)
	///
	/// This is NOT a business user ID. Gateway only understands cryptographic
	/// identity, not business user identity. The matching engine receives
	/// the principal identifier (public key) from Gateway.
	pub public_key: String,
}

impl From<OrderCommand> for Order {
	fn from(cmd: OrderCommand) -> Self {
		Self {
			order_id: cmd.order_id,
			market: cmd.market,
			side: cmd.side,
			price: cmd.price,
			size: cmd.size,
			remaining_size: cmd.size,
			timestamp: cmd.timestamp,
			public_key: cmd.public_key,
		}
	}
}

/// Matching result from processing an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
	/// The order that was matched
	pub order: Order,
	/// List of trades generated from matching
	pub trades: Vec<Trade>,
	/// Whether the order was fully filled
	pub fully_filled: bool,
	/// Whether the order was partially filled and remaining on the book
	pub partially_filled: bool,
}

/// Error types for matching operations
#[derive(Debug, thiserror::Error)]
pub enum MatchingError {
	#[error("Invalid order: {0}")]
	InvalidOrder(String),
	#[error("Order book error: {0}")]
	OrderBookError(String),
	#[error("Market not found: {0}")]
	MarketNotFound(String),
}
