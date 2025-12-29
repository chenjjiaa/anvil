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

use std::collections::HashMap;
use std::net::SocketAddr;

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
			matching_engines: {
				let mut map = HashMap::new();
				map.insert("BTC-USDT".to_string(), "http://localhost:50051".to_string());
				map
			},
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
