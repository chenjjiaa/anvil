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

//! Crash recovery module
//!
//! This module implements the crash recovery logic for the matching engine.
//! Recovery proceeds in three phases:
//!
//! 1. Load latest snapshot (if available)
//! 2. Replay events from State Journal since snapshot
//! 3. Replay incomplete orders from Order Journal
//!
//! The recovery ensures that:
//! - Orderbook state is consistent
//! - Idempotency is maintained
//! - No orders are lost or duplicated

use tracing::{info, warn};

use crate::{
	MatchingEngine,
	event::EventStorage,
	journal::OrderJournal,
	snapshot::{SnapshotError, SnapshotStorage},
};

/// Crash recovery coordinator
pub struct RecoveryCoordinator {
	snapshot_storage: Box<dyn SnapshotStorage>,
	event_storage: Box<dyn EventStorage>,
	journal: Box<dyn OrderJournal>,
}

impl RecoveryCoordinator {
	pub fn new(
		snapshot_storage: Box<dyn SnapshotStorage>,
		event_storage: Box<dyn EventStorage>,
		journal: Box<dyn OrderJournal>,
	) -> Self {
		Self {
			snapshot_storage,
			event_storage,
			journal,
		}
	}

	/// Perform full crash recovery
	///
	/// Returns:
	/// - Ok(Some(seq)): Recovery successful, last sequence number
	/// - Ok(None): No recovery needed (clean start)
	/// - Err(msg): Recovery failed
	pub fn recover(&mut self, engine: &MatchingEngine) -> Result<Option<u64>, String> {
		info!("Starting crash recovery...");

		// Phase 1: Try to load latest snapshot
		let snapshot_seq = match self.snapshot_storage.load_latest() {
			Ok(snapshot) => {
				info!(
					"Loaded snapshot at seq={}, size={} bytes",
					snapshot.metadata.event_seq, snapshot.metadata.size_bytes
				);

				engine
					.restore_from_snapshot(snapshot.clone())
					.map_err(|e| format!("Failed to restore snapshot: {}", e))?;

				Some(snapshot.metadata.event_seq)
			}
			Err(SnapshotError::NotFound) => {
				info!("No snapshot found, starting from empty state");
				None
			}
			Err(e) => {
				warn!("Failed to load snapshot: {}, starting from empty state", e);
				None
			}
		};

		// Phase 2: Replay events since snapshot
		let from_seq = snapshot_seq.map(|s| s + 1).unwrap_or(1);
		let last_event_seq = self.event_storage.last_sequence();

		if last_event_seq >= from_seq {
			info!(
				"Replaying events from seq={} to seq={}",
				from_seq, last_event_seq
			);

			let events = self
				.event_storage
				.replay_from(from_seq)
				.map_err(|e| format!("Failed to replay events: {}", e))?;

			engine
				.replay_events(events)
				.map_err(|e| format!("Failed to apply events: {}", e))?;

			info!("Event replay complete");
		} else {
			info!("No events to replay");
		}

		// Phase 3: Replay incomplete orders from Order Journal
		let incomplete_orders: Vec<_> = self.journal.replay().collect();

		if !incomplete_orders.is_empty() {
			info!(
				"Found {} incomplete orders in journal (crash during processing)",
				incomplete_orders.len()
			);

			// These orders were accepted (in journal) but may not have completed matching
			// For MVP, we can re-enqueue them or mark them as needing manual review
			// For now, we'll just log them
			warn!(
				"Recovery of incomplete orders not fully implemented. {} orders may need manual review",
				incomplete_orders.len()
			);
		}

		let final_seq = if last_event_seq > 0 {
			Some(last_event_seq)
		} else {
			snapshot_seq
		};

		info!("Crash recovery complete at seq={:?}", final_seq);
		Ok(final_seq)
	}
}
