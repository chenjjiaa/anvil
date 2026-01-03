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

//! Request context for tracing and observability
//!
//! This module provides `RequestContext` which encapsulates tracing and request
//! identification information for observability purposes.
//!
//! # Tracing Context Propagation
//!
//! The gateway supports both W3C Trace Context (`traceparent`, `tracestate`) and
//! legacy `X-Trace-Id` headers for distributed tracing:
//!
//! 1. **W3C Trace Context (preferred)**: If `traceparent` header is present and valid,
//!    it is used to extract trace ID and span context. The gateway propagates this
//!    context to downstream services (matching engine via gRPC).
//!
//! 2. **Legacy X-Trace-Id (fallback)**: If `traceparent` is not present, the gateway
//!    checks for `X-Trace-Id` header. If valid, it converts it to W3C format for
//!    downstream propagation.
//!
//! 3. **Generation**: If neither header is present or both are invalid, the gateway
//!    generates a new trace ID and creates W3C-compliant headers.
//!
//! # Request ID vs Trace ID
//!
//! - **`request_id`**: Unique identifier for a single HTTP request within the gateway.
//!   Used for log correlation within a single service instance. Generated per request
//!   if not provided in `X-Request-Id` header.
//!
//! - **`trace_id`**: End-to-end trace identifier that spans multiple services.
//!   Used for distributed tracing across the entire request path (gateway → matching
//!   engine → other services). Propagated via `traceparent` or `X-Trace-Id` headers.
//!
//! # Best Practices
//!
//! For payment/transaction systems:
//!
//! - **Upstream services** (API Gateway, BFF, or clients with tracing SDKs) should
//!   provide `traceparent` header on initial request. This ensures consistent trace
//!   context across retries.
//!
//! - **Gateway fallback**: If no `traceparent` is provided, gateway generates and
//!   backfills headers in response. Clients should reuse these headers for retries.
//!
//! - **Validation**: Gateway validates incoming trace IDs. Invalid formats trigger
//!   warning logs and fallback to generated trace IDs.

use actix_web::{
	HttpMessage, HttpRequest,
	dev::ServiceRequest,
	http::header::{HeaderName, HeaderValue},
};
use tracing::warn;
use uuid::Uuid;

use crate::trace_context::{
	TraceContextError, generate_trace_id, parse_traceparent, parse_tracestate,
	validate_hex_trace_id,
};

/// HTTP header name for request ID
pub const HEADER_REQUEST_ID: &str = "X-Request-Id";

/// HTTP header name for legacy trace ID (backward compatibility)
pub const HEADER_TRACE_ID: &str = "X-Trace-Id";

/// HTTP header name for W3C Trace Context traceparent
pub const HEADER_TRACEPARENT: &str = "traceparent";

/// HTTP header name for W3C Trace Context tracestate
pub const HEADER_TRACESTATE: &str = "tracestate";

/// Request context containing tracing and request identification information
///
/// This structure holds both request-level and trace-level identifiers for
/// observability and log correlation. It is attached to HTTP requests via
/// `ServiceRequest::extensions()` and propagated to downstream services.
///
/// # Context Extraction Priority
///
/// When creating a `RequestContext`, the gateway follows this priority:
///
/// 1. **W3C traceparent** (highest priority): Parse `traceparent` header if present
/// 2. **Legacy X-Trace-Id**: Validate `X-Trace-Id` header if `traceparent` is missing
/// 3. **Generation**: Generate new trace ID if neither header is present or both are invalid
///
/// Invalid trace IDs trigger warning logs but do not fail the request.
#[derive(Clone, Debug)]
pub struct RequestContext {
	/// Unique identifier for this HTTP request within the gateway
	///
	/// Used for log correlation within a single service instance. Generated
	/// per request if not provided in `X-Request-Id` header.
	pub request_id: String,

	/// End-to-end trace identifier spanning multiple services
	///
	/// Used for distributed tracing across the entire request path. Extracted
	/// from `traceparent` or `X-Trace-Id` headers, or generated if not present.
	pub trace_id: String,

	/// W3C Trace Context traceparent header value (if present or generated)
	///
	/// This header is propagated to downstream services via gRPC metadata.
	/// Format: `version-traceid-spanid-flags` (hex-encoded).
	pub traceparent: Option<String>,

	/// W3C Trace Context tracestate header value (if present)
	///
	/// Vendor-specific trace state. Propagated to downstream services if present.
	pub tracestate: Option<String>,
}

impl RequestContext {
	/// Ensure a RequestContext exists for the given request, creating one if needed
	///
	/// This function checks if a `RequestContext` already exists in the request
	/// extensions. If not, it extracts tracing context from headers and creates
	/// a new context following the priority order:
	///
	/// 1. **W3C traceparent**: Parse `traceparent` header (highest priority)
	/// 2. **Legacy X-Trace-Id**: Validate `X-Trace-Id` header (fallback)
	/// 3. **Generation**: Generate new trace ID if neither is present or valid
	///
	/// The created context is stored in `ServiceRequest::extensions()` for reuse.
	///
	/// # Arguments
	///
	/// * `req` - The HTTP service request (mutable to insert context into extensions)
	///
	/// # Returns
	///
	/// Returns the `RequestContext` (either existing or newly created).
	///
	/// # Errors
	///
	/// Invalid trace IDs are logged as warnings but do not fail the request.
	/// The gateway falls back to generating a new trace ID.
	pub fn ensure(req: &mut ServiceRequest) -> Self {
		if let Some(ctx) = req.extensions().get::<RequestContext>() {
			return ctx.clone();
		}

		let request_id =
			extract_header(req, HEADER_REQUEST_ID).unwrap_or_else(|| Uuid::new_v4().to_string());

		// 1) 优先 traceparent
		let mut trace_id_bytes = None;
		let mut incoming_traceparent = None;
		let mut incoming_tracestate = None;

		if let Some(raw_tp) = extract_header(req, HEADER_TRACEPARENT) {
			match parse_traceparent(&raw_tp) {
				Ok(tp) => {
					trace_id_bytes = Some(tp.trace_id);
					incoming_traceparent = Some(raw_tp);
					incoming_tracestate = extract_header(req, HEADER_TRACESTATE)
						.and_then(|ts| parse_tracestate(&ts).ok().map(|_| ts));
				}
				Err(e) => {
					warn!(
						target: "gateway::trace",
						request_id = %request_id,
						error = %trace_err(&e),
						"Invalid traceparent, will generate new trace"
					);
				}
			}
		}

		// 2) 兼容 X-Trace-Id
		if trace_id_bytes.is_none()
			&& let Some(raw_xtid) = extract_header(req, HEADER_TRACE_ID)
		{
			match validate_hex_trace_id(&raw_xtid) {
				Ok(bytes) => {
					trace_id_bytes = Some(bytes);
				}
				Err(e) => warn!(
					target: "gateway::trace",
					request_id = %request_id,
					error = %trace_err(&e),
					"Invalid X-Trace-Id, will generate new trace"
				),
			}
		}

		// 3) 生成
		let trace_id_bytes = trace_id_bytes.unwrap_or_else(generate_trace_id);
		let trace_id = hex::encode(trace_id_bytes);

		let ctx = RequestContext {
			request_id,
			trace_id,
			traceparent: incoming_traceparent,
			tracestate: incoming_tracestate,
		};
		req.extensions_mut().insert(ctx.clone());
		ctx
	}

	/// Extract RequestContext from an HTTP request if it exists
	///
	/// This function retrieves a previously created `RequestContext` from the
	/// request extensions. Returns `None` if no context was found.
	///
	/// # Arguments
	///
	/// * `req` - The HTTP request to extract context from
	///
	/// # Returns
	///
	/// Returns `Some(RequestContext)` if found, `None` otherwise.
	pub fn from_http(req: &HttpRequest) -> Option<Self> {
		req.extensions().get::<RequestContext>().cloned()
	}

	/// Write tracing and request identification headers to HTTP response
	///
	/// This function adds the following headers to the response if they are not
	/// already present:
	///
	/// - `X-Request-Id`: Request ID for log correlation
	/// - `X-Trace-Id`: Legacy trace ID (for backward compatibility)
	/// - `traceparent`: W3C Trace Context traceparent (if available)
	/// - `tracestate`: W3C Trace Context tracestate (if available)
	///
	/// Headers are only added if they are not already present in the response,
	/// allowing handlers to override them if needed.
	///
	/// # Arguments
	///
	/// * `res` - The HTTP service response to write headers to
	pub fn write_response_headers<B>(&self, res: &mut actix_web::dev::ServiceResponse<B>) {
		let headers = res.headers_mut();
		if headers.get(HEADER_REQUEST_ID).is_none()
			&& let Ok(value) = HeaderValue::from_str(&self.request_id)
		{
			headers.insert(HeaderName::from_static("x-request-id"), value);
		}
		if headers.get(HEADER_TRACE_ID).is_none()
			&& let Ok(value) = HeaderValue::from_str(&self.trace_id)
		{
			headers.insert(HeaderName::from_static("x-trace-id"), value);
		}
		if headers.get(HEADER_TRACEPARENT).is_none()
			&& let Some(tp) = &self.traceparent
			&& let Ok(value) = HeaderValue::from_str(tp)
		{
			headers.insert(HeaderName::from_static("traceparent"), value);
		}
		if headers.get(HEADER_TRACESTATE).is_none()
			&& let Some(ts) = &self.tracestate
			&& let Ok(value) = HeaderValue::from_str(ts)
		{
			headers.insert(HeaderName::from_static("tracestate"), value);
		}
	}
}

/// Extract a header value from HTTP request headers as a string
///
/// # Arguments
///
/// * `req` - The HTTP service request
/// * `name` - Header name (case-insensitive)
///
/// # Returns
///
/// Returns `Some(String)` if the header exists and is valid UTF-8, `None` otherwise.
fn extract_header(req: &ServiceRequest, name: &str) -> Option<String> {
	req.headers()
		.get(name)
		.and_then(|v| v.to_str().ok())
		.map(|s| s.to_string())
}

/// Convert a TraceContextError to a static string for logging
///
/// This helper function converts trace context parsing errors to short string
/// identifiers suitable for structured logging.
///
/// # Arguments
///
/// * `err` - The trace context error to convert
///
/// # Returns
///
/// Returns a static string identifier for the error type.
fn trace_err(err: &TraceContextError) -> &'static str {
	match err {
		TraceContextError::Format => "invalid_format",
		TraceContextError::Version => "invalid_version",
		TraceContextError::TraceId => "invalid_trace_id",
		TraceContextError::SpanId => "invalid_span_id",
		TraceContextError::TraceFlags => "invalid_trace_flags",
		TraceContextError::TraceState => "invalid_tracestate",
	}
}
