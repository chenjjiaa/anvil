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

use serde::{Deserialize, Serialize};

/// Order side (buy or sell)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
	Buy,
	Sell,
}

/// Order type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderType {
	Limit,
	Market,
}

/// Order status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
	Pending,
	Accepted,
	PartiallyFilled,
	Filled,
	Cancelled,
	Rejected,
}

/// Request to place an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceOrderRequest {
	/// Market identifier (e.g., "BTC-USDT")
	pub market: String,
	/// Order side
	pub side: Side,
	/// Order type
	#[serde(rename = "type")]
	pub order_type: OrderType,
	/// Price (for limit orders)
	pub price: Option<u64>,
	/// Size/quantity
	pub size: u64,
	/// Client-provided order ID (optional)
	pub client_order_id: Option<String>,
}

/// Response from placing an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceOrderResponse {
	/// Server-assigned order ID
	pub order_id: String,
	/// Status of the order
	pub status: OrderStatus,
	/// Client order ID (if provided)
	pub client_order_id: Option<String>,
}

/// Order information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
	/// Order ID
	pub order_id: String,
	/// Market identifier
	pub market: String,
	/// Order side
	pub side: Side,
	/// Order type
	#[serde(rename = "type")]
	pub order_type: OrderType,
	/// Price
	pub price: Option<u64>,
	/// Original size
	pub size: u64,
	/// Filled size
	pub filled_size: u64,
	/// Remaining size
	pub remaining_size: u64,
	/// Status
	pub status: OrderStatus,
	/// Timestamp when order was created
	pub created_at: u64,
}

/// Trade execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
	/// Trade ID
	pub trade_id: String,
	/// Market identifier
	pub market: String,
	/// Price at which trade executed
	pub price: u64,
	/// Size/quantity executed
	pub size: u64,
	/// Side of the trade (from taker's perspective)
	pub side: Side,
	/// Timestamp when trade occurred
	pub timestamp: u64,
	/// Maker order ID
	pub maker_order_id: String,
	/// Taker order ID
	pub taker_order_id: String,
}
