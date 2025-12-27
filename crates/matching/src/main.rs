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

//! Matching engine service entry point
//!
//! This binary runs the matching engine service, which receives orders
//! from the gateway and produces matched trades for settlement.

use anvil_matching::{Matcher, server};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::RwLock;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// Initialize tracing
	tracing_subscriber::fmt()
		.with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
		.init();

	// Get configuration from environment
	let addr: SocketAddr = std::env::var("MATCHING_ADDR")
		.ok()
		.and_then(|s| s.parse().ok())
		.unwrap_or_else(|| "0.0.0.0:50051".parse().unwrap());

	let market = std::env::var("MARKET").unwrap_or_else(|_| "BTC-USDT".to_string());

	tracing::info!("Starting Anvil Matching Engine");
	tracing::info!("Market: {}", market);
	tracing::info!("Listening on: {}", addr);

	// Initialize the matching engine
	let matcher = Arc::new(RwLock::new(Matcher::new()));

	// Create gRPC server
	let matching_service = server::create_server(matcher.clone());

	// Start gRPC server
	let server = Server::builder().add_service(matching_service).serve(addr);

	// Wait for shutdown signal
	tokio::select! {
		_ = server => {
			tracing::info!("gRPC server stopped");
		}
		_ = signal::ctrl_c() => {
			tracing::info!("Shutting down...");
		}
	}

	Ok(())
}
