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
	collections::HashMap,
	sync::Arc,
	time::{Duration, Instant},
};

use anvil_matching::types::Order as MatchingOrder;
use anvil_sdk::types::PlaceOrderRequest;
use thiserror::Error;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::{
	config::GatewayRuntimeConfig,
	grpc_client::{GrpcClientError, MatchingGrpcClient, proto::SubmitDisposition},
	request_context::RequestContext,
};

/// Error types for dispatching operations
#[derive(Debug, Error)]
pub enum DispatcherError {
	#[error("Matching engine not found for market: {0}")]
	MatchingEngineNotFound(String),
	#[error("Gateway dispatch queue is overloaded")]
	GatewayOverloaded,
	#[error("Timed out while waiting in dispatch queue")]
	QueueTimeout,
	#[error("Matching confirmation not received within gateway timeout")]
	MatchingTimeout,
	#[error("Matching engine overloaded: {0}")]
	MatchingOverloaded(String),
	#[error("Matching rejected order: {0}")]
	MatchingRejected(String),
	#[error("Matching internal error: {0}")]
	MatchingInternal(String),
	#[error("Dispatching error: {0}")]
	DispatchingError(String),
	#[error("Invalid response from matching engine: {0}")]
	InvalidResponse(String),
}

#[derive(Debug, Clone)]
pub struct DispatchTimings {
	pub queue_wait_ms: u128,
	pub rpc_ms: u128,
}

#[derive(Debug, Clone)]
pub struct DispatchResult {
	pub order: MatchingOrder,
	pub timings: DispatchTimings,
}

struct DispatchJob {
	order: MatchingOrder,
	endpoint: String,
	context: RequestContext,
	enqueued_at: Instant,
	response_tx: oneshot::Sender<Result<DispatchResult, DispatcherError>>,
}

/// Dispatcher that forwards orders to the appropriate matching engine
///
/// Uses gRPC to communicate with matching engines.
pub struct MatchingDispatcher {
	/// Market -> Matching engine endpoint mapping
	matching_engines: HashMap<String, String>,
	/// Market -> gRPC client mapping (with mutex for async access)
	clients: Arc<Mutex<HashMap<String, MatchingGrpcClient>>>,
	queue_tx: mpsc::Sender<DispatchJob>,
	queue_timeout: Duration,
	rpc_timeout: Duration,
}

impl MatchingDispatcher {
	/// Create a new dispatcher
	pub async fn new(config: &GatewayRuntimeConfig) -> anyhow::Result<Self> {
		let (queue_tx, queue_rx) = mpsc::channel(config.dispatch_queue_capacity);

		let dispatcher = Self {
			matching_engines: config.matching_engines.clone(),
			clients: Arc::new(Mutex::new(HashMap::new())),
			queue_tx,
			queue_timeout: Duration::from_millis(config.dispatch_queue_timeout_ms),
			rpc_timeout: Duration::from_millis(config.matching_rpc_timeout_ms),
		};

		dispatcher.spawn_workers(queue_rx);

		tracing::info!(
			target: "server::dispatcher",
			"MatchingDispatcher initialized with {} markets (queue cap={}, queue timeout={}ms, rpc timeout={}ms)",
			dispatcher.matching_engines.len(),
			config.dispatch_queue_capacity,
			config.dispatch_queue_timeout_ms,
			config.matching_rpc_timeout_ms,
		);

		Ok(dispatcher)
	}

	/// Get or create gRPC client for a market
	async fn get_client(
		clients: &Arc<Mutex<HashMap<String, MatchingGrpcClient>>>,
		endpoint: &str,
		rpc_timeout: Duration,
	) -> Result<MatchingGrpcClient, DispatcherError> {
		let mut clients_guard = clients.lock().await;

		if let Some(client) = clients_guard.get(endpoint) {
			return Ok(client.clone());
		}

		let client = MatchingGrpcClient::new(endpoint, rpc_timeout)
			.await
			.map_err(|e| {
				DispatcherError::DispatchingError(format!("Failed to create client: {}", e))
			})?;
		let clone = client.clone();
		clients_guard.insert(endpoint.to_string(), client);
		Ok(clone)
	}

	/// Dispatch an order to the appropriate matching engine
	///
	/// This converts the gateway's PlaceOrderRequest into the matching
	/// engine's internal Order format and forwards it via gRPC.
	///
	/// Note: The `principal_id` parameter is the cryptographic principal
	/// identifier (hex-encoded public key), NOT a business user ID.
	/// Gateway only understands cryptographic identity, not business user identity.
	pub async fn dispatch_order(
		&self,
		request: PlaceOrderRequest,
		principal_id: String,
		context: RequestContext,
	) -> Result<DispatchResult, DispatcherError> {
		let endpoint = self
			.matching_engines
			.get(&request.market)
			.cloned()
			.ok_or_else(|| DispatcherError::MatchingEngineNotFound(request.market.clone()))?;

		// Convert PlaceOrderRequest to MatchingOrder
		let price = request.price.ok_or_else(|| {
			DispatcherError::DispatchingError("Limit orders require a price".to_string())
		})?;

		let order = MatchingOrder {
			order_id: uuid::Uuid::new_v4().to_string(),
			market: request.market.clone(),
			side: request.side,
			price,
			size: request.size,
			remaining_size: request.size,
			timestamp: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			// Matching engine's Order struct uses `public_key` field to store
			// the cryptographic principal identifier (hex-encoded public key).
			// Gateway only understands cryptographic identity, not business user identity.
			public_key: principal_id,
		};

		let (response_tx, response_rx) = oneshot::channel();
		let job = DispatchJob {
			order,
			endpoint,
			context,
			enqueued_at: Instant::now(),
			response_tx,
		};

		self.queue_tx.try_send(job).map_err(|e| match e {
			mpsc::error::TrySendError::Full(_) => DispatcherError::GatewayOverloaded,
			mpsc::error::TrySendError::Closed(_) => {
				DispatcherError::DispatchingError("Dispatch queue closed".to_string())
			}
		})?;

		let result = tokio::time::timeout(self.queue_timeout, response_rx)
			.await
			.map_err(|_| DispatcherError::QueueTimeout)?;

		result.map_err(|_| {
			DispatcherError::DispatchingError("Dispatcher worker dropped response".to_string())
		})?
	}

	fn spawn_workers(&self, mut queue_rx: mpsc::Receiver<DispatchJob>) {
		let clients = self.clients.clone();
		let rpc_timeout = self.rpc_timeout;

		tokio::spawn(async move {
			while let Some(job) = queue_rx.recv().await {
				let queue_wait = job.enqueued_at.elapsed();
				let context = job.context.clone();

				let mut client = match Self::get_client(&clients, &job.endpoint, rpc_timeout).await
				{
					Ok(client) => client,
					Err(err) => {
						let _ = job.response_tx.send(Err(err));
						continue;
					}
				};

				let rpc_start = Instant::now();
				let result = client.submit_order(job.order.clone(), &context).await;
				let rpc_elapsed = rpc_start.elapsed();

				let timings = DispatchTimings {
					queue_wait_ms: queue_wait.as_millis(),
					rpc_ms: rpc_elapsed.as_millis(),
				};

				let outcome = match result {
					Ok(response) => {
						let disposition = SubmitDisposition::try_from(response.disposition).ok();
						match disposition {
							Some(SubmitDisposition::AcceptedOk) => Ok(DispatchResult {
								order: job.order.clone(),
								timings,
							}),
							Some(SubmitDisposition::OverloadedEngine) => {
								Err(DispatcherError::MatchingOverloaded(response.reason.clone()))
							}
							Some(SubmitDisposition::RejectedOrder)
							| Some(SubmitDisposition::InvalidOrder) => {
								Err(DispatcherError::MatchingRejected(response.reason.clone()))
							}
							Some(SubmitDisposition::InternalError) => {
								Err(DispatcherError::MatchingInternal(response.reason.clone()))
							}
							None => Err(DispatcherError::InvalidResponse(
								"Missing disposition".to_string(),
							)),
						}
					}
					Err(GrpcClientError::Timeout) => Err(DispatcherError::MatchingTimeout),
					Err(GrpcClientError::Transport(e)) => Err(DispatcherError::DispatchingError(e)),
					Err(GrpcClientError::Status(e)) => Err(DispatcherError::MatchingInternal(e)),
					Err(GrpcClientError::Serialization(e)) => {
						Err(DispatcherError::DispatchingError(e))
					}
				};

				let _ = job.response_tx.send(outcome);
			}
		});
	}
}

impl Default for MatchingDispatcher {
	fn default() -> Self {
		panic!("Use MatchingDispatcher::new with configuration")
	}
}
