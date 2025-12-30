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

use actix_web::web;

use crate::handlers;

/// Configure API routes for the gateway
///
/// This function sets up all HTTP routes for the gateway service:
/// - `/api/v1/orders` - Order management endpoints
/// - `/health` - Health check endpoint
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
	cfg.service(
		web::scope("/api/v1")
			.route("/orders", web::post().to(handlers::place_order))
			.route("/orders/{order_id}", web::get().to(handlers::get_order))
			.route(
				"/orders/{order_id}",
				web::delete().to(handlers::cancel_order),
			),
	)
	.route("/health", web::get().to(handlers::health));
}
