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

use std::{net::SocketAddr, path::PathBuf};

use serde::{Deserialize, Serialize};

/// Matching engine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchingConfig {
	/// gRPC server bind address
	pub bind_addr: SocketAddr,
	/// Market identifier
	pub market: String,
	/// Settlement service endpoint
	pub settlement_endpoint: String,
	/// Ingress queue capacity
	pub ingress_queue_size: usize,
	/// Event buffer capacity
	pub event_buffer_size: usize,
	/// Event writer batch size
	pub event_batch_size: usize,
	/// Event writer batch timeout (milliseconds)
	pub event_batch_timeout_ms: u64,
	/// Snapshot interval (seconds)
	pub snapshot_interval_secs: u64,
	/// Maximum snapshots to keep
	pub max_snapshots_to_keep: usize,
	/// Journal path (optional, for future file-based journal)
	pub journal_path: Option<PathBuf>,
	/// Event storage path (optional, for future file-based events)
	pub event_storage_path: Option<PathBuf>,
	/// Snapshot path (optional, for future file-based snapshots)
	pub snapshot_path: Option<PathBuf>,
	/// Enable verbose logging
	pub verbose_logging: bool,
}

impl Default for MatchingConfig {
	fn default() -> Self {
		Self {
			bind_addr: "0.0.0.0:50051".parse().unwrap(),
			market: "BTC-USDT".to_string(),
			settlement_endpoint: "http://localhost:50052".to_string(),
			ingress_queue_size: 10000,
			event_buffer_size: 10000,
			event_batch_size: 100,
			event_batch_timeout_ms: 100,
			snapshot_interval_secs: 300,
			max_snapshots_to_keep: 10,
			journal_path: None,
			event_storage_path: None,
			snapshot_path: None,
			verbose_logging: false,
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
