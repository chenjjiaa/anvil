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

use crate::signing::{SignatureAlgorithm, sign_order_request};
use crate::types::{Order, PlaceOrderRequest, PlaceOrderResponse};
use reqwest::Client as ReqwestClient;
use std::time::Duration;
use thiserror::Error;

/// Error types for client operations
#[derive(Debug, Error)]
pub enum ClientError {
	#[error("Network error: {0}")]
	Network(String),
	#[error("Serialization error: {0}")]
	Serialization(String),
	#[error("Server error: {0}")]
	Server(String),
	#[error("Authentication error: {0}")]
	Authentication(String),
	#[error("Invalid response: {0}")]
	InvalidResponse(String),
}

/// Client for interacting with the order gateway
///
/// This is an async client interface using reqwest for HTTP communication.
pub struct Client {
	base_url: String,
	client: ReqwestClient,
}

impl Client {
	/// Create a new client with the given base URL
	pub fn new(base_url: impl Into<String>) -> Self {
		let client = ReqwestClient::builder()
			.timeout(Duration::from_secs(30))
			.build()
			.expect("Failed to create HTTP client");

		Self {
			base_url: base_url.into(),
			client,
		}
	}

	/// Create a new client with custom configuration
	pub fn with_config(base_url: impl Into<String>, timeout: Duration) -> Self {
		let client = ReqwestClient::builder()
			.timeout(timeout)
			.build()
			.expect("Failed to create HTTP client");

		Self {
			base_url: base_url.into(),
			client,
		}
	}

	/// Place an order
	///
	/// This method sends an authenticated order request to the gateway.
	/// The order is validated, sequenced, and matched off-chain before
	/// any blockchain interaction occurs.
	pub async fn place_order(
		&self,
		request: PlaceOrderRequest,
	) -> Result<PlaceOrderResponse, ClientError> {
		// Sign the request if private key is provided
		// Note: In a real implementation, the private key would be passed separately
		// For now, we assume the request already has a signature

		let url = format!("{}/api/v1/orders", self.base_url);

		let response = self
			.client
			.post(&url)
			.json(&request)
			.send()
			.await
			.map_err(|e| ClientError::Network(format!("Request failed: {}", e)))?;

		if !response.status().is_success() {
			let status = response.status();
			let error_text = response
				.text()
				.await
				.unwrap_or_else(|_| format!("HTTP {}", status));
			return Err(ClientError::Server(format!("{}: {}", status, error_text)));
		}

		let order_response: PlaceOrderResponse = response
			.json()
			.await
			.map_err(|e| ClientError::Serialization(format!("Failed to parse response: {}", e)))?;

		Ok(order_response)
	}

	/// Place an order with automatic signing
	pub async fn place_order_signed(
		&self,
		mut request: PlaceOrderRequest,
		private_key: &[u8],
		algorithm: SignatureAlgorithm,
	) -> Result<PlaceOrderResponse, ClientError> {
		// Sign the request
		let signature = sign_order_request(&request, private_key, algorithm)
			.map_err(|e| ClientError::Authentication(format!("Signing failed: {}", e)))?;
		request.signature = signature;

		self.place_order(request).await
	}

	/// Get order status by order ID
	pub async fn get_order(&self, order_id: &str) -> Result<Order, ClientError> {
		let url = format!("{}/api/v1/orders/{}", self.base_url, order_id);

		let response = self
			.client
			.get(&url)
			.send()
			.await
			.map_err(|e| ClientError::Network(format!("Request failed: {}", e)))?;

		if !response.status().is_success() {
			let status = response.status();
			let error_text = response
				.text()
				.await
				.unwrap_or_else(|_| format!("HTTP {}", status));
			return Err(ClientError::Server(format!("{}: {}", status, error_text)));
		}

		let order: Order = response
			.json()
			.await
			.map_err(|e| ClientError::Serialization(format!("Failed to parse response: {}", e)))?;

		Ok(order)
	}

	/// Cancel an order
	pub async fn cancel_order(&self, order_id: &str) -> Result<(), ClientError> {
		let url = format!("{}/api/v1/orders/{}", self.base_url, order_id);

		let response = self
			.client
			.delete(&url)
			.send()
			.await
			.map_err(|e| ClientError::Network(format!("Request failed: {}", e)))?;

		if !response.status().is_success() {
			let status = response.status();
			let error_text = response
				.text()
				.await
				.unwrap_or_else(|_| format!("HTTP {}", status));
			return Err(ClientError::Server(format!("{}: {}", status, error_text)));
		}

		Ok(())
	}

	/// Check gateway health
	pub async fn health_check(&self) -> Result<bool, ClientError> {
		let url = format!("{}/health", self.base_url);

		let response = self
			.client
			.get(&url)
			.send()
			.await
			.map_err(|e| ClientError::Network(format!("Request failed: {}", e)))?;

		Ok(response.status().is_success())
	}
}

/// Synchronous client wrapper (for compatibility)
///
/// This wraps the async client and runs it in a tokio runtime.
/// For new code, prefer using the async Client directly.
pub struct SyncClient {
	client: Client,
	runtime: tokio::runtime::Runtime,
}

impl SyncClient {
	/// Create a new synchronous client
	pub fn new(base_url: impl Into<String>) -> anyhow::Result<Self> {
		let runtime = tokio::runtime::Runtime::new()
			.map_err(|e| anyhow::anyhow!("Failed to create tokio runtime: {}", e))?;
		Ok(Self {
			client: Client::new(base_url),
			runtime,
		})
	}

	/// Place an order (synchronous)
	pub fn place_order(
		&self,
		request: PlaceOrderRequest,
	) -> Result<PlaceOrderResponse, ClientError> {
		self.runtime.block_on(self.client.place_order(request))
	}

	/// Get order status (synchronous)
	pub fn get_order(&self, order_id: &str) -> Result<Order, ClientError> {
		self.runtime.block_on(self.client.get_order(order_id))
	}

	/// Cancel an order (synchronous)
	pub fn cancel_order(&self, order_id: &str) -> Result<(), ClientError> {
		self.runtime.block_on(self.client.cancel_order(order_id))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_client_creation() {
		let client = Client::new("http://localhost:8080");
		assert_eq!(client.base_url, "http://localhost:8080");
	}

	#[test]
	fn test_sync_client_creation() {
		let client = SyncClient::new("http://localhost:8080");
		assert!(client.is_ok());
	}
}
