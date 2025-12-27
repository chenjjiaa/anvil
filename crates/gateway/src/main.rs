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

//! Order Gateway Service
//!
//! This service handles client order submission, performs authentication
//! and validation, and routes orders to the appropriate matching engine.

mod admission;
mod auth;
mod router;
mod server;

use server::GatewayServer;
use std::net::SocketAddr;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let addr = SocketAddr::from(([0, 0, 0, 0], 8080));

	println!("Starting Anvil Gateway on {}", addr);

	let server = GatewayServer::new().await?;
	server.serve(addr).await?;

	Ok(())
}
