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

use anvil_matching::types::Order as MatchingOrder;
use anvil_sdk::types::{PlaceOrderRequest, Side};
use std::collections::HashMap;
use thiserror::Error;

/// Error types for routing operations
#[derive(Debug, Error)]
pub enum RouterError {
	#[error("Matching engine not found for market: {0}")]
	MatchingEngineNotFound(String),
	#[error("Routing error: {0}")]
	RoutingError(String),
}

/// Router that forwards orders to the appropriate matching engine
///
/// In production, this would connect to matching engines via gRPC,
/// message queues, or other inter-service communication mechanisms.
pub struct Router {
	/// Market -> Matching engine endpoint mapping
	/// In a real implementation, this would be a connection pool or client
	matching_engines: HashMap<String, String>,
}

impl Router {
	/// Create a new router
	pub fn new() -> Self {
		let mut engines = HashMap::new();
		// TODO: Load from configuration
		engines.insert("BTC-USDT".to_string(), "http://localhost:8081".to_string());
		Self {
			matching_engines: engines,
		}
	}

	/// Route an order to the appropriate matching engine
	///
	/// This converts the gateway's PlaceOrderRequest into the matching
	/// engine's internal Order format and forwards it.
	pub fn route_order(
		&self,
		request: PlaceOrderRequest,
		user_id: String,
	) -> Result<MatchingOrder, RouterError> {
		// Check if matching engine exists for this market
		if !self.matching_engines.contains_key(&request.market) {
			return Err(RouterError::MatchingEngineNotFound(request.market));
		}

		// Convert PlaceOrderRequest to MatchingOrder
		let price = request
			.price
			.ok_or_else(|| RouterError::RoutingError("Limit orders require a price".to_string()))?;

		let order = MatchingOrder {
			order_id: uuid::Uuid::new_v4().to_string(),
			market: request.market.clone(),
			side: request.side,
			price,
			size: request.size,
			remaining_size: request.size,
			timestamp: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			user_id,
		};

		// TODO: Actually send to matching engine via gRPC/message queue
		// For now, just return the converted order

		Ok(order)
	}
}

impl Default for Router {
	fn default() -> Self {
		Self::new()
	}
}
