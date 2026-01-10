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

//! gRPC client for communicating with settlement service

use std::time::Duration;

use thiserror::Error;
use tonic::transport::{Channel, Endpoint};

use anvil_sdk::types::Trade;
use anvil_settlement::transaction::Chain;

// Include generated gRPC code
pub mod proto {
	tonic::include_proto!("anvil.settlement");
}

use proto::settlement_service_client::SettlementServiceClient;
use proto::{
	Chain as ProtoChain, OrderSide as ProtoOrderSide, SubmitTradesRequest, Trade as ProtoTrade,
};

/// Error types for gRPC client operations
#[derive(Debug, Error)]
pub enum SettlementClientError {
	#[error("gRPC transport error: {0}")]
	Transport(String),
	#[error("gRPC status error: {0}")]
	Status(String),
	#[error("Serialization error: {0}")]
	Serialization(String),
}

/// gRPC client for settlement service
#[derive(Clone)]
pub struct SettlementGrpcClient {
	client: SettlementServiceClient<Channel>,
}

impl SettlementGrpcClient {
	/// Create a new gRPC client
	pub async fn new(endpoint: &str) -> Result<Self, SettlementClientError> {
		let channel = Endpoint::from_shared(endpoint.to_string())
			.map_err(|e| SettlementClientError::Transport(format!("Invalid endpoint: {}", e)))?
			.timeout(Duration::from_secs(10))
			.connect()
			.await
			.map_err(|e| SettlementClientError::Transport(format!("Connection failed: {}", e)))?;

		Ok(Self {
			client: SettlementServiceClient::new(channel),
		})
	}

	/// Submit matched trades for settlement
	pub async fn submit_trades(
		&mut self,
		market: &str,
		trades: Vec<Trade>,
		chain: Chain,
	) -> Result<String, SettlementClientError> {
		// Convert internal trades to proto trades
		let proto_trades: Vec<ProtoTrade> = trades
			.iter()
			.map(|t| ProtoTrade {
				trade_id: t.trade_id.clone(),
				market: t.market.clone(),
				price: t.price,
				size: t.size,
				side: match t.side {
					anvil_sdk::types::Side::Buy => ProtoOrderSide::Buy as i32,
					anvil_sdk::types::Side::Sell => ProtoOrderSide::Sell as i32,
				},
				timestamp: t.timestamp,
				maker_order_id: t.maker_order_id.clone(),
				taker_order_id: t.taker_order_id.clone(),
			})
			.collect();

		let request = SubmitTradesRequest {
			market: market.to_string(),
			trades: proto_trades,
			chain: match chain {
				Chain::Solana => ProtoChain::Solana as i32,
				Chain::Ethereum => ProtoChain::Ethereum as i32,
			},
		};

		let response = self
			.client
			.submit_trades(tonic::Request::new(request))
			.await
			.map_err(|e| SettlementClientError::Status(format!("gRPC error: {}", e)))?
			.into_inner();

		Ok(response.tx_hash)
	}
}
