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

use std::{collections::HashMap, env, net::SocketAddr};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// Logging configuration constants
/// Default log level (can be overridden by RUST_LOG environment variable)
pub const DEFAULT_LOG_LEVEL: &str = "info";

/// Default log directory component name
pub const LOG_COMPONENT_NAME: &str = "gateway";

/// Default console output enabled (can be overridden by LOG_TO_CONSOLE environment variable)
pub const DEFAULT_LOG_TO_CONSOLE: bool = false;

// Server configuration constants
/// Default HTTP server bind address (can be overridden by GATEWAY_BIND_ADDR environment variable)
pub const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

// Admission / anti-abuse configuration constants
/// Default requests-per-second limit per principal (can be overridden by GATEWAY_RATE_LIMIT_RPS)
pub const DEFAULT_RATE_LIMIT_RPS: u32 = 100;

/// Default burst capacity per principal (can be overridden by GATEWAY_RATE_LIMIT_BURST)
pub const DEFAULT_RATE_LIMIT_BURST: u32 = 200;

/// Default replay window in seconds (can be overridden by GATEWAY_REPLAY_WINDOW_SECS)
pub const DEFAULT_REPLAY_WINDOW_SECS: u64 = 30;

/// Default nonce TTL in seconds (can be overridden by GATEWAY_NONCE_TTL_SECS)
pub const DEFAULT_NONCE_TTL_SECS: u64 = 120;

/// Default replay cache maximum capacity in entries (can be overridden by GATEWAY_REPLAY_CACHE_MAX_CAPACITY)
/// This provides a strict upper bound on memory usage for replay protection.
pub const DEFAULT_REPLAY_CACHE_MAX_CAPACITY: u64 = 1_000_000;

/// Default maximum HTTP request body size in bytes (can be overridden by GATEWAY_MAX_BODY_BYTES)
pub const DEFAULT_MAX_BODY_BYTES: usize = 64 * 1024;

/// Default matching-engine RPC timeout in milliseconds (can be overridden by GATEWAY_MATCHING_RPC_TIMEOUT_MS)
pub const DEFAULT_MATCHING_RPC_TIMEOUT_MS: u64 = 1_500;

/// Default dispatch queue capacity for the matching dispatcher
pub const DEFAULT_DISPATCH_QUEUE_CAPACITY: usize = 1_024;

/// Default dispatch queue timeout (ms) while waiting in bounded queue
pub const DEFAULT_DISPATCH_QUEUE_TIMEOUT_MS: u64 = 1_000;

#[derive(Debug, Clone)]
pub struct GatewayRuntimeConfig {
	pub bind_addr: SocketAddr,
	pub workers: usize,
	pub max_body_bytes: usize,
	pub matching_engines: HashMap<String, String>,
	pub dispatch_queue_capacity: usize,
	pub dispatch_queue_timeout_ms: u64,
	pub matching_rpc_timeout_ms: u64,
}

impl GatewayRuntimeConfig {
	pub fn from_env() -> Result<Self> {
		dotenv::dotenv().ok();

		let bind_addr_str =
			env::var("GATEWAY_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
		let bind_addr = bind_addr_str
			.parse()
			.with_context(|| format!("Invalid bind address: {}", bind_addr_str))?;

		let workers = env::var("GATEWAY_WORKERS")
			.ok()
			.and_then(|w| w.parse().ok())
			.unwrap_or_else(num_cpus::get);

		let max_body_bytes = env::var("GATEWAY_MAX_BODY_BYTES")
			.ok()
			.and_then(|v| v.parse().ok())
			.unwrap_or(DEFAULT_MAX_BODY_BYTES);

		let matching_rpc_timeout_ms = env::var("GATEWAY_MATCHING_RPC_TIMEOUT_MS")
			.ok()
			.and_then(|v| v.parse().ok())
			.unwrap_or(DEFAULT_MATCHING_RPC_TIMEOUT_MS);

		let dispatch_queue_capacity = env::var("GATEWAY_DISPATCH_QUEUE_CAPACITY")
			.ok()
			.and_then(|v| v.parse().ok())
			.unwrap_or(DEFAULT_DISPATCH_QUEUE_CAPACITY);

		let dispatch_queue_timeout_ms = env::var("GATEWAY_DISPATCH_QUEUE_TIMEOUT_MS")
			.ok()
			.and_then(|v| v.parse().ok())
			.unwrap_or(DEFAULT_DISPATCH_QUEUE_TIMEOUT_MS);

		let matching_engines = default_matching_engines();

		Ok(Self {
			bind_addr,
			workers,
			max_body_bytes,
			matching_engines,
			dispatch_queue_capacity,
			dispatch_queue_timeout_ms,
			matching_rpc_timeout_ms,
		})
	}
}

fn default_matching_engines() -> HashMap<String, String> {
	let mut map = HashMap::new();
	// TODO: Load from configuration file or service discovery.
	map.insert("BTC-USDT".to_string(), "http://localhost:50051".to_string());
	map
}

/// Gateway service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct GatewayConfig {
	/// HTTP server bind address
	pub bind_addr: SocketAddr,
	/// Number of worker threads
	pub workers: Option<usize>,
	/// Matching engine endpoints (market -> endpoint)
	pub matching_engines: HashMap<String, String>,
	/// Rate limiting configuration
	pub rate_limit: RateLimitConfig,
	/// Authentication configuration
	pub auth: AuthConfig,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct RateLimitConfig {
	/// Requests per second per user
	pub requests_per_second: u32,
	/// Burst capacity
	pub burst: u32,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct AuthConfig {
	/// Require signature verification
	pub require_signature: bool,
	/// Supported signature algorithms
	pub supported_algorithms: Vec<String>,
}

impl Default for GatewayConfig {
	fn default() -> Self {
		Self {
			bind_addr: "0.0.0.0:8080".parse().unwrap(),
			workers: None,
			matching_engines: default_matching_engines(),
			rate_limit: RateLimitConfig {
				requests_per_second: 100,
				burst: 200,
			},
			auth: AuthConfig {
				require_signature: true,
				supported_algorithms: vec!["ed25519".to_string(), "ecdsa".to_string()],
			},
		}
	}
}

impl GatewayConfig {
	/// Load configuration from environment variables
	#[allow(dead_code)]
	pub fn from_env() -> Result<Self, config::ConfigError> {
		let cfg = config::Config::builder()
			.add_source(config::Environment::with_prefix("GATEWAY"))
			.build()?;

		cfg.try_deserialize()
	}

	/// Load configuration from file
	#[allow(dead_code)]
	pub fn from_file(path: &str) -> Result<Self, config::ConfigError> {
		let cfg = config::Config::builder()
			.add_source(config::File::with_name(path))
			.add_source(config::Environment::with_prefix("GATEWAY"))
			.build()?;

		cfg.try_deserialize()
	}
}
