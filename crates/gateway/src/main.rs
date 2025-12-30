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

//! Order Gateway Service
//!
//! This service handles client order submission, performs cryptographic
//! authentication and protocol-level validation, and routes orders to the
//! appropriate matching engine.
//!
//! # Identity Model
//!
//! Gateway only understands **cryptographic identity** (public keys and signatures),
//! NOT business user identity (user accounts, KYC, profiles, etc.).
//!
//! - Gateway verifies that orders are signed by the holder of a private key
//! - Gateway performs rate limiting at the cryptographic principal level (public key)
//! - Gateway does NOT understand user accounts, user IDs, or business-level identity
//!
//! This design ensures Gateway remains infrastructure-focused and does not
//! become entangled with business logic.

mod admission;
mod auth;
mod config;
mod grpc_client;
mod handlers;
mod logging;
mod middleware;
mod router;
mod server;

use std::net::SocketAddr;

use anyhow::{Context, Result};
use tracing::info;

use crate::{config::DEFAULT_BIND_ADDR, logging::init_logging};
use server::GatewayServer;

#[actix_rt::main]
async fn main() -> Result<()> {
	// Initialize logging first
	init_logging()?;

	// Get bind address from environment or use default
	let bind_addr_str =
		std::env::var("GATEWAY_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
	let addr: SocketAddr = bind_addr_str
		.parse()
		.with_context(|| format!("Invalid bind address: {}", bind_addr_str))?;
	info!(target: "server", "Starting Anvil Gateway on {}", addr);

	let server = GatewayServer::new()
		.await
		.context("Failed to create gateway server")?;

	info!(target: "server", "Gateway server initialized");

	server
		.serve(addr)
		.await
		.context("Failed to start gateway server")?;

	Ok(())
}
