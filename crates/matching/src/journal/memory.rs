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

use std::collections::HashMap;

use super::{JournalError, OrderJournal};
use crate::types::OrderCommand;

/// In-memory implementation of Order Journal for MVP
///
/// This implementation provides a lightweight, non-persistent journal
/// suitable for initial development and testing. It maintains orders
/// in memory with minimal overhead.
///
/// Characteristics:
/// - No fsync or durability guarantees
/// - Fast append and lookup operations
/// - Simple HashMap-based storage
/// - Lifecycle: covers only "received -> completed" interval
///
/// Future evolution paths:
/// - Add mmap-backed storage for crash persistence
/// - Implement truncation/compaction for long-running systems
/// - Replace with external log system (Kafka, etc.)
pub struct MemoryOrderJournal {
	/// Active orders indexed by order_id
	active_orders: HashMap<String, OrderCommand>,
	/// Completed order IDs for cleanup tracking
	completed_orders: Vec<String>,
}

impl MemoryOrderJournal {
	pub fn new() -> Self {
		Self {
			active_orders: HashMap::new(),
			completed_orders: Vec::new(),
		}
	}

	/// Perform cleanup of completed orders
	///
	/// This can be called periodically to reclaim memory.
	/// In production, this would be coordinated with State Journal commits.
	pub fn compact(&mut self) {
		for order_id in self.completed_orders.drain(..) {
			self.active_orders.remove(&order_id);
		}
	}
}

impl Default for MemoryOrderJournal {
	fn default() -> Self {
		Self::new()
	}
}

impl OrderJournal for MemoryOrderJournal {
	fn append(&mut self, order: OrderCommand) -> Result<(), JournalError> {
		if self.active_orders.contains_key(&order.order_id) {
			return Err(JournalError::DuplicateOrder(order.order_id.clone()));
		}

		self.active_orders.insert(order.order_id.clone(), order);
		Ok(())
	}

	fn is_active(&self, order_id: &str) -> bool {
		self.active_orders.contains_key(order_id)
	}

	fn mark_completed(&mut self, order_id: &str) {
		if self.active_orders.contains_key(order_id) {
			self.completed_orders.push(order_id.to_string());
		}
	}

	fn replay(&self) -> Box<dyn Iterator<Item = OrderCommand> + '_> {
		Box::new(self.active_orders.values().cloned())
	}

	fn active_count(&self) -> usize {
		self.active_orders.len()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anvil_sdk::types::Side;

	fn create_test_order(order_id: &str, market: &str) -> OrderCommand {
		OrderCommand {
			order_id: order_id.to_string(),
			market: market.to_string(),
			side: Side::Buy,
			price: 50000,
			size: 1,
			timestamp: 1000,
			public_key: "test_key".to_string(),
		}
	}

	#[test]
	fn test_append_and_is_active() {
		let mut journal = MemoryOrderJournal::new();
		let order = create_test_order("order_1", "BTC-USDT");

		assert!(!journal.is_active("order_1"));

		journal.append(order.clone()).unwrap();
		assert!(journal.is_active("order_1"));
		assert_eq!(journal.active_count(), 1);
	}

	#[test]
	fn test_duplicate_order_rejected() {
		let mut journal = MemoryOrderJournal::new();
		let order = create_test_order("order_1", "BTC-USDT");

		journal.append(order.clone()).unwrap();
		let result = journal.append(order.clone());

		assert!(result.is_err());
		assert!(matches!(result, Err(JournalError::DuplicateOrder(_))));
	}

	#[test]
	fn test_mark_completed() {
		let mut journal = MemoryOrderJournal::new();
		let order = create_test_order("order_1", "BTC-USDT");

		journal.append(order).unwrap();
		assert!(journal.is_active("order_1"));

		journal.mark_completed("order_1");
		// Still active until compact is called
		assert!(journal.is_active("order_1"));

		journal.compact();
		assert!(!journal.is_active("order_1"));
		assert_eq!(journal.active_count(), 0);
	}

	#[test]
	fn test_replay() {
		let mut journal = MemoryOrderJournal::new();

		for i in 0..5 {
			let order = create_test_order(&format!("order_{}", i), "BTC-USDT");
			journal.append(order).unwrap();
		}

		let replayed: Vec<_> = journal.replay().collect();
		assert_eq!(replayed.len(), 5);

		// Mark some as completed
		journal.mark_completed("order_0");
		journal.mark_completed("order_2");
		journal.compact();

		let replayed_after_compact: Vec<_> = journal.replay().collect();
		assert_eq!(replayed_after_compact.len(), 3);
	}
}
