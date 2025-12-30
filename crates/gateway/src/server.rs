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

use std::{net::SocketAddr, sync::Arc};

use actix_web::{App, HttpServer, web};
use anyhow::Context;

use crate::{
	auth::{AuthProvider, SignatureAuthProvider},
	dispatcher::MatchingDispatcher,
	middleware::{CorsMiddleware, LoggingMiddleware},
	routes,
};

/// Gateway server state
#[derive(Clone)]
pub struct GatewayState {
	pub dispatcher: Arc<MatchingDispatcher>,
	/// Authentication provider
	///
	/// Gateway uses this to extract public keys and verify signatures.
	/// The default implementation is SignatureAuthProvider, but production
	/// systems should provide their own implementation based on their
	/// authentication requirements.
	pub auth_provider: Arc<dyn AuthProvider>,
}

/// Gateway server
pub struct GatewayServer {
	state: GatewayState,
}

impl GatewayServer {
	/// Create a new gateway server
	///
	/// Uses the default SignatureAuthProvider for authentication.
	/// Production systems should create their own AuthProvider implementation
	/// and pass it to GatewayState.
	pub async fn new() -> anyhow::Result<Self> {
		let dispatcher = Arc::new(MatchingDispatcher::new());
		let auth_provider: Arc<dyn AuthProvider> = Arc::new(SignatureAuthProvider);
		Ok(Self {
			state: GatewayState {
				dispatcher,
				auth_provider,
			},
		})
	}

	/// Start the HTTP server with actix-web
	pub async fn serve(&self, addr: SocketAddr) -> anyhow::Result<()> {
		let state = self.state.clone();

		// Get number of workers from environment or use CPU count
		let workers = std::env::var("GATEWAY_WORKERS")
			.ok()
			.and_then(|w| w.parse().ok())
			.unwrap_or_else(num_cpus::get);

		tracing::info!(
			target: "server::server",
			"Starting HTTP server on {} with {} workers",
			addr,
			workers
		);

		HttpServer::new(move || {
			App::new()
				.app_data(web::Data::new(state.clone()))
				.wrap(CorsMiddleware)
				.wrap(LoggingMiddleware)
				.configure(routes::configure_routes)
		})
		.workers(workers)
		.bind(addr)
		.context("Failed to bind to address")?
		.run()
		.await
		.context("HTTP server error")?;

		Ok(())
	}
}
