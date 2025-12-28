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

use crate::transaction::Chain;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;

/// Settlement service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementConfig {
	/// gRPC server bind address
	pub bind_addr: SocketAddr,
	/// Chain-specific RPC endpoints
	pub rpc_endpoints: HashMap<Chain, String>,
	/// Transaction confirmation requirements
	pub confirmation_requirements: HashMap<Chain, u64>,
}

impl Default for SettlementConfig {
	fn default() -> Self {
		let mut rpc_endpoints = HashMap::new();
		rpc_endpoints.insert(Chain::Solana, "http://localhost:8899".to_string());
		rpc_endpoints.insert(Chain::Ethereum, "http://localhost:8545".to_string());

		let mut confirmations = HashMap::new();
		confirmations.insert(Chain::Solana, 1);
		confirmations.insert(Chain::Ethereum, 12);

		Self {
			bind_addr: "0.0.0.0:50052".parse().unwrap(),
			rpc_endpoints,
			confirmation_requirements: confirmations,
		}
	}
}

impl SettlementConfig {
	/// Load configuration from environment variables
	pub fn from_env() -> Result<Self, config::ConfigError> {
		let cfg = config::Config::builder()
			.add_source(config::Environment::with_prefix("SETTLEMENT"))
			.build()?;

		cfg.try_deserialize()
	}

	/// Load configuration from file
	pub fn from_file(path: &str) -> Result<Self, config::ConfigError> {
		let cfg = config::Config::builder()
			.add_source(config::File::with_name(path))
			.add_source(config::Environment::with_prefix("SETTLEMENT"))
			.build()?;

		cfg.try_deserialize()
	}
}
