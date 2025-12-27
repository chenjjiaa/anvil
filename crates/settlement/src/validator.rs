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

use anvil_sdk::types::Trade;
use thiserror::Error;

/// Error types for trade validation
#[derive(Debug, Error)]
pub enum ValidationError {
	#[error("Invalid trade: {0}")]
	InvalidTrade(String),
	#[error("Price mismatch: expected {expected}, got {actual}")]
	PriceMismatch { expected: u64, actual: u64 },
	#[error("Size mismatch: expected {expected}, got {actual}")]
	SizeMismatch { expected: u64, actual: u64 },
	#[error("Market not found: {0}")]
	MarketNotFound(String),
	#[error("Protocol rule violation: {0}")]
	ProtocolViolation(String),
}

/// Validate a matched trade against protocol rules
///
/// This function ensures that trades produced by the matching engine
/// comply with protocol-level constraints before settlement.
pub fn validate_trade(trade: &Trade) -> Result<(), ValidationError> {
	// Validate basic fields
	if trade.market.is_empty() {
		return Err(ValidationError::InvalidTrade(
			"Market identifier is required".to_string(),
		));
	}

	if trade.price == 0 {
		return Err(ValidationError::InvalidTrade(
			"Price must be greater than zero".to_string(),
		));
	}

	if trade.size == 0 {
		return Err(ValidationError::InvalidTrade(
			"Size must be greater than zero".to_string(),
		));
	}

	if trade.maker_order_id.is_empty() || trade.taker_order_id.is_empty() {
		return Err(ValidationError::InvalidTrade(
			"Order IDs are required".to_string(),
		));
	}

	// Validate against protocol-specific rules
	// - Check price limits (circuit breakers)
	// - Verify market is still active
	// - Check for duplicate trades (replay protection)
	// - Validate user balances (if required)

	// Price limit check (example: prevent trades > 10% from last price)
	// This would typically query a price oracle or recent trade history
	// For now, we just check that price is reasonable (> 0)

	// Market availability check
	// This would query market status from a registry
	// For now, we assume market is available if market name is valid

	// Replay protection would check against a database of processed trades
	// For now, we rely on trade_id uniqueness

	Ok(())
}

/// Validate a batch of trades
pub fn validate_trades(trades: &[Trade]) -> Result<(), ValidationError> {
	for trade in trades {
		validate_trade(trade)?;
	}
	Ok(())
}
