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
use dashmap::DashMap;
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;
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
	#[allow(dead_code)]
	InsufficientBalance,
}

/// Market availability tracker
#[derive(Clone)]
struct MarketAvailability {
	available: bool,
	#[allow(dead_code)]
	last_check: Instant,
}

/// Rate limiter per user
type UserRateLimiter = Arc<
	RateLimiter<
		governor::state::direct::NotKeyed,
		governor::state::InMemoryState,
		governor::clock::DefaultClock,
	>,
>;

/// Admission controller
pub struct AdmissionController {
	/// Rate limiters per user
	rate_limiters: DashMap<String, UserRateLimiter>,
	/// Market availability
	markets: Arc<DashMap<String, MarketAvailability>>,
	/// Requests per second quota
	quota: Quota,
	/// Burst capacity
	#[allow(dead_code)]
	burst: u32,
}

impl AdmissionController {
	/// Create a new admission controller
	pub fn new(requests_per_second: u32, burst: u32) -> Self {
		let quota = Quota::per_second(NonZeroU32::new(requests_per_second).unwrap());
		Self {
			rate_limiters: DashMap::new(),
			markets: Arc::new(DashMap::new()),
			quota,
			burst,
		}
	}

	/// Check rate limit for a user
	pub fn check_rate_limit(&self, user_id: &str) -> Result<(), AdmissionError> {
		let limiter = self
			.rate_limiters
			.entry(user_id.to_string())
			.or_insert_with(|| Arc::new(RateLimiter::direct(self.quota)))
			.clone();

		limiter
			.check()
			.map_err(|_| AdmissionError::RateLimitExceeded)
	}

	/// Mark market as available
	#[allow(dead_code)]
	pub fn set_market_available(&self, market: &str, available: bool) {
		self.markets.insert(
			market.to_string(),
			MarketAvailability {
				available,
				last_check: Instant::now(),
			},
		);
	}

	/// Check if market is available
	pub fn is_market_available(&self, market: &str) -> bool {
		self.markets
			.get(market)
			.map(|m| m.available)
			.unwrap_or(true) // Default to available if not tracked
	}

	/// Check user balance (placeholder - would query blockchain)
	#[allow(dead_code)]
	pub async fn check_balance(
		&self,
		_user_id: &str,
		_market: &str,
		_required: u64,
	) -> Result<(), AdmissionError> {
		// TODO: Query blockchain for user balance
		// For now, assume sufficient balance
		Ok(())
	}
}

/// Global admission controller instance
static ADMISSION_CONTROLLER: std::sync::OnceLock<AdmissionController> = std::sync::OnceLock::new();

fn get_admission_controller() -> &'static AdmissionController {
	ADMISSION_CONTROLLER.get_or_init(|| AdmissionController::new(100, 200))
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
		if let Some(price) = request.price
			&& price == 0
		{
			return Err(AdmissionError::InvalidOrder(
				"Price must be greater than zero".to_string(),
			));
		}
	}

	// Check market availability
	if !get_admission_controller().is_market_available(&request.market) {
		return Err(AdmissionError::MarketNotAvailable(request.market.clone()));
	}

	// Rate limiting is checked per user in the handler
	// Balance checking would be async and done in handler

	Ok(())
}

/// Check rate limit for a user
pub fn check_rate_limit(user_id: &str) -> Result<(), AdmissionError> {
	get_admission_controller().check_rate_limit(user_id)
}

/// Check user balance (async)
#[allow(dead_code)]
pub async fn check_balance(
	user_id: &str,
	market: &str,
	required: u64,
) -> Result<(), AdmissionError> {
	get_admission_controller()
		.check_balance(user_id, market, required)
		.await
}
