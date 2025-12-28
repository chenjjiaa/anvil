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

//! gRPC client for communicating with matching engine

// Include generated gRPC code from matching proto
pub mod proto {
	tonic::include_proto!("anvil.matching");
}

use anvil_sdk::types::{Order, OrderStatus, Side};
use proto::matching_service_client::MatchingServiceClient;
use proto::{
	OrderSide as ProtoOrderSide, OrderStatus as ProtoOrderStatus, SubmitOrderRequest,
	SubmitOrderResponse,
};
use std::time::Duration;
use thiserror::Error;
use tonic::transport::{Channel, Endpoint};

/// Error types for gRPC client operations
#[derive(Debug, Error)]
pub enum GrpcClientError {
	#[error("gRPC transport error: {0}")]
	Transport(String),
	#[error("gRPC status error: {0}")]
	Status(String),
	#[error("Serialization error: {0}")]
	#[allow(dead_code)]
	Serialization(String),
}

/// gRPC client for matching engine
#[derive(Clone)]
pub struct MatchingGrpcClient {
	client: MatchingServiceClient<Channel>,
}

impl MatchingGrpcClient {
	/// Create a new gRPC client
	pub async fn new(endpoint: &str) -> Result<Self, GrpcClientError> {
		let channel = Endpoint::from_shared(endpoint.to_string())
			.map_err(|e| GrpcClientError::Transport(format!("Invalid endpoint: {}", e)))?
			.timeout(Duration::from_secs(5))
			.connect()
			.await
			.map_err(|e| GrpcClientError::Transport(format!("Connection failed: {}", e)))?;

		Ok(Self {
			client: MatchingServiceClient::new(channel),
		})
	}

	/// Submit an order to the matching engine
	pub async fn submit_order(
		&mut self,
		order: anvil_matching::types::Order,
	) -> Result<SubmitOrderResponse, GrpcClientError> {
		let request = SubmitOrderRequest {
			order_id: order.order_id.clone(),
			market: order.market.clone(),
			side: match order.side {
				Side::Buy => ProtoOrderSide::Buy as i32,
				Side::Sell => ProtoOrderSide::Sell as i32,
			},
			price: order.price,
			size: order.size,
			remaining_size: order.remaining_size,
			timestamp: order.timestamp,
			user_id: order.user_id.clone(),
		};

		let response = self
			.client
			.submit_order(tonic::Request::new(request))
			.await
			.map_err(|e| GrpcClientError::Status(format!("gRPC error: {}", e)))?
			.into_inner();

		Ok(response)
	}

	/// Get order status
	#[allow(dead_code)]
	pub async fn get_order(&mut self, order_id: &str) -> Result<Order, GrpcClientError> {
		use proto::GetOrderRequest;
		let request = GetOrderRequest {
			order_id: order_id.to_string(),
		};

		let response = self
			.client
			.get_order(tonic::Request::new(request))
			.await
			.map_err(|e| GrpcClientError::Status(format!("gRPC error: {}", e)))?
			.into_inner();

		// Convert proto order to SDK order
		let proto_order = response
			.order
			.ok_or_else(|| GrpcClientError::Serialization("Order not found".to_string()))?;

		let market = proto_order.market.clone();
		let side = match proto_order.side() {
			ProtoOrderSide::Buy => Side::Buy,
			ProtoOrderSide::Sell => Side::Sell,
		};
		let status = match proto_order.status() {
			ProtoOrderStatus::Pending => OrderStatus::Pending,
			ProtoOrderStatus::Accepted => OrderStatus::Accepted,
			ProtoOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
			ProtoOrderStatus::Filled => OrderStatus::Filled,
			ProtoOrderStatus::Cancelled => OrderStatus::Cancelled,
			ProtoOrderStatus::Rejected => OrderStatus::Rejected,
		};

		Ok(Order {
			order_id: proto_order.order_id,
			market,
			side,
			order_type: anvil_sdk::types::OrderType::Limit, // TODO: Add to proto
			price: Some(proto_order.price),
			size: proto_order.size,
			filled_size: proto_order.filled_size,
			remaining_size: proto_order.remaining_size,
			status,
			created_at: proto_order.created_at,
		})
	}

	/// Cancel an order
	#[allow(dead_code)]
	pub async fn cancel_order(
		&mut self,
		market: &str,
		side: Side,
		order_id: &str,
	) -> Result<bool, GrpcClientError> {
		use proto::CancelOrderRequest;

		let request = CancelOrderRequest {
			order_id: order_id.to_string(),
			market: market.to_string(),
			side: match side {
				Side::Buy => ProtoOrderSide::Buy as i32,
				Side::Sell => ProtoOrderSide::Sell as i32,
			},
		};

		let response = self
			.client
			.cancel_order(tonic::Request::new(request))
			.await
			.map_err(|e| GrpcClientError::Status(format!("gRPC error: {}", e)))?
			.into_inner();

		Ok(response.success)
	}
}
