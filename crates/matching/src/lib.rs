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

//! Anvil Matching Engine
//!
//! This crate provides a high-performance, deterministic matching engine
//! for limit order books. It maintains in-memory order books, applies
//! price-time priority, and produces replayable matching results.
//!
//! Architecture:
//! - Single-threaded matching core for deterministic behavior
//! - Event sourcing for crash recovery
//! - Order Journal for idempotency
//! - MPSC ingress queue for multi-threaded RPC ingress
//! - SPSC event buffer for non-blocking event persistence

pub mod client;
pub mod config;
pub mod engine;
pub mod event;
pub mod journal;
pub mod matcher;
pub mod orderbook;
pub mod queue;
pub mod recovery;
pub mod server;
pub mod snapshot;
pub mod types;

pub use engine::{EngineConfig, EngineError, MatchingEngine, MatchingEngineState};
pub use event::{
	EventBuffer, EventConsumer, EventProducer, EventStorage, EventWriter, EventWriterConfig,
	MatchingEvent, MemoryEventStorage,
};
pub use journal::{MemoryOrderJournal, OrderJournal};
#[allow(deprecated)]
pub use matcher::Matcher;
pub use orderbook::OrderBook;
pub use queue::{IngressQueue, QueueReceiver, QueueSender};
pub use recovery::RecoveryCoordinator;
pub use snapshot::{MemorySnapshotStorage, SnapshotProvider, Snapshotter, SnapshotterConfig};
pub use types::*;
