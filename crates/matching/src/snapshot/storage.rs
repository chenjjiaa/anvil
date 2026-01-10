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

use serde::{Deserialize, Serialize};

use super::SnapshotError;
use crate::event::SequenceNumber;

/// Metadata about a snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
	/// When the snapshot was created (unix timestamp)
	pub created_at: u64,
	/// Event sequence number aligned with this snapshot
	pub event_seq: SequenceNumber,
	/// Size of the snapshot in bytes
	pub size_bytes: usize,
	/// Market covered by this snapshot
	pub market: String,
}

/// Complete snapshot with metadata and data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
	pub metadata: SnapshotMetadata,
	pub state_data: Vec<u8>,
}

/// Snapshot Storage trait - persistence layer for snapshots
///
/// The Snapshot Storage is responsible for saving and loading
/// orderbook snapshots. Unlike Event Storage (which is append-only),
/// snapshot storage typically maintains only recent snapshots and
/// can delete old ones.
///
/// This abstraction allows different backing stores:
/// - In-memory (MVP, testing)
/// - Local filesystem (JSON or binary)
/// - Object storage (S3, etc.)
/// - Database (for queryable snapshots)
pub trait SnapshotStorage: Send {
	/// Save a snapshot
	fn save(&mut self, snapshot: Snapshot) -> Result<(), SnapshotError>;

	/// Load the latest snapshot
	fn load_latest(&self) -> Result<Snapshot, SnapshotError>;

	/// Load a snapshot at or before a given sequence number
	fn load_at_seq(&self, seq: SequenceNumber) -> Result<Snapshot, SnapshotError>;

	/// List all available snapshots
	fn list_snapshots(&self) -> Vec<SnapshotMetadata>;

	/// Delete snapshots older than the given sequence number
	fn cleanup_before(&mut self, seq: SequenceNumber) -> Result<usize, SnapshotError>;
}

/// In-memory snapshot storage for MVP
///
/// Stores snapshots in memory only. Suitable for:
/// - Development and testing
/// - MVP deployment where crash recovery uses event replay only
/// - Benchmarking without I/O
pub struct MemorySnapshotStorage {
	snapshots: Arc<Mutex<Vec<Snapshot>>>,
}

impl MemorySnapshotStorage {
	pub fn new() -> Self {
		Self {
			snapshots: Arc::new(Mutex::new(Vec::new())),
		}
	}
}

impl Default for MemorySnapshotStorage {
	fn default() -> Self {
		Self::new()
	}
}

impl SnapshotStorage for MemorySnapshotStorage {
	fn save(&mut self, snapshot: Snapshot) -> Result<(), SnapshotError> {
		let mut snapshots = self.snapshots.lock().unwrap();
		snapshots.push(snapshot);

		// Keep snapshots sorted by sequence number
		snapshots.sort_by_key(|s| s.metadata.event_seq);

		Ok(())
	}

	fn load_latest(&self) -> Result<Snapshot, SnapshotError> {
		let snapshots = self.snapshots.lock().unwrap();
		snapshots.last().cloned().ok_or(SnapshotError::NotFound)
	}

	fn load_at_seq(&self, seq: SequenceNumber) -> Result<Snapshot, SnapshotError> {
		let snapshots = self.snapshots.lock().unwrap();

		// Find the latest snapshot with event_seq <= seq
		snapshots
			.iter()
			.rev()
			.find(|s| s.metadata.event_seq <= seq)
			.cloned()
			.ok_or(SnapshotError::NotFound)
	}

	fn list_snapshots(&self) -> Vec<SnapshotMetadata> {
		let snapshots = self.snapshots.lock().unwrap();
		snapshots.iter().map(|s| s.metadata.clone()).collect()
	}

	fn cleanup_before(&mut self, seq: SequenceNumber) -> Result<usize, SnapshotError> {
		let mut snapshots = self.snapshots.lock().unwrap();
		let original_len = snapshots.len();

		snapshots.retain(|s| s.metadata.event_seq >= seq);

		Ok(original_len - snapshots.len())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn create_test_snapshot(seq: SequenceNumber) -> Snapshot {
		Snapshot {
			metadata: SnapshotMetadata {
				created_at: 1000,
				event_seq: seq,
				size_bytes: 100,
				market: "BTC-USDT".to_string(),
			},
			state_data: vec![0u8; 100],
		}
	}

	#[test]
	fn test_save_and_load_latest() {
		let mut storage = MemorySnapshotStorage::new();

		storage.save(create_test_snapshot(100)).unwrap();
		storage.save(create_test_snapshot(200)).unwrap();

		let latest = storage.load_latest().unwrap();
		assert_eq!(latest.metadata.event_seq, 200);
	}

	#[test]
	fn test_load_at_seq() {
		let mut storage = MemorySnapshotStorage::new();

		storage.save(create_test_snapshot(100)).unwrap();
		storage.save(create_test_snapshot(200)).unwrap();
		storage.save(create_test_snapshot(300)).unwrap();

		let snap = storage.load_at_seq(250).unwrap();
		assert_eq!(snap.metadata.event_seq, 200);

		let snap = storage.load_at_seq(300).unwrap();
		assert_eq!(snap.metadata.event_seq, 300);

		let result = storage.load_at_seq(50);
		assert!(result.is_err());
	}

	#[test]
	fn test_cleanup() {
		let mut storage = MemorySnapshotStorage::new();

		storage.save(create_test_snapshot(100)).unwrap();
		storage.save(create_test_snapshot(200)).unwrap();
		storage.save(create_test_snapshot(300)).unwrap();

		let deleted = storage.cleanup_before(200).unwrap();
		assert_eq!(deleted, 1);

		let list = storage.list_snapshots();
		assert_eq!(list.len(), 2);
		assert_eq!(list[0].event_seq, 200);
	}
}
