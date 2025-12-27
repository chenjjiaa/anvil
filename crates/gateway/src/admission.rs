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

use anvil_sdk::types::{OrderType, PlaceOrderRequest};
use thiserror::Error;

/// Error types for admission control
#[derive(Debug, Error)]
pub enum AdmissionError {
	#[error("Invalid order: {0}")]
	InvalidOrder(String),
	#[error("Rate limit exceeded")]
	RateLimitExceeded,
	#[error("Market not available: {0}")]
	MarketNotAvailable(String),
	#[error("Insufficient balance")]
	InsufficientBalance,
}

/// Validate and admit an order request
///
/// This function performs basic syntactic validation and protocol-level
/// admission checks before forwarding to the matching engine.
pub fn validate_and_admit(request: &PlaceOrderRequest) -> Result<(), AdmissionError> {
	// Validate market identifier
	if request.market.is_empty() {
		return Err(AdmissionError::InvalidOrder(
			"Market identifier is required".to_string(),
		));
	}

	// Validate size
	if request.size == 0 {
		return Err(AdmissionError::InvalidOrder(
			"Order size must be greater than zero".to_string(),
		));
	}

	// Validate price for limit orders
	if matches!(request.order_type, OrderType::Limit) {
		if request.price.is_none() {
			return Err(AdmissionError::InvalidOrder(
				"Limit orders require a price".to_string(),
			));
		}
		if let Some(price) = request.price {
			if price == 0 {
				return Err(AdmissionError::InvalidOrder(
					"Price must be greater than zero".to_string(),
				));
			}
		}
	}

	// TODO: Check rate limits
	// TODO: Verify market availability
	// TODO: Check user balance (if required by protocol)

	Ok(())
}
