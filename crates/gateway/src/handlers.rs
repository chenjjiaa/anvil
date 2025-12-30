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

use actix_web::{HttpRequest, HttpResponse, Responder, web};
use anvil_sdk::types::{OrderStatus, PlaceOrderRequest, PlaceOrderResponse};
use thiserror::Error;

use crate::{
	admission,
	admission::AdmissionError,
	auth,
	auth::{AuthContext, AuthError},
	router::RouterError,
	server::GatewayState,
};

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

impl actix_web::ResponseError for GatewayError {
	fn error_response(&self) -> HttpResponse {
		let status = match self {
			GatewayError::Auth(_) => actix_web::http::StatusCode::UNAUTHORIZED,
			GatewayError::Admission(_) => actix_web::http::StatusCode::BAD_REQUEST,
			GatewayError::Routing(_) => actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
			GatewayError::Internal(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
		};

		HttpResponse::build(status).json(serde_json::json!({
			"error": self.to_string()
		}))
	}
}

/// Health check endpoint
pub async fn health() -> impl Responder {
	HttpResponse::Ok().json(serde_json::json!({
		"status": "ok",
		"service": "anvil-gateway"
	}))
}

/// Handle order placement request
///
/// Gateway performs cryptographic authentication and protocol-level admission control.
/// It does NOT understand business user identity - all operations are based on
/// cryptographic principals (public keys).
///
/// # Authentication Model (Protocol Requirement)
///
/// **Authentication materials MUST be in HTTP headers, NOT in request body.**
///
/// This is a hard protocol requirement:
/// - Public key: `X-Public-Key` header
/// - Signature: `X-Signature` header
///
/// The order payload (`PlaceOrderRequest`) contains ONLY business data
/// (market, price, size, etc.), NEVER authentication materials.
pub async fn place_order(
	state: web::Data<GatewayState>,
	request: web::Json<PlaceOrderRequest>,
	req: HttpRequest,
) -> Result<HttpResponse, GatewayError> {
	// Construct AuthContext from HTTP request
	// Authentication materials MUST be in headers per protocol specification
	let auth_ctx = AuthContext::from_http(req.headers());

	// Extract principal using AuthProvider
	// This extracts the public key and signature from headers/metadata,
	// creates a Principal, and verifies the signature against the order payload
	let principal =
		auth::authenticate_with_provider(&auth_ctx, &request, state.auth_provider.as_ref())
			.map_err(GatewayError::Auth)?;

	// Check rate limit by principal (public key)
	// Gateway only performs rate limiting at the cryptographic principal level,
	// not at the business user level.
	admission::check_rate_limit(&principal)?;

	// Validate and admit the order (protocol-level checks)
	admission::validate_and_admit(&request)?;

	// Route to matching engine (use principal.id() as identifier)
	// Note: principal.id() returns hex-encoded public key, which is passed
	// to matching engine as the principal identifier (not a business user ID).
	let matching_order = state
		.router
		.route_order(request.into_inner(), principal.id())
		.await?;

	Ok(HttpResponse::Ok().json(PlaceOrderResponse {
		order_id: matching_order.order_id,
		status: OrderStatus::Accepted,
		client_order_id: None,
	}))
}

/// Handle order query request
pub async fn get_order(
	_state: web::Data<GatewayState>,
	path: web::Path<String>,
) -> Result<HttpResponse, GatewayError> {
	let order_id = path.into_inner();

	// TODO: Query order from matching engine via gRPC
	Err(GatewayError::Internal(format!(
		"Order query not yet implemented: {}",
		order_id
	)))
}

/// Handle order cancellation request
pub async fn cancel_order(
	_state: web::Data<GatewayState>,
	path: web::Path<String>,
) -> Result<HttpResponse, GatewayError> {
	let order_id = path.into_inner();

	// TODO: Cancel order via matching engine gRPC
	Err(GatewayError::Internal(format!(
		"Order cancellation not yet implemented: {}",
		order_id
	)))
}
