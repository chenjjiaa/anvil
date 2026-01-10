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

use std::sync::{Arc, Mutex};

use thiserror::Error;

use super::{EventBatch, MatchingEvent, SequenceNumber};

/// Error types for event storage operations
#[derive(Debug, Error)]
pub enum StorageError {
	#[error("Failed to write events: {0}")]
	WriteFailed(String),
	#[error("Failed to read events: {0}")]
	ReadFailed(String),
	#[error("Storage corrupted: {0}")]
	Corrupted(String),
}

/// Event Storage trait - persistence layer for matching events
///
/// The Event Storage is responsible for durably persisting events
/// that represent the single source of truth for matching engine state.
///
/// Key properties:
/// - Append-only: events are never modified after writing
/// - Ordered: events can be replayed in sequence order
/// - Durable: events survive crashes (implementation-dependent)
///
/// This abstraction allows different backing stores:
/// - In-memory Vec (MVP, testing)
/// - File-based append log
/// - External systems (Kafka, etc.)
pub trait EventStorage: Send {
	/// Append a batch of events to storage
	///
	/// All events in the batch should be written atomically if possible.
	/// Returns the sequence number of the last committed event.
	fn append_batch(&mut self, batch: EventBatch) -> Result<SequenceNumber, StorageError>;

	/// Replay events from a given sequence number
	///
	/// Used during crash recovery to rebuild orderbook state.
	/// Returns all events with sequence >= from_seq.
	fn replay_from(&self, from_seq: SequenceNumber) -> Result<Vec<MatchingEvent>, StorageError>;

	/// Get the sequence number of the last committed event
	fn last_sequence(&self) -> SequenceNumber;

	/// Get total count of stored events
	fn event_count(&self) -> usize;
}

/// In-memory event storage for MVP
///
/// This implementation stores all events in memory with no durability.
/// Suitable for:
/// - Development and testing
/// - MVP deployment where crash recovery is not critical
/// - Benchmarking matching logic without I/O overhead
///
/// Not suitable for production use where crash recovery is required.
pub struct MemoryEventStorage {
	events: Arc<Mutex<Vec<MatchingEvent>>>,
	last_seq: Arc<Mutex<SequenceNumber>>,
}

impl MemoryEventStorage {
	pub fn new() -> Self {
		Self {
			events: Arc::new(Mutex::new(Vec::new())),
			last_seq: Arc::new(Mutex::new(0)),
		}
	}

	/// Clear all stored events (for testing)
	#[cfg(test)]
	pub fn clear(&mut self) {
		self.events.lock().unwrap().clear();
		*self.last_seq.lock().unwrap() = 0;
	}
}

impl Default for MemoryEventStorage {
	fn default() -> Self {
		Self::new()
	}
}

impl EventStorage for MemoryEventStorage {
	fn append_batch(&mut self, batch: EventBatch) -> Result<SequenceNumber, StorageError> {
		if batch.is_empty() {
			return Ok(self.last_sequence());
		}

		let mut events = self.events.lock().unwrap();
		let mut last_seq = self.last_seq.lock().unwrap();

		for event in batch.events {
			events.push(event.clone());
			*last_seq = event.sequence();
		}

		Ok(*last_seq)
	}

	fn replay_from(&self, from_seq: SequenceNumber) -> Result<Vec<MatchingEvent>, StorageError> {
		let events = self.events.lock().unwrap();
		Ok(events
			.iter()
			.filter(|e| e.sequence() >= from_seq)
			.cloned()
			.collect())
	}

	fn last_sequence(&self) -> SequenceNumber {
		*self.last_seq.lock().unwrap()
	}

	fn event_count(&self) -> usize {
		self.events.lock().unwrap().len()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anvil_sdk::types::Side;

	fn create_test_event(seq: u64) -> MatchingEvent {
		MatchingEvent::OrderAccepted {
			seq,
			order_id: format!("order_{}", seq),
			market: "BTC-USDT".to_string(),
			side: Side::Buy,
			price: 50000,
			size: 1,
			timestamp: 1000,
		}
	}

	#[test]
	fn test_append_and_replay() {
		let mut storage = MemoryEventStorage::new();

		let events = vec![create_test_event(1), create_test_event(2)];
		let batch = EventBatch::new(events);

		let last_seq = storage.append_batch(batch).unwrap();
		assert_eq!(last_seq, 2);
		assert_eq!(storage.event_count(), 2);

		let replayed = storage.replay_from(1).unwrap();
		assert_eq!(replayed.len(), 2);

		let replayed_from_2 = storage.replay_from(2).unwrap();
		assert_eq!(replayed_from_2.len(), 1);
	}

	#[test]
	fn test_last_sequence() {
		let mut storage = MemoryEventStorage::new();
		assert_eq!(storage.last_sequence(), 0);

		storage
			.append_batch(EventBatch::new(vec![create_test_event(5)]))
			.unwrap();
		assert_eq!(storage.last_sequence(), 5);

		storage
			.append_batch(EventBatch::new(vec![create_test_event(10)]))
			.unwrap();
		assert_eq!(storage.last_sequence(), 10);
	}
}
