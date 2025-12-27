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

//! Settlement Core Service
//!
//! This service validates matched trades, constructs chain-specific
//! transactions, and submits them to the blockchain for final settlement.

mod submitter;
mod transaction;
mod validator;

use submitter::SettlementSubmitter;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	println!("Starting Anvil Settlement Core");

	// Initialize settlement submitter
	let submitter = SettlementSubmitter::new().await?;

	// TODO: Set up channel to receive matched trades from matching engine
	// TODO: Process trades, validate, construct transactions, and submit

	println!("Settlement core ready");

	// Wait for shutdown signal
	signal::ctrl_c().await?;
	println!("Shutting down...");

	Ok(())
}
