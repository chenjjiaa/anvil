// Copyright 2025 chenjjiaa
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

use crate::transaction::{Chain, SettlementTransaction, TransactionBuilder, TransactionError};
use anvil_sdk::types::Trade;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Error types for transaction submission
#[derive(Debug, Error)]
pub enum SubmissionError {
	#[error("Transaction error: {0}")]
	Transaction(#[from] TransactionError),
	#[error("Submission failed: {0}")]
	SubmissionFailed(String),
	#[error("Network error: {0}")]
	NetworkError(String),
	#[error("Transaction rejected: {0}")]
	Rejected(String),
}

/// Transaction submission status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubmissionStatus {
	Pending,
	Submitted,
	Confirmed,
	Failed,
}

/// Transaction submission result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionResult {
	/// Transaction hash
	pub tx_hash: String,
	/// Submission status
	pub status: SubmissionStatus,
	/// Number of confirmations (if applicable)
	pub confirmations: u64,
	/// Error message (if failed)
	pub error: Option<String>,
}

/// Settlement submitter that handles transaction submission and confirmation tracking
pub struct SettlementSubmitter {
	/// Chain -> Transaction builder mapping
	builders: HashMap<Chain, TransactionBuilder>,
	/// Chain -> RPC endpoint mapping
	rpc_endpoints: HashMap<Chain, String>,
}

impl SettlementSubmitter {
	/// Create a new settlement submitter
	pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
		let mut builders = HashMap::new();
		builders.insert(Chain::Solana, TransactionBuilder::new(Chain::Solana));
		builders.insert(Chain::Ethereum, TransactionBuilder::new(Chain::Ethereum));

		let mut rpc_endpoints = HashMap::new();
		// TODO: Load from configuration
		rpc_endpoints.insert(Chain::Solana, "http://localhost:8899".to_string());
		rpc_endpoints.insert(Chain::Ethereum, "http://localhost:8545".to_string());

		Ok(Self {
			builders,
			rpc_endpoints,
		})
	}

	/// Submit trades for settlement on a specific chain
	pub async fn submit_trades(
		&self,
		chain: Chain,
		trades: Vec<Trade>,
	) -> Result<SubmissionResult, SubmissionError> {
		// Get transaction builder for this chain
		let builder = self.builders.get(&chain).ok_or_else(|| {
			SubmissionError::SubmissionFailed(format!("Chain {:?} not supported", chain))
		})?;

		// Build the settlement transaction
		let transaction = builder.build_settlement_transaction(trades)?;

		// Submit to the blockchain
		self.submit_transaction(chain, transaction).await
	}

	/// Submit a transaction to the blockchain
	async fn submit_transaction(
		&self,
		chain: Chain,
		transaction: SettlementTransaction,
	) -> Result<SubmissionResult, SubmissionError> {
		// Get RPC endpoint for this chain
		let _endpoint = self.rpc_endpoints.get(&chain).ok_or_else(|| {
			SubmissionError::SubmissionFailed(format!(
				"RPC endpoint not configured for {:?}",
				chain
			))
		})?;

		// TODO: Actually submit transaction via chain-specific RPC
		// For Solana: use solana-client
		// For Ethereum: use ethers-rs

		// Placeholder: return pending status
		Ok(SubmissionResult {
			tx_hash: transaction.tx_hash,
			status: SubmissionStatus::Pending,
			confirmations: 0,
			error: None,
		})
	}

	/// Check the status of a submitted transaction
	pub async fn check_transaction_status(
		&self,
		_chain: Chain,
		tx_hash: &str,
	) -> Result<SubmissionResult, SubmissionError> {
		// TODO: Query blockchain for transaction status
		// This would use chain-specific RPC calls to check confirmation status

		Ok(SubmissionResult {
			tx_hash: tx_hash.to_string(),
			status: SubmissionStatus::Pending,
			confirmations: 0,
			error: None,
		})
	}
}
