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

//! Settlement Core Service
//!
//! This service validates matched trades, constructs chain-specific
//! transactions, and submits them to the blockchain for final settlement.

use anvil_settlement::submitter::SettlementSubmitter;
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::RwLock;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<()> {
	// Initialize tracing
	tracing_subscriber::fmt()
		.with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
		.init();

	// Get configuration
	let addr: SocketAddr = std::env::var("SETTLEMENT_ADDR")
		.ok()
		.and_then(|s| s.parse().ok())
		.unwrap_or_else(|| "0.0.0.0:50052".parse().unwrap());

	tracing::info!("Starting Anvil Settlement Core");
	tracing::info!("Listening on: {}", addr);

	// Initialize settlement submitter
	let submitter = Arc::new(RwLock::new(
		SettlementSubmitter::new()
			.await
			.context("Failed to initialize settlement submitter")?,
	));

	// Create gRPC server
	let settlement_service = anvil_settlement::server::create_server(submitter.clone());

	// Start gRPC server
	let server = Server::builder()
		.add_service(settlement_service)
		.serve(addr);

	// Wait for shutdown signal
	tokio::select! {
		result = server => {
			result.context("gRPC server error")?;
			tracing::info!("gRPC server stopped");
		}
		_ = signal::ctrl_c() => {
			tracing::info!("Shutting down...");
		}
	}

	Ok(())
}
