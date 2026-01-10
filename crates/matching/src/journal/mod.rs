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

mod memory;

use thiserror::Error;

use crate::types::OrderCommand;
pub use memory::MemoryOrderJournal;

/// Error types for Order Journal operations
#[derive(Debug, Error)]
pub enum JournalError {
	#[error("Failed to append order: {0}")]
	AppendFailed(String),
	#[error("Order already exists: {0}")]
	DuplicateOrder(String),
	#[error("Journal storage error: {0}")]
	StorageError(String),
}

/// Order Journal trait - the idempotency anchor for the matching engine
///
/// The Order Journal records orders that have been accepted by the system
/// but have not yet completed their lifecycle. Its primary purpose is to:
/// - Provide an idempotency anchor (prevent duplicate order processing)
/// - Enable crash recovery by replaying incomplete orders
/// - Define the semantic boundary between "received" and "completed"
///
/// Key semantic constraints:
/// - Orders are append-only; they cannot be modified once written
/// - An order remains "active" until explicitly marked complete via State Journal
/// - The journal does NOT provide delete/remove semantics
/// - Lifecycle completion is determined by State Journal commits, not journal operations
///
/// This abstraction is implementation-agnostic: it can be backed by
/// in-memory structures, mmap files, or external systems like Kafka.
pub trait OrderJournal: Send {
	/// Append an order to the journal
	///
	/// This must complete before ACK is sent to the client.
	/// Returns error if the order_id already exists in active orders.
	fn append(&mut self, order: OrderCommand) -> Result<(), JournalError>;

	/// Check if an order is still active (incomplete lifecycle)
	///
	/// Returns true if the order exists in the journal and has not been
	/// marked complete via mark_completed.
	fn is_active(&self, order_id: &str) -> bool;

	/// Mark an order as completed
	///
	/// Called after the order's final state has been committed to the State Journal.
	/// This allows the Order Journal to eventually clean up/compact old records.
	fn mark_completed(&mut self, order_id: &str);

	/// Replay all active orders for crash recovery
	///
	/// Returns an iterator over orders that were accepted but not yet completed
	/// at the time of the crash.
	fn replay(&self) -> Box<dyn Iterator<Item = OrderCommand> + '_>;

	/// Get the count of active orders
	fn active_count(&self) -> usize;
}
