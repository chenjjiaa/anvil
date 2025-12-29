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

use std::future::{Ready, ready};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use actix_web::{
	Error,
	dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use tracing::info;

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
				HeaderValue::from_static("Content-Type, Authorization"),
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

pub struct LoggingMiddlewareInner<S> {
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
		let span =
			tracing::span!(tracing::Level::INFO, "http_request", method = %method, path = %path);
		let _enter = span.enter();

		Box::pin(async move {
			let start = std::time::Instant::now();
			let res = service.call(req).await;
			let duration = start.elapsed();

			match &res {
				Ok(response) => {
					info!(
						status = response.status().as_u16(),
						duration_ms = duration.as_millis(),
						"Request completed"
					);
				}
				Err(e) => {
					tracing::error!(error = %e, duration_ms = duration.as_millis(), "Request failed");
				}
			}

			res
		})
	}
}
