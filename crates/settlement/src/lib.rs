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

//! Anvil Settlement Core
//!
//! This crate provides trade validation, transaction construction,
//! and blockchain submission functionality.

pub mod chains;
pub mod config;
pub mod server;
pub mod submitter;
pub mod transaction;
pub mod validator;

pub use anvil_sdk::types::Trade;
pub use submitter::{SettlementSubmitter, SubmissionResult, SubmissionStatus};
pub use transaction::Chain;
pub use transaction::{SettlementTransaction, TransactionBuilder, TransactionError};
pub use validator::{ValidationError, validate_trade, validate_trades};
