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

//! Protocol-level admission control for Gateway
//!
//! This module performs protocol-level admission checks:
//! - Rate limiting per cryptographic principal (public key)
//! - Market availability checks
//! - Order format validation
//!
//! # Rate Limiting Model
//!
//! Gateway only performs rate limiting at the **cryptographic principal level**,
//! NOT at the business user level. This means:
//!
//! - Each public key has its own rate limit
//! - A single entity can use multiple public keys (this is intentional)
//! - User-level rate limiting should be handled by the application layer
//!
//! Gateway does NOT perform:
//! - User account balance checks (handled by settlement service)
//! - Business-level user identity verification
//! - KYC or compliance checks

use std::{
	num::NonZeroU32,
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
	},
	time::{Duration, Instant},
};

use anvil_sdk::types::{OrderType, PlaceOrderRequest};
use dashmap::DashMap;
use governor::{Quota, RateLimiter};
use moka::sync::Cache;
use thiserror::Error;

use crate::{
	auth::Principal,
	config::{
		DEFAULT_NONCE_TTL_SECS, DEFAULT_RATE_LIMIT_BURST, DEFAULT_RATE_LIMIT_RPS,
		DEFAULT_REPLAY_CACHE_MAX_CAPACITY, DEFAULT_REPLAY_WINDOW_SECS,
	},
};

/// Error types for admission control
#[derive(Debug, Error)]
pub enum AdmissionError {
	#[error("Invalid order: {0}")]
	InvalidOrder(String),
	#[error("Rate limit exceeded")]
	RateLimitExceeded,
	#[error("Replay detected")]
	ReplayDetected,
	#[error("Timestamp outside allowed window")]
	TimestampOutsideWindow,
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

/// Rate limiter per principal (public key)
///
/// Gateway only performs rate limiting at the cryptographic principal level,
/// not at the business user level. This is because Gateway only understands
/// cryptographic identity (public keys), not business user identity.
type PrincipalRateLimiter = Arc<
	RateLimiter<
		governor::state::direct::NotKeyed,
		governor::state::InMemoryState,
		governor::clock::DefaultClock,
	>,
>;

/// Admission controller
///
/// Performs protocol-level admission control:
/// - Rate limiting per cryptographic principal (public key)
/// - Market availability checks
/// - Order format validation
///
/// Note: Gateway does NOT perform user-level rate limiting or balance checks.
/// Those are business logic concerns that belong in the application layer.
pub struct AdmissionController {
	/// Rate limiters per principal (public key)
	/// Future: move per-principal rate limiters into a bounded cache (e.g. moka)
	rate_limiters: DashMap<String, PrincipalRateLimiter>,
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
		let quota = Quota::per_second(
			NonZeroU32::new(requests_per_second).expect("GATEWAY_RATE_LIMIT_RPS must be > 0"),
		)
		.allow_burst(NonZeroU32::new(burst).expect("GATEWAY_RATE_LIMIT_BURST must be > 0"));
		Self {
			rate_limiters: DashMap::new(),
			markets: Arc::new(DashMap::new()),
			quota,
			burst,
		}
	}

	/// Check rate limit for a principal (public key)
	///
	/// Gateway only performs rate limiting at the cryptographic principal level.
	/// This prevents abuse by individual public keys, but does not prevent
	/// a single entity from using multiple public keys to bypass limits.
	///
	/// This is intentional - Gateway only understands cryptographic identity,
	/// not business user identity. User-level rate limiting should be handled
	/// by the application layer if needed.
	pub fn check_rate_limit(&self, principal: &Principal) -> Result<(), AdmissionError> {
		let principal_id = principal.id();
		let limiter = self
			.rate_limiters
			.entry(principal_id)
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

	/// Check balance (placeholder - would query blockchain)
	///
	/// Note: This function is intentionally minimal. Balance checking is
	/// business logic that should be handled by the application layer or
	/// settlement service, not by Gateway. Gateway only performs protocol-level
	/// admission control.
	#[allow(dead_code)]
	pub async fn check_balance(&self, _market: &str, _required: u64) -> Result<(), AdmissionError> {
		// TODO: Query blockchain for balance if needed
		// For now, assume sufficient balance
		// In production, this should be handled by settlement service
		Ok(())
	}
}

/// Global admission controller instance
static ADMISSION_CONTROLLER: std::sync::OnceLock<AdmissionController> = std::sync::OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub enum ReplayOutcome {
	RetryableFailure,
	Terminal,
}

#[derive(Debug, Clone, Copy)]
enum ReplayState {
	InFlight = 0,
	RetryableReady = 1,
	Terminal = 2,
}

impl ReplayState {
	fn from_u8(value: u8) -> Self {
		match value {
			1 => ReplayState::RetryableReady,
			2 => ReplayState::Terminal,
			_ => ReplayState::InFlight,
		}
	}
}

#[derive(Debug)]
struct ReplayEntry {
	state: AtomicU8,
	retry_consumed: AtomicBool,
	token: AtomicU64,
}

impl ReplayEntry {
	fn new(token: u64) -> Self {
		Self {
			state: AtomicU8::new(ReplayState::InFlight as u8),
			retry_consumed: AtomicBool::new(false),
			token: AtomicU64::new(token),
		}
	}
}

#[derive(Debug, Clone)]
pub struct ReplayGuard {
	entry: Arc<ReplayEntry>,
}

impl ReplayGuard {
	pub fn finish(self, outcome: ReplayOutcome) {
		match outcome {
			ReplayOutcome::RetryableFailure => {
				if self.entry.retry_consumed.load(Ordering::SeqCst) {
					self.entry
						.state
						.store(ReplayState::Terminal as u8, Ordering::SeqCst);
				} else {
					self.entry
						.state
						.store(ReplayState::RetryableReady as u8, Ordering::SeqCst);
				}
			}
			ReplayOutcome::Terminal => {
				self.entry
					.state
					.store(ReplayState::Terminal as u8, Ordering::SeqCst);
			}
		}
	}
}

/// Best-effort replay cache (per gateway instance, short-lived).
///
/// Keyed by `(principal_id, nonce)` tuple with a TTL. This is NOT meant to provide
/// global uniqueness across multiple gateway instances.
///
/// This implementation uses `moka::sync::Cache` to provide:
/// - **Bounded memory**: Strict entry limit via `max_capacity`
/// - **Native TTL**: Automatic expiration via `time_to_live`
/// - **Atomic semantics**: Stronger concurrency guarantees than manual check-then-insert
/// - **Structured keys**: `(String, String)` tuple instead of string concatenation
///
/// The cache is intended as a best-effort, short-lived admission control mechanism
/// at the gateway layer, not as a source of global idempotency.
struct ReplayCache {
	cache: Cache<(String, String), Arc<ReplayEntry>>,
	replay_window_secs: u64,
	next_token: AtomicU64,
}

impl ReplayCache {
	fn new(replay_window_secs: u64, nonce_ttl_secs: u64, max_capacity: u64) -> Self {
		let cache = Cache::builder()
			.max_capacity(max_capacity)
			.time_to_live(Duration::from_secs(nonce_ttl_secs))
			.build();

		Self {
			cache,
			replay_window_secs,
			next_token: AtomicU64::new(1),
		}
	}

	fn check_timestamp(&self, timestamp: u64) -> Result<(), AdmissionError> {
		let now_secs = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_err(|_| AdmissionError::TimestampOutsideWindow)?
			.as_secs();

		let window = self.replay_window_secs;
		if timestamp + window < now_secs || timestamp > now_secs + window {
			return Err(AdmissionError::TimestampOutsideWindow);
		}

		Ok(())
	}

	fn begin(
		&self,
		principal_id: &str,
		timestamp: u64,
		nonce: &str,
	) -> Result<ReplayGuard, AdmissionError> {
		self.check_timestamp(timestamp)?;
		let key = (principal_id.to_string(), nonce.to_string());
		let token = self.next_token.fetch_add(1, Ordering::Relaxed);

		let entry = self
			.cache
			.get_with(key, || Arc::new(ReplayEntry::new(token)));

		// If we created the entry, allow immediately.
		if entry.token.load(Ordering::Relaxed) == token {
			return Ok(ReplayGuard { entry });
		}

		let state = ReplayState::from_u8(entry.state.load(Ordering::SeqCst));

		match state {
			ReplayState::InFlight => Err(AdmissionError::ReplayDetected),
			ReplayState::Terminal => Err(AdmissionError::ReplayDetected),
			ReplayState::RetryableReady => {
				// Allow a single retry after a retryable failure.
				let already_used = entry.retry_consumed.swap(true, Ordering::SeqCst);
				if already_used {
					Err(AdmissionError::ReplayDetected)
				} else {
					entry
						.state
						.store(ReplayState::InFlight as u8, Ordering::SeqCst);
					Ok(ReplayGuard { entry })
				}
			}
		}
	}
}

static REPLAY_CACHE: std::sync::OnceLock<ReplayCache> = std::sync::OnceLock::new();

fn get_admission_controller() -> &'static AdmissionController {
	ADMISSION_CONTROLLER.get_or_init(|| {
		let rps = std::env::var("GATEWAY_RATE_LIMIT_RPS")
			.map(|v| {
				v.parse::<u32>()
					.expect("GATEWAY_RATE_LIMIT_RPS must be a valid u32")
			})
			.unwrap_or(DEFAULT_RATE_LIMIT_RPS);
		let burst = std::env::var("GATEWAY_RATE_LIMIT_BURST")
			.map(|v| {
				v.parse::<u32>()
					.expect("GATEWAY_RATE_LIMIT_BURST must be a valid u32")
			})
			.unwrap_or(DEFAULT_RATE_LIMIT_BURST);

		AdmissionController::new(rps, burst)
	})
}

fn get_replay_cache() -> &'static ReplayCache {
	REPLAY_CACHE.get_or_init(|| {
		let replay_window_secs = std::env::var("GATEWAY_REPLAY_WINDOW_SECS")
			.map(|v| {
				v.parse::<u64>()
					.expect("GATEWAY_REPLAY_WINDOW_SECS must be a valid u64")
			})
			.unwrap_or(DEFAULT_REPLAY_WINDOW_SECS);
		let nonce_ttl_secs = std::env::var("GATEWAY_NONCE_TTL_SECS")
			.map(|v| {
				v.parse::<u64>()
					.expect("GATEWAY_NONCE_TTL_SECS must be a valid u64")
			})
			.unwrap_or(DEFAULT_NONCE_TTL_SECS);

		let max_capacity = std::env::var("GATEWAY_REPLAY_CACHE_MAX_CAPACITY")
			.map(|v| {
				v.parse::<u64>()
					.expect("GATEWAY_REPLAY_CACHE_MAX_CAPACITY must be a valid u64")
			})
			.unwrap_or(DEFAULT_REPLAY_CACHE_MAX_CAPACITY);

		ReplayCache::new(replay_window_secs, nonce_ttl_secs, max_capacity)
	})
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

	// Rate limiting is checked per principal in the handler
	// Balance checking would be async and done in handler if needed

	Ok(())
}

/// Check rate limit for a principal (public key)
///
/// Gateway only performs rate limiting at the cryptographic principal level.
/// This is protocol-level protection, not business-level user protection.
pub fn check_rate_limit(principal: &Principal) -> Result<(), AdmissionError> {
	get_admission_controller().check_rate_limit(principal)
}

/// Begin replay tracking for `(principal, nonce)`.
///
/// This records the request as in-flight. If the nonce is already in-flight or
/// terminal, it returns `ReplayDetected`. If the previous attempt ended with a
/// retryable failure and the retry has not been consumed yet, it will allow one
/// more attempt and mark it as in-flight again.
pub fn begin_replay(
	principal: &Principal,
	timestamp: u64,
	nonce: &str,
) -> Result<ReplayGuard, AdmissionError> {
	let cache = get_replay_cache();
	cache.begin(&principal.id(), timestamp, nonce)
}

/// Check balance (async)
///
/// Note: This is intentionally minimal. Balance checking is business logic
/// that should be handled by the application layer or settlement service.
#[allow(dead_code)]
pub async fn check_balance(market: &str, required: u64) -> Result<(), AdmissionError> {
	get_admission_controller()
		.check_balance(market, required)
		.await
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::auth::{Principal, SignatureAlgorithm};

	fn principal() -> Principal {
		Principal::new(vec![0u8; 32], SignatureAlgorithm::Ed25519)
	}

	fn now_secs() -> u64 {
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs()
	}

	#[test]
	fn replay_rejects_duplicate_inflight() {
		let cache = ReplayCache::new(30, 60, 100);
		let principal = principal();

		let guard = cache
			.begin(&principal.id(), now_secs(), "nonce-1")
			.expect("first attempt allowed");
		assert!(cache.begin(&principal.id(), now_secs(), "nonce-1").is_err());

		guard.finish(ReplayOutcome::Terminal);
		assert!(matches!(
			cache.begin(&principal.id(), now_secs(), "nonce-1"),
			Err(AdmissionError::ReplayDetected)
		));
	}

	#[test]
	fn replay_allows_single_retry_after_retryable_failure() {
		let cache = ReplayCache::new(30, 60, 100);
		let principal = principal();

		let guard1 = cache
			.begin(&principal.id(), now_secs(), "nonce-2")
			.expect("first attempt allowed");
		guard1.finish(ReplayOutcome::RetryableFailure);

		let guard2 = cache
			.begin(&principal.id(), now_secs(), "nonce-2")
			.expect("retry allowed");
		guard2.finish(ReplayOutcome::RetryableFailure);

		assert!(matches!(
			cache.begin(&principal.id(), now_secs(), "nonce-2"),
			Err(AdmissionError::ReplayDetected)
		));
	}
}
