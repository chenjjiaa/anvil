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

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Matching engine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchingConfig {
	/// gRPC server bind address
	pub bind_addr: SocketAddr,
	/// Market identifier
	pub market: String,
	/// Settlement service endpoint
	pub settlement_endpoint: String,
}

impl Default for MatchingConfig {
	fn default() -> Self {
		Self {
			bind_addr: "0.0.0.0:50051".parse().unwrap(),
			market: "BTC-USDT".to_string(),
			settlement_endpoint: "http://localhost:50052".to_string(),
		}
	}
}

impl MatchingConfig {
	/// Load configuration from environment variables
	pub fn from_env() -> Result<Self, config::ConfigError> {
		let cfg = config::Config::builder()
			.add_source(config::Environment::with_prefix("MATCHING"))
			.build()?;

		cfg.try_deserialize()
	}

	/// Load configuration from file
	pub fn from_file(path: &str) -> Result<Self, config::ConfigError> {
		let cfg = config::Config::builder()
			.add_source(config::File::with_name(path))
			.add_source(config::Environment::with_prefix("MATCHING"))
			.build()?;

		cfg.try_deserialize()
	}
}
