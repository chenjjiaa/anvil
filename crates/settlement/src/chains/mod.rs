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

//! Chain-specific transaction builders

pub mod ethereum;
pub mod solana;

use crate::transaction::{Chain, SettlementTransaction, TransactionError};
use anvil_sdk::types::Trade;

/// Build a settlement transaction for a specific chain
pub async fn build_transaction(
	chain: Chain,
	trades: Vec<Trade>,
) -> Result<SettlementTransaction, TransactionError> {
	match chain {
		Chain::Solana => solana::build_solana_transaction(trades).await,
		Chain::Ethereum => ethereum::build_ethereum_transaction(trades).await,
	}
}
