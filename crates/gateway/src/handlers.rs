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

use crate::admission::{AdmissionError, check_rate_limit, validate_and_admit};
use crate::auth::{AuthError, authenticate_order, extract_user_id};
use crate::router::RouterError;
use crate::server::GatewayState;
use actix_web::{HttpResponse, Responder, web};
use anvil_sdk::types::{OrderStatus, PlaceOrderRequest, PlaceOrderResponse};
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
pub async fn place_order(
	state: web::Data<GatewayState>,
	request: web::Json<PlaceOrderRequest>,
) -> Result<HttpResponse, GatewayError> {
	// Extract user ID (placeholder - in production, extract from auth token)
	let user_id = extract_user_id(&request);

	// Check rate limit
	check_rate_limit(&user_id)?;

	// Authenticate the order
	// Try to extract public key from request, fallback to placeholder for now
	let public_key = match crate::auth::extract_public_key(&request) {
		Ok(key) => key,
		Err(_) => {
			// TODO: In production, require public key in request
			// For now, use placeholder (will fail verification)
			b"placeholder_public_key_32_bytes!!".to_vec()
		}
	};

	// Only authenticate if signature is provided
	if !request.signature.is_empty() {
		authenticate_order(&request, &public_key)?;
	}

	// Validate and admit the order
	validate_and_admit(&request)?;

	// Route to matching engine
	let matching_order = state
		.router
		.route_order(request.into_inner(), user_id)
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
