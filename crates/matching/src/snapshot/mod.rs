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

pub mod snapshotter;
mod storage;

use thiserror::Error;

use crate::event::SequenceNumber;
pub use snapshotter::{SnapshotProvider, Snapshotter, SnapshotterConfig};
pub use storage::{MemorySnapshotStorage, Snapshot, SnapshotMetadata, SnapshotStorage};

/// Error types for snapshot operations
#[derive(Debug, Error)]
pub enum SnapshotError {
	#[error("Failed to create snapshot: {0}")]
	CreationFailed(String),
	#[error("Failed to load snapshot: {0}")]
	LoadFailed(String),
	#[error("Snapshot corrupted: {0}")]
	Corrupted(String),
	#[error("No snapshot available")]
	NotFound,
}

/// Snapshot point representing a captured state
///
/// A snapshot point consists of:
/// - The event sequence number at which the snapshot was taken
/// - The complete orderbook state at that point
/// - Metadata about when and why the snapshot was created
///
/// During crash recovery:
/// 1. Load the latest snapshot
/// 2. Replay events from (snapshot.event_seq + 1) to current
/// 3. Rebuild orderbook to current state
pub struct SnapshotPoint {
	/// Event sequence number aligned with this snapshot
	pub event_seq: SequenceNumber,
	/// Serialized orderbook state
	pub state_data: Vec<u8>,
	/// Metadata about this snapshot
	pub metadata: SnapshotMetadata,
}
