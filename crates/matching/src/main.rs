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

use anvil_matching::Matcher;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// Initialize the matching engine
	let mut matcher = Matcher::new();

	// TODO: Set up gRPC or message queue to receive orders from gateway
	// TODO: Set up channel to send matched trades to settlement core

	println!("Anvil Matching Engine started");
	println!("Market: TODO - configure via environment or CLI");

	// Wait for shutdown signal
	signal::ctrl_c().await?;
	println!("Shutting down...");

	Ok(())
}
