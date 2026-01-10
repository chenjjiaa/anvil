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

//! gRPC server for matching engine
//!
//! This module implements the RPC ingress layer, which is multi-threaded
//! and responsible for:
//! - Receiving and validating order requests
//! - Checking idempotency via Order Journal
//! - Appending orders to Order Journal
//! - Enqueuing orders to the matching loop
//! - Returning ACK to clients
//!
//! The RPC layer does NOT perform matching - that happens in the
//! single-threaded matching loop.

use std::sync::{Arc, Mutex};

use anvil_sdk::types::Side;
use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tonic::{Request, Response, Status};
use tracing::{debug, field, info, warn};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::journal::OrderJournal;
use crate::queue::QueueSender;
use crate::types::OrderCommand;

// Include generated gRPC code
pub mod proto {
	tonic::include_proto!("anvil.matching");
}

use proto::matching_service_server::{MatchingService, MatchingServiceServer};
use proto::{
	CancelOrderRequest, CancelOrderResponse, GetOrderRequest, GetOrderResponse, MatchedTrade,
	OrderSide as ProtoOrderSide, OrderStatus as ProtoOrderStatus, StreamMatchedTradesRequest,
	SubmitDisposition, SubmitOrderRequest, SubmitOrderResponse,
};
use tokio_stream;

/// Matching service implementation
///
/// This is the RPC ingress layer. It is multi-threaded (one handler per request)
/// but does NOT perform matching. Its responsibilities are:
/// - Parameter validation
/// - Idempotency checking via Order Journal
/// - Appending to Order Journal
/// - Enqueuing to ingress queue
/// - Returning ACK
pub struct MatchingServiceImpl {
	queue_sender: QueueSender,
	journal: Arc<Mutex<Box<dyn OrderJournal>>>,
	market: String,
}

impl MatchingServiceImpl {
	pub fn new(
		queue_sender: QueueSender,
		journal: Arc<Mutex<Box<dyn OrderJournal>>>,
		market: String,
	) -> Self {
		Self {
			queue_sender,
			journal,
			market,
		}
	}
}

#[tonic::async_trait]
impl MatchingService for MatchingServiceImpl {
	async fn submit_order(
		&self,
		request: Request<SubmitOrderRequest>,
	) -> Result<Response<SubmitOrderResponse>, Status> {
		let start = std::time::Instant::now();

		// Extract tracing context from gRPC metadata
		let parent_cx =
			TraceContextPropagator::new().extract(&MetadataExtractor(request.metadata()));

		let req = request.into_inner();

		// Create structured span with all important fields
		let span = tracing::info_span!(
			"submit_order",
			order_id = %req.order_id,
			market = %req.market,
			side = ?req.side(),
			price = req.price,
			size = req.size,
			public_key = %req.public_key,
			trace_id = field::Empty,
			status = field::Empty,
			disposition = field::Empty,
			latency_ms = field::Empty
		);

		// Set parent context for distributed tracing
		if let Err(err) = span.set_parent(parent_cx) {
			warn!(error = %err, "failed to set parent span context");
		}

		// Enter the span for this request
		let _guard = span.enter();

		// Basic validation
		if req.market != self.market {
			let duration = start.elapsed();
			tracing::Span::current().record("status", "rejected");
			tracing::Span::current().record("disposition", "invalid_order");
			tracing::Span::current().record("latency_ms", duration.as_millis() as u64);
			warn!(
				order_id = %req.order_id,
				market = %req.market,
				reason = "unsupported market",
				duration_ms = duration.as_millis(),
				"Order rejected"
			);
			return Ok(Response::new(SubmitOrderResponse {
				order_id: req.order_id,
				status: ProtoOrderStatus::Rejected as i32,
				trades: Vec::new(),
				fully_filled: false,
				partially_filled: false,
				disposition: SubmitDisposition::InvalidOrder as i32,
				reason: format!("Market {} not supported", req.market),
			}));
		}

		if req.size == 0 {
			let duration = start.elapsed();
			tracing::Span::current().record("status", "rejected");
			tracing::Span::current().record("disposition", "invalid_order");
			tracing::Span::current().record("latency_ms", duration.as_millis() as u64);
			warn!(
				order_id = %req.order_id,
				reason = "zero size",
				duration_ms = duration.as_millis(),
				"Order rejected"
			);
			return Ok(Response::new(SubmitOrderResponse {
				order_id: req.order_id,
				status: ProtoOrderStatus::Rejected as i32,
				trades: Vec::new(),
				fully_filled: false,
				partially_filled: false,
				disposition: SubmitDisposition::InvalidOrder as i32,
				reason: "Order size must be greater than 0".to_string(),
			}));
		}

		// Create order command
		let cmd = OrderCommand {
			order_id: req.order_id.clone(),
			market: req.market.clone(),
			side: match req.side() {
				ProtoOrderSide::Buy => Side::Buy,
				ProtoOrderSide::Sell => Side::Sell,
			},
			price: req.price,
			size: req.size,
			timestamp: req.timestamp,
			public_key: req.public_key.clone(),
		};

		// Check idempotency: is this order already active?
		{
			let journal = self.journal.lock().unwrap();
			if journal.is_active(&cmd.order_id) {
				let duration = start.elapsed();
				tracing::Span::current().record("status", "rejected");
				tracing::Span::current().record("disposition", "duplicate");
				tracing::Span::current().record("latency_ms", duration.as_millis() as u64);
				debug!(
					order_id = %cmd.order_id,
					reason = "duplicate order ID",
					duration_ms = duration.as_millis(),
					"Duplicate order detected"
				);
				return Ok(Response::new(SubmitOrderResponse {
					order_id: cmd.order_id,
					status: ProtoOrderStatus::Rejected as i32,
					trades: Vec::new(),
					fully_filled: false,
					partially_filled: false,
					disposition: SubmitDisposition::InvalidOrder as i32,
					reason: "Duplicate order ID".to_string(),
				}));
			}
		}

		// Append to Order Journal (before ACK!)
		{
			let mut journal = self.journal.lock().unwrap();
			if let Err(e) = journal.append(cmd.clone()) {
				let duration = start.elapsed();
				tracing::Span::current().record("status", "rejected");
				tracing::Span::current().record("disposition", "journal_error");
				tracing::Span::current().record("latency_ms", duration.as_millis() as u64);
				warn!(
					order_id = %cmd.order_id,
					error = %e,
					duration_ms = duration.as_millis(),
					"Journal append failed"
				);
				return Ok(Response::new(SubmitOrderResponse {
					order_id: cmd.order_id,
					status: ProtoOrderStatus::Rejected as i32,
					trades: Vec::new(),
					fully_filled: false,
					partially_filled: false,
					disposition: SubmitDisposition::InternalError as i32,
					reason: format!("Journal error: {}", e),
				}));
			}
		}

		// Try to enqueue to matching loop
		match self.queue_sender.try_enqueue(cmd.clone()) {
			Ok(_) => {
				// Successfully enqueued
				// ACK means: order has been accepted and recorded in journal
				// Matching will happen asynchronously
				let duration = start.elapsed();
				tracing::Span::current().record("status", "accepted");
				tracing::Span::current().record("disposition", "accepted_ok");
				tracing::Span::current().record("latency_ms", duration.as_millis() as u64);
				info!(
					order_id = %cmd.order_id,
					market = %cmd.market,
					side = ?cmd.side,
					price = cmd.price,
					size = cmd.size,
					duration_ms = duration.as_millis(),
					"Order accepted"
				);

				Ok(Response::new(SubmitOrderResponse {
					order_id: cmd.order_id,
					status: ProtoOrderStatus::Accepted as i32,
					trades: Vec::new(),
					fully_filled: false,
					partially_filled: false,
					disposition: SubmitDisposition::AcceptedOk as i32,
					reason: String::new(),
				}))
			}
			Err(crate::queue::QueueError::Full) => {
				// Queue full - engine overloaded
				// Note: order is still in journal, will be retried on recovery
				let duration = start.elapsed();
				tracing::Span::current().record("status", "rejected");
				tracing::Span::current().record("disposition", "overloaded");
				tracing::Span::current().record("latency_ms", duration.as_millis() as u64);
				warn!(
					order_id = %cmd.order_id,
					reason = "ingress queue full",
					duration_ms = duration.as_millis(),
					"Engine overloaded"
				);

				Ok(Response::new(SubmitOrderResponse {
					order_id: cmd.order_id,
					status: ProtoOrderStatus::Rejected as i32,
					trades: Vec::new(),
					fully_filled: false,
					partially_filled: false,
					disposition: SubmitDisposition::OverloadedEngine as i32,
					reason: "Matching engine overloaded".to_string(),
				}))
			}
			Err(e) => {
				// Queue disconnected or other error
				let duration = start.elapsed();
				tracing::Span::current().record("status", "rejected");
				tracing::Span::current().record("disposition", "queue_error");
				tracing::Span::current().record("latency_ms", duration.as_millis() as u64);
				warn!(
					order_id = %cmd.order_id,
					error = %e,
					duration_ms = duration.as_millis(),
					"Queue error"
				);
				Ok(Response::new(SubmitOrderResponse {
					order_id: cmd.order_id,
					status: ProtoOrderStatus::Rejected as i32,
					trades: Vec::new(),
					fully_filled: false,
					partially_filled: false,
					disposition: SubmitDisposition::InternalError as i32,
					reason: format!("Queue error: {}", e),
				}))
			}
		}
	}

	async fn get_order(
		&self,
		_request: Request<GetOrderRequest>,
	) -> Result<Response<GetOrderResponse>, Status> {
		// TODO: Implement order query
		Err(Status::unimplemented("Order query not yet implemented"))
	}

	async fn cancel_order(
		&self,
		_request: Request<CancelOrderRequest>,
	) -> Result<Response<CancelOrderResponse>, Status> {
		// TODO: Implement order cancellation via OrderCommand
		Err(Status::unimplemented(
			"Order cancellation not yet implemented",
		))
	}

	type StreamMatchedTradesStream =
		tokio_stream::wrappers::ReceiverStream<Result<MatchedTrade, Status>>;

	async fn stream_matched_trades(
		&self,
		_request: Request<StreamMatchedTradesRequest>,
	) -> Result<Response<Self::StreamMatchedTradesStream>, Status> {
		// TODO: Implement streaming from event buffer
		let (_tx, rx) = tokio::sync::mpsc::channel(128);
		Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
			rx,
		)))
	}
}

/// Create matching service server
pub fn create_server(
	queue_sender: QueueSender,
	journal: Arc<Mutex<Box<dyn OrderJournal>>>,
	market: String,
) -> MatchingServiceServer<MatchingServiceImpl> {
	MatchingServiceServer::new(MatchingServiceImpl::new(queue_sender, journal, market))
}

struct MetadataExtractor<'a>(&'a tonic::metadata::MetadataMap);

impl<'a> Extractor for MetadataExtractor<'a> {
	fn get(&self, key: &str) -> Option<&str> {
		self.0.get(key).and_then(|v| v.to_str().ok())
	}

	fn keys(&self) -> Vec<&str> {
		Vec::new()
	}
}
