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

use crate::handlers;
use crate::middleware;
use actix_web::{App, HttpServer, web};
use std::net::SocketAddr;
use std::sync::Arc;

/// Gateway server state
#[derive(Clone)]
pub struct GatewayState {
	pub router: Arc<crate::router::Router>,
}

/// Gateway server
pub struct GatewayServer {
	state: GatewayState,
}

impl GatewayServer {
	/// Create a new gateway server
	pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
		let router = Arc::new(crate::router::Router::new());
		Ok(Self {
			state: GatewayState { router },
		})
	}

	/// Start the HTTP server with actix-web
	pub async fn serve(&self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
		let state = self.state.clone();

		// Get number of workers from environment or use CPU count
		let workers = std::env::var("GATEWAY_WORKERS")
			.ok()
			.and_then(|w| w.parse().ok())
			.unwrap_or_else(num_cpus::get);

		tracing::info!(
			"Starting Anvil Gateway on {} with {} workers",
			addr,
			workers
		);

		HttpServer::new(move || {
			App::new()
				.app_data(web::Data::new(state.clone()))
				.wrap(middleware::CorsMiddleware)
				.wrap(middleware::LoggingMiddleware)
				.service(
					web::scope("/api/v1")
						.route("/orders", web::post().to(handlers::place_order))
						.route("/orders/{order_id}", web::get().to(handlers::get_order))
						.route(
							"/orders/{order_id}",
							web::delete().to(handlers::cancel_order),
						),
				)
				.route("/health", web::get().to(handlers::health))
		})
		.workers(workers)
		.bind(addr)?
		.run()
		.await?;

		Ok(())
	}
}
