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
use std::fmt;
use thiserror::Error;
use tracing::field;
use uuid::Uuid;

use crate::{
	admission,
	admission::AdmissionError,
	admission::{ReplayGuard, ReplayOutcome},
	auth,
	auth::{AuthContext, AuthError},
	dispatcher::{DispatchResult, DispatcherError},
	request_context::RequestContext,
	server::GatewayState,
};

/// Error types for gateway operations
#[derive(Debug, Error)]
pub enum GatewayErrorKind {
	#[error("Authentication error: {0}")]
	Auth(AuthError),
	#[error("Admission error: {0}")]
	Admission(AdmissionError),
	#[error("Dispatching error: {0}")]
	Dispatching(DispatcherError),
	#[error("Internal error: {0}")]
	Internal(String),
}

#[derive(Debug)]
pub struct GatewayError {
	kind: GatewayErrorKind,
	request_id: String,
}

impl fmt::Display for GatewayError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.kind)
	}
}

#[derive(Debug, Clone, Copy)]
enum Retryability {
	Retryable,
	NonRetryable,
	Unconfirmed,
}

impl GatewayError {
	fn auth(err: AuthError, ctx: &RequestContext) -> Self {
		Self {
			kind: GatewayErrorKind::Auth(err),
			request_id: ctx.request_id.clone(),
		}
	}

	fn admission(err: AdmissionError, ctx: &RequestContext) -> Self {
		Self {
			kind: GatewayErrorKind::Admission(err),
			request_id: ctx.request_id.clone(),
		}
	}

	fn dispatch(err: DispatcherError, ctx: &RequestContext) -> Self {
		Self {
			kind: GatewayErrorKind::Dispatching(err),
			request_id: ctx.request_id.clone(),
		}
	}

	fn internal(msg: impl Into<String>, ctx: &RequestContext) -> Self {
		Self {
			kind: GatewayErrorKind::Internal(msg.into()),
			request_id: ctx.request_id.clone(),
		}
	}
}

impl actix_web::ResponseError for GatewayError {
	fn error_response(&self) -> HttpResponse {
		let (status, code, retryability, reason) = match &self.kind {
			GatewayErrorKind::Auth(e) => (
				actix_web::http::StatusCode::UNAUTHORIZED,
				"AUTH_FAILED",
				Retryability::NonRetryable,
				e.to_string(),
			),
			GatewayErrorKind::Admission(AdmissionError::RateLimitExceeded) => (
				actix_web::http::StatusCode::TOO_MANY_REQUESTS,
				"RATE_LIMITED",
				Retryability::Retryable,
				"Rate limit exceeded".to_string(),
			),
			GatewayErrorKind::Admission(AdmissionError::ReplayDetected) => (
				actix_web::http::StatusCode::BAD_REQUEST,
				"REPLAY_DETECTED",
				Retryability::NonRetryable,
				"Duplicate nonce within replay window".to_string(),
			),
			GatewayErrorKind::Admission(AdmissionError::TimestampOutsideWindow) => (
				actix_web::http::StatusCode::BAD_REQUEST,
				"TIMESTAMP_INVALID",
				Retryability::NonRetryable,
				"Timestamp outside allowed window".to_string(),
			),
			GatewayErrorKind::Admission(AdmissionError::MarketNotAvailable(market)) => (
				actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
				"MARKET_UNAVAILABLE",
				Retryability::NonRetryable,
				format!("Market not available: {}", market),
			),
			GatewayErrorKind::Admission(AdmissionError::InvalidOrder(reason)) => (
				actix_web::http::StatusCode::BAD_REQUEST,
				"INVALID_ORDER",
				Retryability::NonRetryable,
				reason.clone(),
			),
			GatewayErrorKind::Admission(AdmissionError::InsufficientBalance) => (
				actix_web::http::StatusCode::BAD_REQUEST,
				"INSUFFICIENT_BALANCE",
				Retryability::NonRetryable,
				"Insufficient balance".to_string(),
			),
			GatewayErrorKind::Dispatching(DispatcherError::GatewayOverloaded) => (
				actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
				"GATEWAY_OVERLOADED",
				Retryability::Retryable,
				"Gateway dispatch queue is overloaded".to_string(),
			),
			GatewayErrorKind::Dispatching(DispatcherError::QueueTimeout) => (
				actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
				"QUEUE_TIMEOUT",
				Retryability::Retryable,
				"Timed out while waiting for dispatch slot".to_string(),
			),
			GatewayErrorKind::Dispatching(DispatcherError::MatchingTimeout) => (
				actix_web::http::StatusCode::ACCEPTED,
				"UNCONFIRMED",
				Retryability::Unconfirmed,
				"Order may be accepted but confirmation timed out".to_string(),
			),
			GatewayErrorKind::Dispatching(DispatcherError::MatchingOverloaded(reason)) => (
				actix_web::http::StatusCode::ACCEPTED,
				"UNCONFIRMED",
				Retryability::Unconfirmed,
				if reason.is_empty() {
					"Matching engine overloaded".to_string()
				} else {
					reason.clone()
				},
			),
			GatewayErrorKind::Dispatching(DispatcherError::MatchingRejected(reason)) => (
				actix_web::http::StatusCode::BAD_REQUEST,
				"MATCHING_REJECTED",
				Retryability::NonRetryable,
				if reason.is_empty() {
					"Order rejected by matching engine".to_string()
				} else {
					reason.clone()
				},
			),
			GatewayErrorKind::Dispatching(DispatcherError::MatchingInternal(reason)) => (
				actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
				"MATCHING_INTERNAL",
				Retryability::Retryable,
				if reason.is_empty() {
					"Matching engine internal error".to_string()
				} else {
					reason.clone()
				},
			),
			GatewayErrorKind::Dispatching(DispatcherError::MatchingEngineNotFound(market)) => (
				actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
				"NO_MATCHING_ENGINE",
				Retryability::NonRetryable,
				format!("Matching engine not found for market: {}", market),
			),
			GatewayErrorKind::Dispatching(DispatcherError::InvalidResponse(reason))
			| GatewayErrorKind::Dispatching(DispatcherError::DispatchingError(reason)) => (
				actix_web::http::StatusCode::BAD_GATEWAY,
				"DISPATCH_ERROR",
				Retryability::Retryable,
				reason.clone(),
			),
			GatewayErrorKind::Internal(reason) => (
				actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
				"INTERNAL",
				Retryability::NonRetryable,
				reason.clone(),
			),
		};

		let (retryable, unconfirmed) = match retryability {
			Retryability::Retryable => (true, false),
			Retryability::NonRetryable => (false, false),
			Retryability::Unconfirmed => (true, true),
		};

		HttpResponse::build(status).json(serde_json::json!({
			"code": code,
			"reason": reason,
			"retryable": retryable,
			"unconfirmed": unconfirmed,
			"request_id": self.request_id,
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
	let context = RequestContext::from_http(&req).unwrap_or_else(|| RequestContext {
		request_id: Uuid::new_v4().to_string(),
		trace_id: Uuid::new_v4().to_string(),
		traceparent: None,
		tracestate: None,
	});
	// Construct AuthContext from HTTP request
	// Authentication materials MUST be in headers per protocol specification
	let auth_ctx = AuthContext::from_http(req.headers());

	// Extract principal using AuthProvider
	// This extracts the public key and signature from headers/metadata,
	// creates a Principal, and verifies the signature against the order payload
	let authenticated =
		auth::authenticate_with_provider(&auth_ctx, &request, state.auth_provider.as_ref())
			.map_err(|e| GatewayError::auth(e, &context))?;
	let principal = authenticated.principal;
	tracing::Span::current().record("principal_id", field::display(principal.id()));

	// Check rate limit by principal (public key)
	// Gateway only performs rate limiting at the cryptographic principal level,
	// not at the business user level.
	admission::check_rate_limit(&principal).map_err(|e| GatewayError::admission(e, &context))?;

	// Validate and admit the order (protocol-level checks)
	admission::validate_and_admit(&request).map_err(|e| GatewayError::admission(e, &context))?;

	let replay_guard: ReplayGuard =
		admission::begin_replay(&principal, authenticated.timestamp, &authenticated.nonce)
			.map_err(|e| GatewayError::admission(e, &context))?;

	// Dispatch to matching engine (use principal.id() as identifier)
	// Note: principal.id() returns hex-encoded public key, which is passed
	// to matching engine as the principal identifier (not a business user ID).
	let dispatch_result = state
		.dispatcher
		.dispatch_order(request.into_inner(), principal.id(), context.clone())
		.await;

	match dispatch_result {
		Ok(DispatchResult { order, timings, .. }) => {
			tracing::Span::current().record("queue_wait_ms", field::display(timings.queue_wait_ms));
			tracing::Span::current().record("rpc_ms", field::display(timings.rpc_ms));
			replay_guard.finish(ReplayOutcome::Terminal);
			Ok(HttpResponse::Ok().json(PlaceOrderResponse {
				order_id: order.order_id,
				status: OrderStatus::Accepted,
				client_order_id: None,
			}))
		}
		Err(err) => {
			let (gateway_err, outcome) = map_dispatch_error(err, &context);
			replay_guard.finish(outcome);
			Err(gateway_err)
		}
	}
}

/// Handle order query request
pub async fn get_order(
	_state: web::Data<GatewayState>,
	path: web::Path<String>,
) -> Result<HttpResponse, GatewayError> {
	let order_id = path.into_inner();
	let context = RequestContext {
		request_id: Uuid::new_v4().to_string(),
		trace_id: Uuid::new_v4().to_string(),
		traceparent: None,
		tracestate: None,
	};

	// TODO: Query order from matching engine via gRPC
	Err(GatewayError::internal(
		format!("Order query not yet implemented: {}", order_id),
		&context,
	))
}

/// Handle order cancellation request
pub async fn cancel_order(
	_state: web::Data<GatewayState>,
	path: web::Path<String>,
) -> Result<HttpResponse, GatewayError> {
	let order_id = path.into_inner();
	let context = RequestContext {
		request_id: Uuid::new_v4().to_string(),
		trace_id: Uuid::new_v4().to_string(),
		traceparent: None,
		tracestate: None,
	};

	// TODO: Cancel order via matching engine gRPC
	Err(GatewayError::internal(
		format!("Order cancellation not yet implemented: {}", order_id),
		&context,
	))
}

fn map_dispatch_error(err: DispatcherError, ctx: &RequestContext) -> (GatewayError, ReplayOutcome) {
	let outcome = match err {
		DispatcherError::GatewayOverloaded
		| DispatcherError::QueueTimeout
		| DispatcherError::MatchingTimeout
		| DispatcherError::MatchingOverloaded(_)
		| DispatcherError::MatchingInternal(_)
		| DispatcherError::InvalidResponse(_)
		| DispatcherError::DispatchingError(_) => ReplayOutcome::RetryableFailure,
		DispatcherError::MatchingRejected(_) | DispatcherError::MatchingEngineNotFound(_) => {
			ReplayOutcome::Terminal
		}
	};

	(GatewayError::dispatch(err, ctx), outcome)
}

#[cfg(test)]
mod tests {
	use super::*;
	use actix_web::ResponseError;
	use actix_web::body::to_bytes;
	use actix_web::http::StatusCode;
	use serde_json::Value;

	fn ctx() -> RequestContext {
		RequestContext {
			request_id: "req-test".to_string(),
			trace_id: "trace-test".to_string(),
			traceparent: None,
			tracestate: None,
		}
	}

	#[actix_rt::test]
	async fn matching_timeout_maps_to_unconfirmed() {
		let err = GatewayError::dispatch(DispatcherError::MatchingTimeout, &ctx());
		let resp = err.error_response();
		assert_eq!(resp.status(), StatusCode::ACCEPTED);
		let body = to_bytes(resp.into_body()).await.unwrap();
		let json: Value = serde_json::from_slice(&body).unwrap();
		assert_eq!(json["code"], "UNCONFIRMED");
		assert_eq!(json["retryable"], true);
		assert_eq!(json["unconfirmed"], true);
	}

	#[actix_rt::test]
	async fn gateway_overload_is_retryable_service_unavailable() {
		let err = GatewayError::dispatch(DispatcherError::GatewayOverloaded, &ctx());
		let resp = err.error_response();
		assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
		let body = to_bytes(resp.into_body()).await.unwrap();
		let json: Value = serde_json::from_slice(&body).unwrap();
		assert_eq!(json["code"], "GATEWAY_OVERLOADED");
		assert_eq!(json["retryable"], true);
	}
}
