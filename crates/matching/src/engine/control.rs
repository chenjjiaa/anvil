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

use tokio::sync::oneshot;

use crate::event::MatchingEvent;
use crate::snapshot::Snapshot;

/// Control messages for the matching engine
///
/// These messages allow external components to interact with the matching engine
/// without blocking the matching loop or requiring shared mutable state.
///
/// The matching engine processes these messages in its main loop alongside
/// order commands, ensuring thread-safe access to engine state.
#[derive(Debug)]
pub enum EngineControlMessage {
	/// Request a snapshot of the current engine state
	///
	/// The matching loop will create a snapshot and send it back via the oneshot channel.
	/// This is non-blocking from the requester's perspective.
	CreateSnapshot {
		respond_to: oneshot::Sender<Result<Snapshot, String>>,
	},

	/// Request to restore engine state from a snapshot
	///
	/// Used during crash recovery to restore orderbook state.
	RestoreSnapshot {
		snapshot: Snapshot,
		respond_to: oneshot::Sender<Result<(), String>>,
	},

	/// Request to replay events to rebuild orderbook state
	///
	/// Used during crash recovery after restoring from snapshot.
	ReplayEvents {
		events: Vec<MatchingEvent>,
		respond_to: oneshot::Sender<Result<(), String>>,
	},

	/// Request the engine to shut down gracefully
	Shutdown,
}
