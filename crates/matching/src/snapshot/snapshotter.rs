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

use std::{
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	thread::{self, JoinHandle},
	time::Duration,
};

use tracing::{debug, error, info, warn};

use super::{Snapshot, SnapshotStorage};

/// Configuration for the Snapshotter
#[derive(Debug, Clone)]
pub struct SnapshotterConfig {
	/// Interval between snapshots (seconds)
	pub snapshot_interval_secs: u64,
	/// Keep at most this many recent snapshots
	pub max_snapshots_to_keep: usize,
}

impl Default for SnapshotterConfig {
	fn default() -> Self {
		Self {
			snapshot_interval_secs: 300, // 5 minutes
			max_snapshots_to_keep: 10,
		}
	}
}

/// Snapshotter - periodically captures orderbook state
///
/// The Snapshotter runs asynchronously in the background, periodically
/// capturing the matching engine's orderbook state and persisting it.
/// This accelerates crash recovery by providing a starting point,
/// reducing the number of events that need to be replayed.
///
/// Design principles:
/// - Non-blocking: does not interfere with matching loop
/// - Eventually consistent: snapshots may lag slightly behind current state
/// - Safe points: only snapshots at consistent state boundaries
/// - Cleanup: removes old snapshots to bound storage usage
///
/// The snapshotter does NOT:
/// - Block the matching loop
/// - Guarantee real-time snapshot freshness
/// - Participate in ACK decisions
/// - Handle idempotency
pub struct Snapshotter {
	thread_handle: Option<JoinHandle<()>>,
	shutdown: Arc<AtomicBool>,
}

impl Snapshotter {
	/// Start the snapshotter
	///
	/// In the MVP implementation, the snapshotter periodically requests
	/// snapshots. In production, this would coordinate with the matching
	/// loop to capture state at safe points.
	pub fn start(
		mut storage: Box<dyn SnapshotStorage>,
		config: SnapshotterConfig,
		snapshot_provider: Arc<dyn SnapshotProvider>,
	) -> Self {
		let shutdown = Arc::new(AtomicBool::new(false));
		let shutdown_clone = shutdown.clone();

		let thread_handle = thread::Builder::new()
			.name("snapshotter".to_string())
			.spawn(move || {
				info!(target: "snapshotter", "Snapshotter started");
				Self::run_snapshot_loop(
					storage.as_mut(),
					&config,
					snapshot_provider.as_ref(),
					&shutdown_clone,
				);
				info!(target: "snapshotter", "Snapshotter stopped");
			})
			.expect("Failed to spawn snapshotter thread");

		Self {
			thread_handle: Some(thread_handle),
			shutdown,
		}
	}

	fn run_snapshot_loop(
		storage: &mut dyn SnapshotStorage,
		config: &SnapshotterConfig,
		provider: &dyn SnapshotProvider,
		shutdown: &Arc<AtomicBool>,
	) {
		let interval = Duration::from_secs(config.snapshot_interval_secs);

		loop {
			if shutdown.load(Ordering::Relaxed) {
				break;
			}

			thread::sleep(interval);

			if shutdown.load(Ordering::Relaxed) {
				break;
			}

			// Request a snapshot from the matching engine
			let start = std::time::Instant::now();
			match provider.create_snapshot() {
				Ok(snapshot) => {
					let snapshot_duration = start.elapsed();
					let seq = snapshot.metadata.event_seq;
					let size = snapshot.metadata.size_bytes;

					match storage.save(snapshot.clone()) {
						Ok(_) => {
							let total_duration = start.elapsed();
							info!(
								target: "snapshotter",
								seq = seq,
								size_bytes = size,
								snapshot_ms = snapshot_duration.as_millis(),
								save_ms = (total_duration - snapshot_duration).as_millis(),
								total_ms = total_duration.as_millis(),
								"Snapshot created and saved"
							);
						}
						Err(e) => {
							error!(
								target: "snapshotter",
								seq = seq,
								error = %e,
								"Failed to save snapshot"
							);
							continue;
						}
					}

					// Cleanup old snapshots
					let snapshots = storage.list_snapshots();
					let total_snapshots = snapshots.len();

					if total_snapshots > config.max_snapshots_to_keep {
						let cutoff_seq =
							snapshots[snapshots.len() - config.max_snapshots_to_keep].event_seq;
						let to_delete = total_snapshots - config.max_snapshots_to_keep;

						match storage.cleanup_before(cutoff_seq) {
							Ok(_) => {
								debug!(
									target: "snapshotter",
									deleted_count = to_delete,
									retained_count = config.max_snapshots_to_keep,
									cutoff_seq = cutoff_seq,
									"Old snapshots cleaned up"
								);
							}
							Err(e) => {
								error!(
									target: "snapshotter",
									cutoff_seq = cutoff_seq,
									error = %e,
									"Failed to cleanup old snapshots"
								);
							}
						}
					}
				}
				Err(e) => {
					error!(target: "snapshotter", error = %e, "Failed to create snapshot");
				}
			}
		}
	}

	pub fn shutdown(mut self) {
		info!(target: "snapshotter", "Shutting down snapshotter");
		self.shutdown.store(true, Ordering::Relaxed);

		if let Some(handle) = self.thread_handle.take()
			&& let Err(e) = handle.join()
		{
			warn!(target: "snapshotter", error = ?e, "Snapshotter thread panicked");
		}
	}
}

impl Drop for Snapshotter {
	fn drop(&mut self) {
		self.shutdown.store(true, Ordering::Relaxed);
		if let Some(handle) = self.thread_handle.take() {
			let _ = handle.join();
		}
	}
}

/// Trait for providing snapshots from the matching engine
///
/// This abstraction allows the snapshotter to request snapshots
/// without knowing the details of the matching engine implementation.
pub trait SnapshotProvider: Send + Sync {
	/// Create a snapshot of current state
	fn create_snapshot(&self) -> Result<Snapshot, String>;
}

/// Mock snapshot provider for testing
#[cfg(test)]
pub struct MockSnapshotProvider {
	seq: Arc<std::sync::Mutex<u64>>,
}

#[cfg(test)]
impl MockSnapshotProvider {
	pub fn new() -> Self {
		Self {
			seq: Arc::new(std::sync::Mutex::new(0)),
		}
	}
}

#[cfg(test)]
impl Default for MockSnapshotProvider {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
impl SnapshotProvider for MockSnapshotProvider {
	fn create_snapshot(&self) -> Result<Snapshot, String> {
		let mut seq = self.seq.lock().unwrap();
		*seq += 1;

		use super::SnapshotMetadata;

		Ok(Snapshot {
			metadata: SnapshotMetadata {
				created_at: std::time::SystemTime::now()
					.duration_since(std::time::UNIX_EPOCH)
					.unwrap()
					.as_secs(),
				event_seq: *seq,
				size_bytes: 100,
				market: "BTC-USDT".to_string(),
			},
			state_data: vec![0u8; 100],
		})
	}
}
