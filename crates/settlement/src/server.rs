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

//! gRPC server for settlement service

use crate::submitter::SettlementSubmitter;
use crate::transaction::Chain;
use crate::validator::validate_trades;
use anvil_sdk::types::Trade;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

// Include generated gRPC code
pub mod proto {
	tonic::include_proto!("anvil.settlement");
}

use proto::settlement_service_server::{SettlementService, SettlementServiceServer};
use proto::{
	Chain as ProtoChain, GetTransactionStatusRequest, GetTransactionStatusResponse,
	OrderSide as ProtoOrderSide, SubmissionStatus as ProtoSubmissionStatus, SubmitTradesRequest,
	SubmitTradesResponse,
};

/// Settlement service implementation
pub struct SettlementServiceImpl {
	submitter: Arc<RwLock<SettlementSubmitter>>,
}

impl SettlementServiceImpl {
	pub fn new(submitter: Arc<RwLock<SettlementSubmitter>>) -> Self {
		Self { submitter }
	}
}

#[tonic::async_trait]
impl SettlementService for SettlementServiceImpl {
	async fn submit_trades(
		&self,
		request: Request<SubmitTradesRequest>,
	) -> Result<Response<SubmitTradesResponse>, Status> {
		let req = request.into_inner();

		// Convert proto trades to internal trades
		let trades: Vec<Trade> = req
			.trades
			.iter()
			.map(|t| Trade {
				trade_id: t.trade_id.clone(),
				market: t.market.clone(),
				price: t.price,
				size: t.size,
				side: match t.side() {
					ProtoOrderSide::Buy => anvil_sdk::types::Side::Buy,
					ProtoOrderSide::Sell => anvil_sdk::types::Side::Sell,
				},
				timestamp: t.timestamp,
				maker_order_id: t.maker_order_id.clone(),
				taker_order_id: t.taker_order_id.clone(),
			})
			.collect();

		// Validate trades
		validate_trades(&trades)
			.map_err(|e| Status::invalid_argument(format!("Trade validation failed: {}", e)))?;

		// Convert chain
		let chain = match req.chain() {
			ProtoChain::Solana => Chain::Solana,
			ProtoChain::Ethereum => Chain::Ethereum,
		};

		// Submit trades
		let submitter = self.submitter.read().await;
		let result = submitter
			.submit_trades(chain, trades)
			.await
			.map_err(|e| Status::internal(format!("Submission failed: {}", e)))?;

		// Convert status
		let status = match result.status {
			crate::submitter::SubmissionStatus::Pending => ProtoSubmissionStatus::Pending,
			crate::submitter::SubmissionStatus::Submitted => ProtoSubmissionStatus::Submitted,
			crate::submitter::SubmissionStatus::Confirmed => ProtoSubmissionStatus::Confirmed,
			crate::submitter::SubmissionStatus::Failed => ProtoSubmissionStatus::Failed,
		};

		Ok(Response::new(SubmitTradesResponse {
			tx_hash: result.tx_hash,
			status: status as i32,
			confirmations: result.confirmations,
			error: result.error.unwrap_or_default(),
		}))
	}

	async fn get_transaction_status(
		&self,
		request: Request<GetTransactionStatusRequest>,
	) -> Result<Response<GetTransactionStatusResponse>, Status> {
		let req = request.into_inner();

		let chain = match req.chain() {
			ProtoChain::Solana => Chain::Solana,
			ProtoChain::Ethereum => Chain::Ethereum,
		};

		let submitter = self.submitter.read().await;
		let result = submitter
			.check_transaction_status(chain, &req.tx_hash)
			.await
			.map_err(|e| Status::internal(format!("Status check failed: {}", e)))?;

		let status = match result.status {
			crate::submitter::SubmissionStatus::Pending => ProtoSubmissionStatus::Pending,
			crate::submitter::SubmissionStatus::Submitted => ProtoSubmissionStatus::Submitted,
			crate::submitter::SubmissionStatus::Confirmed => ProtoSubmissionStatus::Confirmed,
			crate::submitter::SubmissionStatus::Failed => ProtoSubmissionStatus::Failed,
		};

		Ok(Response::new(GetTransactionStatusResponse {
			tx_hash: result.tx_hash,
			status: status as i32,
			confirmations: result.confirmations,
			error: result.error.unwrap_or_default(),
		}))
	}
}

/// Create settlement service server
pub fn create_server(
	submitter: Arc<RwLock<SettlementSubmitter>>,
) -> SettlementServiceServer<SettlementServiceImpl> {
	SettlementServiceServer::new(SettlementServiceImpl::new(submitter))
}
