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

//! Matching engine service entry point
//!
//! This binary wires up all components of the matching engine:
//! - Order Journal (idempotency)
//! - Ingress Queue (MPSC from RPC to matching loop)
//! - Matching Loop (single-threaded core)
//! - Event Buffer (SPSC from matching loop to event writer)
//! - Event Writer (persistence)
//! - Snapshotter (periodic state capture)
//! - RPC Server (multi-threaded ingress)

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tokio::signal;
use tonic::transport::Server;
use tracing::info;

use anvil_matching::{
	EventBuffer, EventWriter, EventWriterConfig, IngressQueue, MatchingEngine, MemoryEventStorage,
	MemoryOrderJournal, MemorySnapshotStorage, OrderJournal, SnapshotProvider, Snapshotter,
	SnapshotterConfig, config::MatchingConfig, engine::EngineConfig, server,
};

#[tokio::main]
async fn main() -> Result<()> {
	// Initialize logging first
	anvil_matching::logging::init_logging()?;

	// Load configuration
	let config = MatchingConfig::from_env().unwrap_or_else(|_| {
		info!(target: "server", "Using default configuration");
		MatchingConfig::default()
	});

	info!(target: "server", "Starting Anvil Matching Engine");
	info!(target: "server", "Market: {}", config.market);
	info!(target: "server", "Listening on: {}", config.bind_addr);
	info!(target: "server", "Ingress queue size: {}", config.ingress_queue_size);
	info!(target: "server", "Event buffer size: {}", config.event_buffer_size);

	// Phase 1: Initialize Order Journal
	info!(target: "server", "Initializing Order Journal...");
	let journal: Box<dyn OrderJournal> = Box::new(MemoryOrderJournal::new());
	let journal = Arc::new(Mutex::new(journal));

	// Phase 2: Create Ingress Queue (MPSC)
	info!(target: "server", "Creating ingress queue...");
	let ingress_queue = IngressQueue::new(config.ingress_queue_size);
	let (queue_sender, queue_receiver) = ingress_queue.split();

	// Phase 3: Create Event Buffer (SPSC)
	info!(target: "server", "Creating event buffer...");
	let event_buffer = EventBuffer::new(config.event_buffer_size);
	let (event_producer, event_consumer) = event_buffer.split();

	// Phase 4: Start Event Writer
	info!(target: "server", "Starting event writer...");
	let event_storage = Box::new(MemoryEventStorage::new());
	let event_writer_config = EventWriterConfig {
		batch_size: config.event_batch_size,
		batch_timeout_ms: config.event_batch_timeout_ms,
		verbose_logging: config.verbose_logging,
	};
	let _event_writer = EventWriter::start(
		event_consumer,
		event_storage,
		journal.clone(),
		event_writer_config,
	);

	// Phase 5: Start Matching Engine (single-threaded core)
	info!(target: "server", "Starting matching engine core...");
	let engine_config = EngineConfig {
		market: config.market.clone(),
		verbose_logging: config.verbose_logging,
	};
	let matching_engine = MatchingEngine::start(
		engine_config,
		queue_receiver,
		event_producer,
		journal.clone(),
	);

	// Phase 6: Start Snapshotter
	info!(target: "server", "Starting snapshotter...");
	let snapshot_storage = Box::new(MemorySnapshotStorage::new());
	let snapshotter_config = SnapshotterConfig {
		snapshot_interval_secs: config.snapshot_interval_secs,
		max_snapshots_to_keep: config.max_snapshots_to_keep,
	};

	// Create snapshot provider adapter
	let snapshot_provider = Arc::new(EngineSnapshotProvider {
		engine: matching_engine,
	});
	let snapshotter = Snapshotter::start(
		snapshot_storage,
		snapshotter_config,
		snapshot_provider.clone(),
	);

	// Phase 7: Start gRPC server
	info!(target: "server", "Starting gRPC server...");
	let matching_service = server::create_server(queue_sender, journal, config.market.clone());

	let server_future = Server::builder()
		.add_service(matching_service)
		.serve(config.bind_addr);

	// Wait for shutdown signal
	tokio::select! {
		result = server_future => {
			result.context("gRPC server error")?;
			info!(target: "server", "gRPC server stopped");
		}
		_ = signal::ctrl_c() => {
			info!(target: "server", "Shutting down...");
		}
	}

	// Graceful shutdown
	info!(target: "server", "Shutting down components...");
	snapshotter.shutdown();
	// matching_engine will be dropped, triggering shutdown
	// event_writer will be dropped, triggering shutdown

	info!(target: "server", "Shutdown complete");
	Ok(())
}

/// Adapter to provide snapshots from the matching engine
struct EngineSnapshotProvider {
	engine: MatchingEngine,
}

impl SnapshotProvider for EngineSnapshotProvider {
	fn create_snapshot(&self) -> Result<anvil_matching::snapshot::Snapshot, String> {
		self.engine.create_snapshot()
	}
}
