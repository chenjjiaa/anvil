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

use crate::admission::{AdmissionError, validate_and_admit};
use crate::auth::{AuthError, authenticate_order, extract_user_id};
use crate::router::{Router, RouterError};
use anvil_sdk::types::{OrderStatus, PlaceOrderRequest, PlaceOrderResponse};
use axum::{
	Json, Router as AxumRouter, extract::State, http::StatusCode, response::IntoResponse,
	routing::post,
};
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;

/// Error types for gateway operations
#[derive(Debug, Error)]
pub enum GatewayError {
	#[error("Authentication error: {0}")]
	Auth(#[from] AuthError),
	#[error("Admission error: {0}")]
	Admission(#[from] AdmissionError),
	#[error("Routing error: {0}")]
	Routing(#[from] RouterError),
	#[error("Internal error: {0}")]
	Internal(String),
}

impl IntoResponse for GatewayError {
	fn into_response(self) -> axum::response::Response {
		let (status, error_message) = match self {
			GatewayError::Auth(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
			GatewayError::Admission(_) => (StatusCode::BAD_REQUEST, self.to_string()),
			GatewayError::Routing(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
			GatewayError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
		};

		(status, Json(serde_json::json!({ "error": error_message }))).into_response()
	}
}

/// Gateway server state
#[derive(Clone)]
struct GatewayState {
	router: Arc<Router>,
}

/// Gateway server
pub struct GatewayServer {
	state: GatewayState,
}

impl GatewayServer {
	/// Create a new gateway server
	pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
		let router = Arc::new(Router::new());
		Ok(Self {
			state: GatewayState { router },
		})
	}

	/// Start the HTTP server
	pub async fn serve(&self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
		let app = AxumRouter::new()
			.route("/api/v1/orders", post(place_order_handler))
			.with_state(self.state.clone());

		let listener = tokio::net::TcpListener::bind(addr).await?;
		println!("Gateway listening on {}", addr);

		axum::serve(listener, app).await?;
		Ok(())
	}
}

/// Handle order placement request
async fn place_order_handler(
	State(state): State<GatewayState>,
	Json(request): Json<PlaceOrderRequest>,
) -> Result<Json<PlaceOrderResponse>, GatewayError> {
	// Extract user ID (placeholder - in production, extract from auth token)
	let user_id = extract_user_id(&request);

	// Authenticate the order
	// TODO: Get public key from request or auth token
	let public_key = b"placeholder_public_key";
	authenticate_order(&request, public_key)?;

	// Validate and admit the order
	validate_and_admit(&request)?;

	// Route to matching engine
	let matching_order = state.router.route_order(request.clone(), user_id)?;

	// TODO: Actually send to matching engine and wait for response
	// For now, return a placeholder response
	Ok(Json(PlaceOrderResponse {
		order_id: matching_order.order_id,
		status: OrderStatus::Accepted,
		client_order_id: None,
	}))
}
