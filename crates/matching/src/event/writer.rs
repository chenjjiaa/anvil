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

use super::{EventBatch, EventStorage, MatchingEvent};
use crate::event::buffer::EventConsumer;

/// Configuration for the Event Writer
#[derive(Debug, Clone)]
pub struct EventWriterConfig {
	/// Maximum number of events to batch before committing
	pub batch_size: usize,
	/// Maximum time to wait before committing a partial batch (milliseconds)
	pub batch_timeout_ms: u64,
	/// Whether to log detailed event information
	pub verbose_logging: bool,
}

impl Default for EventWriterConfig {
	fn default() -> Self {
		Self {
			batch_size: 100,
			batch_timeout_ms: 100,
			verbose_logging: false,
		}
	}
}

/// Event Writer - consumes events from buffer and persists them
///
/// The Event Writer runs in a separate thread, consuming events produced
/// by the matching loop and writing them to durable storage. It batches
/// events to improve throughput and reduce I/O overhead.
///
/// Responsibilities:
/// - Consume events from Event Buffer (SPSC channel)
/// - Batch events for efficient commits
/// - Persist events to Event Storage
/// - Provide backpressure signals if storage falls behind
///
/// The Event Writer maintains the commit point: the last event sequence
/// that has been durably persisted. This is used during crash recovery
/// to determine which events need to be replayed.
pub struct EventWriter {
	/// Handle to the writer thread
	thread_handle: Option<JoinHandle<()>>,
	/// Shutdown signal
	shutdown: Arc<AtomicBool>,
}

impl EventWriter {
	/// Start the event writer with given configuration
	///
	/// The writer runs in a background thread, continuously consuming
	/// events from the buffer and writing them to storage.
	pub fn start(
		consumer: EventConsumer,
		mut storage: Box<dyn EventStorage>,
		config: EventWriterConfig,
	) -> Self {
		let shutdown = Arc::new(AtomicBool::new(false));
		let shutdown_clone = shutdown.clone();

		let thread_handle = thread::Builder::new()
			.name("event-writer".to_string())
			.spawn(move || {
				info!(target: "event_writer", "Event writer started");
				Self::run_writer_loop(&consumer, storage.as_mut(), &config, &shutdown_clone);
				info!(target: "event_writer", "Event writer stopped");
			})
			.expect("Failed to spawn event writer thread");

		Self {
			thread_handle: Some(thread_handle),
			shutdown,
		}
	}

	/// Main event writer loop
	fn run_writer_loop(
		consumer: &EventConsumer,
		storage: &mut dyn EventStorage,
		config: &EventWriterConfig,
		shutdown: &Arc<AtomicBool>,
	) {
		let batch_timeout = Duration::from_millis(config.batch_timeout_ms);
		let mut pending_events = Vec::with_capacity(config.batch_size);
		let mut last_commit_time = std::time::Instant::now();

		loop {
			if shutdown.load(Ordering::Relaxed) {
				// Flush remaining events before shutdown
				if !pending_events.is_empty() {
					let batch_size = pending_events.len();
					if let Err(e) = Self::commit_batch(storage, &pending_events, config) {
						error!(
							target: "event_writer",
							batch_size = batch_size,
							error = %e,
							"Failed to commit final batch during shutdown"
						);
					} else {
						info!(
							target: "event_writer",
							batch_size = batch_size,
							"Flushed final batch during shutdown"
						);
					}
				}
				break;
			}

			// Try to drain events from buffer
			let drained_count = {
				let drained = consumer.drain(config.batch_size - pending_events.len());
				let count = drained.len();
				pending_events.extend(drained);
				count
			};

			// Check buffer pressure for observability
			if drained_count > config.batch_size / 2 && config.verbose_logging {
				debug!(
					target: "event_writer",
					drained = drained_count,
					pending = pending_events.len(),
					"High event buffer pressure"
				);
			}

			// Commit if batch is full or timeout elapsed
			let should_commit = pending_events.len() >= config.batch_size
				|| (!pending_events.is_empty() && last_commit_time.elapsed() >= batch_timeout);

			if should_commit {
				let batch_size = pending_events.len();
				let start = std::time::Instant::now();

				match Self::commit_batch(storage, &pending_events, config) {
					Ok(last_seq) => {
						let duration = start.elapsed();
						let first_seq = pending_events.first().map(|e| e.sequence()).unwrap_or(0);

						if config.verbose_logging {
							debug!(
								target: "event_writer",
								batch_size = batch_size,
								seq_range = format!("{}-{}", first_seq, last_seq),
								latency_ms = duration.as_millis(),
								"Batch committed"
							);
						} else if batch_size > 50 {
							// Log large batches even if verbose logging is off
							info!(
								target: "event_writer",
								batch_size = batch_size,
								seq_range = format!("{}-{}", first_seq, last_seq),
								latency_ms = duration.as_millis(),
								"Large batch committed"
							);
						}

						pending_events.clear();
						last_commit_time = std::time::Instant::now();
					}
					Err(e) => {
						error!(
							target: "event_writer",
							batch_size = batch_size,
							error = %e,
							"Failed to commit event batch"
						);
						// In production, this should trigger alerting
						// For now, we'll retry on next iteration
						thread::sleep(Duration::from_millis(100));
					}
				}
			} else if pending_events.is_empty() {
				// No events to process, wait a bit
				thread::sleep(Duration::from_millis(10));
			}
		}
	}

	/// Commit a batch of events to storage
	fn commit_batch(
		storage: &mut dyn EventStorage,
		events: &[MatchingEvent],
		_config: &EventWriterConfig,
	) -> Result<u64, String> {
		if events.is_empty() {
			return Ok(storage.last_sequence());
		}

		let batch = EventBatch::new(events.to_vec());
		storage
			.append_batch(batch)
			.map_err(|e| format!("Storage error: {}", e))
	}

	/// Shutdown the event writer gracefully
	pub fn shutdown(mut self) {
		info!(target: "event_writer", "Shutting down event writer");
		self.shutdown.store(true, Ordering::Relaxed);

		if let Some(handle) = self.thread_handle.take()
			&& let Err(e) = handle.join()
		{
			warn!(target: "event_writer", error = ?e, "Event writer thread panicked");
		}
	}
}

impl Drop for EventWriter {
	fn drop(&mut self) {
		self.shutdown.store(true, Ordering::Relaxed);
		if let Some(handle) = self.thread_handle.take()
			&& let Err(e) = handle.join()
		{
			let _ = Err::<(), _>(e);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::event::{EventBuffer, MemoryEventStorage};
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
	fn test_event_writer_basic() {
		let buffer = EventBuffer::new(100);
		let (producer, consumer) = buffer.split();
		let storage = Box::new(MemoryEventStorage::new());

		let config = EventWriterConfig {
			batch_size: 5,
			batch_timeout_ms: 50,
			verbose_logging: false,
		};

		let writer = EventWriter::start(consumer, storage, config);

		// Push some events
		for i in 0..10 {
			producer.push(create_test_event(i)).unwrap();
		}

		// Give writer time to process
		thread::sleep(Duration::from_millis(200));

		writer.shutdown();
	}
}
