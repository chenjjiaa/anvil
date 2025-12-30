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

use crate::Matcher;
use crate::types::Order;
use anvil_sdk::types::Side;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

// Include generated gRPC code
pub mod proto {
	tonic::include_proto!("anvil.matching");
}

use proto::matching_service_server::{MatchingService, MatchingServiceServer};
use proto::{
	CancelOrderRequest, CancelOrderResponse, GetOrderRequest, GetOrderResponse, MatchedTrade,
	OrderSide as ProtoOrderSide, OrderStatus as ProtoOrderStatus, StreamMatchedTradesRequest,
	SubmitOrderRequest, SubmitOrderResponse, Trade as ProtoTrade,
};
use tokio_stream;

/// Matching service implementation
pub struct MatchingServiceImpl {
	matcher: Arc<RwLock<Matcher>>,
}

impl MatchingServiceImpl {
	pub fn new(matcher: Arc<RwLock<Matcher>>) -> Self {
		Self { matcher }
	}
}

#[tonic::async_trait]
impl MatchingService for MatchingServiceImpl {
	async fn submit_order(
		&self,
		request: Request<SubmitOrderRequest>,
	) -> Result<Response<SubmitOrderResponse>, Status> {
		let req = request.into_inner();

		// Convert proto order to internal order
		let order = Order {
			order_id: req.order_id.clone(),
			market: req.market.clone(),
			side: match req.side() {
				ProtoOrderSide::Buy => Side::Buy,
				ProtoOrderSide::Sell => Side::Sell,
			},
			price: req.price,
			size: req.size,
			remaining_size: req.remaining_size,
			timestamp: req.timestamp,
			public_key: req.public_key.clone(),
		};

		// Submit to matcher
		let matcher = self.matcher.read().await;
		let result = matcher
			.match_order(order)
			.map_err(|e| Status::internal(format!("Matching error: {}", e)))?;

		// Send matched trades to settlement if any
		if !result.trades.is_empty() {
			// TODO: Send to settlement via gRPC client
			// This would be done asynchronously to not block the response
		}

		// Convert trades to proto
		let proto_trades: Vec<ProtoTrade> = result
			.trades
			.iter()
			.map(|t| ProtoTrade {
				trade_id: t.trade_id.clone(),
				market: t.market.clone(),
				price: t.price,
				size: t.size,
				side: match t.side {
					Side::Buy => ProtoOrderSide::Buy as i32,
					Side::Sell => ProtoOrderSide::Sell as i32,
				},
				timestamp: t.timestamp,
				maker_order_id: t.maker_order_id.clone(),
				taker_order_id: t.taker_order_id.clone(),
			})
			.collect();

		// Determine status
		let status = if result.fully_filled {
			ProtoOrderStatus::Filled
		} else if result.partially_filled {
			ProtoOrderStatus::PartiallyFilled
		} else {
			ProtoOrderStatus::Accepted
		};

		Ok(Response::new(SubmitOrderResponse {
			order_id: result.order.order_id,
			status: status as i32,
			trades: proto_trades,
			fully_filled: result.fully_filled,
			partially_filled: result.partially_filled,
		}))
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
		request: Request<CancelOrderRequest>,
	) -> Result<Response<CancelOrderResponse>, Status> {
		let req = request.into_inner();

		let side = match req.side() {
			ProtoOrderSide::Buy => Side::Buy,
			ProtoOrderSide::Sell => Side::Sell,
		};

		let matcher = self.matcher.write().await;
		let result = matcher
			.cancel_order(&req.market, side, &req.order_id)
			.map_err(|e| Status::internal(format!("Cancel error: {}", e)))?;

		Ok(Response::new(CancelOrderResponse {
			success: result.is_some(),
			order_id: req.order_id,
		}))
	}

	type StreamMatchedTradesStream =
		tokio_stream::wrappers::ReceiverStream<Result<MatchedTrade, Status>>;

	async fn stream_matched_trades(
		&self,
		_request: Request<StreamMatchedTradesRequest>,
	) -> Result<Response<Self::StreamMatchedTradesStream>, Status> {
		// TODO: Implement streaming
		let (_tx, rx) = tokio::sync::mpsc::channel(128);
		Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
			rx,
		)))
	}
}

/// Create matching service server
pub fn create_server(matcher: Arc<RwLock<Matcher>>) -> MatchingServiceServer<MatchingServiceImpl> {
	MatchingServiceServer::new(MatchingServiceImpl::new(matcher))
}
