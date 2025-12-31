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

use std::{collections::HashMap, sync::Arc};

use anvil_matching::types::Order as MatchingOrder;
use anvil_sdk::types::PlaceOrderRequest;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::grpc_client::{GrpcClientError, MatchingGrpcClient};

/// Error types for dispatching operations
#[derive(Debug, Error)]
pub enum DispatcherError {
	#[error("Matching engine not found for market: {0}")]
	MatchingEngineNotFound(String),
	#[error("Matching confirmation not received within gateway timeout")]
	MatchingTimeout,
	#[error("Dispatching error: {0}")]
	DispatchingError(String),
	#[error("gRPC client error: {0}")]
	GrpcClient(#[from] GrpcClientError),
}

/// Dispatcher that forwards orders to the appropriate matching engine
///
/// Uses gRPC to communicate with matching engines.
pub struct MatchingDispatcher {
	/// Market -> Matching engine endpoint mapping
	matching_engines: HashMap<String, String>,
	/// Market -> gRPC client mapping (with mutex for async access)
	clients: Arc<Mutex<HashMap<String, MatchingGrpcClient>>>,
}

impl MatchingDispatcher {
	/// Create a new dispatcher
	pub fn new() -> Self {
		let mut engines = HashMap::new();
		// TODO: Load from configuration
		engines.insert("BTC-USDT".to_string(), "http://localhost:50051".to_string());

		tracing::info!(
			target: "server::dispatcher",
			"MatchingDispatcher initialized with {} markets",
			engines.len()
		);

		Self {
			matching_engines: engines,
			clients: Arc::new(Mutex::new(HashMap::new())),
		}
	}

	/// Get or create gRPC client for a market
	async fn get_client(&self, market: &str) -> Result<MatchingGrpcClient, DispatcherError> {
		let endpoint = self
			.matching_engines
			.get(market)
			.ok_or_else(|| DispatcherError::MatchingEngineNotFound(market.to_string()))?;

		let mut clients = self.clients.lock().await;

		if let Some(client) = clients.get(market) {
			// Clone the client (tonic clients are cheap to clone)
			Ok(client.clone())
		} else {
			// Create new client
			let client = MatchingGrpcClient::new(endpoint).await.map_err(|e| {
				DispatcherError::DispatchingError(format!("Failed to create client: {}", e))
			})?;
			let client_clone = client.clone();
			clients.insert(market.to_string(), client);
			Ok(client_clone)
		}
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
	) -> Result<MatchingOrder, DispatcherError> {
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

		// Get gRPC client and submit order
		let mut client = self.get_client(&request.market).await?;
		match client.submit_order(order.clone()).await {
			Ok(_response) => {}
			Err(GrpcClientError::Timeout) => return Err(DispatcherError::MatchingTimeout),
			Err(e) => return Err(DispatcherError::GrpcClient(e)),
		}

		Ok(order)
	}
}

impl Default for MatchingDispatcher {
	fn default() -> Self {
		Self::new()
	}
}
