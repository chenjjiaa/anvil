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

use std::{
	future::{Ready, ready},
	pin::Pin,
	rc::Rc,
	task::{Context, Poll},
};

use actix_web::{
	Error, HttpMessage,
	dev::{Service, ServiceRequest, ServiceResponse, Transform},
	http::header::HeaderMap,
};
use opentelemetry::{
	propagation::{Extractor, TextMapPropagator},
	trace::{TraceContextExt, TraceFlags, TraceState},
};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing::{Instrument, field, info};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::request_context::RequestContext;

/// CORS middleware for actix-web
pub struct CorsMiddleware;

impl<S, B> Transform<S, ServiceRequest> for CorsMiddleware
where
	S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
	S::Future: 'static,
	B: 'static,
{
	type Response = ServiceResponse<B>;
	type Error = Error;
	type InitError = ();
	type Transform = CorsMiddlewareInner<S>;
	type Future = Ready<Result<Self::Transform, Self::InitError>>;

	fn new_transform(&self, service: S) -> Self::Future {
		ready(Ok(CorsMiddlewareInner {
			service: Rc::new(service),
		}))
	}
}

/// OpenTelemetry header extractor for actix-web HTTP headers
///
/// This adapter implements OpenTelemetry's `Extractor` trait to extract trace
/// context from HTTP headers. It is used by `TraceContextPropagator` to read
/// `traceparent` and `tracestate` headers from incoming requests.
struct HeaderExtractor<'a>(&'a HeaderMap);

impl<'a> Extractor for HeaderExtractor<'a> {
	fn get(&self, key: &str) -> Option<&str> {
		self.0
			.get(key)
			.and_then(|v: &actix_web::http::header::HeaderValue| v.to_str().ok())
	}

	fn keys(&self) -> Vec<&str> {
		self.0
			.keys()
			.map(|k: &actix_web::http::header::HeaderName| k.as_str())
			.collect()
	}
}

pub struct CorsMiddlewareInner<S> {
	service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for CorsMiddlewareInner<S>
where
	S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
	S::Future: 'static,
	B: 'static,
{
	type Response = ServiceResponse<B>;
	type Error = Error;
	type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>>>>;

	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.service.poll_ready(cx)
	}

	fn call(&self, req: ServiceRequest) -> Self::Future {
		let service = self.service.clone();

		Box::pin(async move {
			let mut res = service.call(req).await?;

			// Add CORS headers
			use actix_web::http::header::HeaderValue;
			res.headers_mut().insert(
				actix_web::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
				HeaderValue::from_static("*"),
			);
			res.headers_mut().insert(
				actix_web::http::header::ACCESS_CONTROL_ALLOW_METHODS,
				HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
			);
			res.headers_mut().insert(
				actix_web::http::header::ACCESS_CONTROL_ALLOW_HEADERS,
				// Note: include gateway auth protocol headers for browser preflight.
				// Gateway requires auth materials in metadata (headers), not in body.
				HeaderValue::from_static(
					"Content-Type, Authorization, X-Public-Key, X-Signature, X-Signature-Alg, X-Timestamp, X-Nonce",
				),
			);

			Ok(res)
		})
	}
}

/// Logging middleware for actix-web
pub struct LoggingMiddleware;

impl<S, B> Transform<S, ServiceRequest> for LoggingMiddleware
where
	S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
	S::Future: 'static,
	B: 'static,
{
	type Response = ServiceResponse<B>;
	type Error = Error;
	type InitError = ();
	type Transform = LoggingMiddlewareInner<S>;
	type Future = Ready<Result<Self::Transform, Self::InitError>>;

	fn new_transform(&self, service: S) -> Self::Future {
		ready(Ok(LoggingMiddlewareInner {
			service: Rc::new(service),
		}))
	}
}

/// Inner service wrapper for logging middleware
///
/// This structure wraps the underlying service and adds logging, tracing, and
/// OpenTelemetry context propagation to HTTP requests.
pub struct LoggingMiddlewareInner<S> {
	/// The wrapped service that handles the actual request processing
	service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for LoggingMiddlewareInner<S>
where
	S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
	S::Future: 'static,
	B: 'static,
{
	type Response = ServiceResponse<B>;
	type Error = Error;
	type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>>>>;

	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.service.poll_ready(cx)
	}

	fn call(&self, req: ServiceRequest) -> Self::Future {
		let service = self.service.clone();
		let method = req.method().clone();
		let path = req.path().to_string();
		let mut req = req;
		let mut ctx = RequestContext::ensure(&mut req);

		// Extract OpenTelemetry trace context from incoming HTTP headers
		//
		// The TraceContextPropagator reads `traceparent` and `tracestate` headers
		// from the request and creates a parent span context. This context is used
		// to link the current request span to the upstream trace.
		let propagator = TraceContextPropagator::new();
		let parent_cx = propagator.extract(&HeaderExtractor(req.headers()));
		let span = tracing::span!(
			tracing::Level::INFO,
			"http_request",
			method = %method,
			path = %path,
			request_id = %ctx.request_id,
			trace_id = field::Empty,  // Will be set after parent context is applied
			principal_id = field::Empty,
			status_code = field::Empty,
			latency_ms = field::Empty,
			queue_wait_ms = field::Empty,
			rpc_ms = field::Empty
		);
		// Set the extracted parent context as the parent of the current span
		//
		// This links the gateway's request span to the upstream trace, enabling
		// end-to-end distributed tracing across service boundaries.
		if let Err(err) = span.set_parent(parent_cx) {
			tracing::warn!(error = %err, "failed to set parent span context");
		}

		// Update RequestContext with OpenTelemetry span context
		//
		// If a valid span context was extracted or created, we update the
		// RequestContext with the trace ID and build W3C-compliant traceparent
		// and tracestate headers. These headers will be propagated to downstream
		// services (matching engine via gRPC) and written to HTTP responses.
		//
		// We also update the span field with the OpenTelemetry trace_id to ensure
		// consistency between span fields and log messages.
		{
			let otel_span = span.context();
			let span_ctx = otel_span.span().span_context().clone();
			if span_ctx.is_valid() {
				let trace_id_hex = span_ctx.trace_id().to_string();
				let span_id_hex = span_ctx.span_id().to_string();
				let trace_flags: TraceFlags = span_ctx.trace_flags();
				let state: TraceState = span_ctx.trace_state().clone();

				// Update span field with OpenTelemetry trace_id to ensure consistency
				span.record("trace_id", &trace_id_hex);

				ctx.trace_id = trace_id_hex.clone();
				ctx.traceparent = Some(format!(
					"00-{}-{}-{:02x}",
					trace_id_hex,
					span_id_hex,
					trace_flags.to_u8()
				));
				let state_header = state.header();
				if !state_header.is_empty() {
					ctx.tracestate = Some(state_header);
				}
				req.extensions_mut().insert(ctx.clone());
			} else {
				// If span context is invalid, use RequestContext trace_id as fallback
				span.record("trace_id", &ctx.trace_id);
			}
		}

		Box::pin(
			async move {
				let start = std::time::Instant::now();
				let res = service.call(req).await;
				let duration = start.elapsed();

				match res {
					Ok(mut response) => {
						let status = response.status().as_u16();
						let current = tracing::Span::current();
						current.record("status_code", status);
						current.record("latency_ms", duration.as_millis() as i64);
						ctx.write_response_headers(&mut response);
						info!(
							method = %method,
							path = %path,
							status = status,
							duration_ms = duration.as_millis(),
							request_id = %ctx.request_id,
							trace_id = %ctx.trace_id,
							"Request completed"
						);
						Ok(response)
					}
					Err(e) => {
						tracing::error!(
							method = %method,
							path = %path,
							error = %e,
							duration_ms = duration.as_millis(),
							request_id = %ctx.request_id,
							trace_id = %ctx.trace_id,
							"Request failed"
						);
						Err(e)
					}
				}
			}
			.instrument(span),
		)
	}
}
