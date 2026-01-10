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

mod buffer;
mod storage;
mod writer;

use anvil_sdk::types::{Side, Trade};
use serde::{Deserialize, Serialize};

pub use buffer::{EventBuffer, EventConsumer, EventProducer};
pub use storage::{EventStorage, MemoryEventStorage};
pub use writer::{EventWriter, EventWriterConfig};

/// Sequence number for event ordering
///
/// Events are assigned monotonically increasing sequence numbers
/// to ensure deterministic replay ordering during crash recovery.
pub type SequenceNumber = u64;

/// Events produced by the matching engine
///
/// These events represent the single source of truth for all state changes
/// in the matching engine. The orderbook state can be fully reconstructed
/// by replaying events from the beginning.
///
/// Design principles:
/// - Events are immutable once emitted
/// - Each event has a unique, monotonically increasing sequence number
/// - Events are sufficient to rebuild complete orderbook state
/// - Events do not contain redundant computed state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchingEvent {
	/// Order was accepted and added to the order book
	OrderAccepted {
		seq: SequenceNumber,
		order_id: String,
		market: String,
		side: Side,
		price: u64,
		size: u64,
		timestamp: u64,
	},

	/// Order was rejected during admission
	OrderRejected {
		seq: SequenceNumber,
		order_id: String,
		market: String,
		reason: String,
		timestamp: u64,
	},

	/// Order was completely filled
	OrderFilled {
		seq: SequenceNumber,
		order_id: String,
		market: String,
		filled_size: u64,
		timestamp: u64,
	},

	/// Order was partially filled
	OrderPartiallyFilled {
		seq: SequenceNumber,
		order_id: String,
		market: String,
		filled_size: u64,
		remaining_size: u64,
		timestamp: u64,
	},

	/// Order was cancelled and removed from the book
	OrderCancelled {
		seq: SequenceNumber,
		order_id: String,
		market: String,
		remaining_size: u64,
		timestamp: u64,
	},

	/// A trade was executed between maker and taker
	TradeExecuted {
		seq: SequenceNumber,
		trade: Trade,
		timestamp: u64,
	},
}

impl MatchingEvent {
	/// Get the sequence number of this event
	pub fn sequence(&self) -> SequenceNumber {
		match self {
			MatchingEvent::OrderAccepted { seq, .. } => *seq,
			MatchingEvent::OrderRejected { seq, .. } => *seq,
			MatchingEvent::OrderFilled { seq, .. } => *seq,
			MatchingEvent::OrderPartiallyFilled { seq, .. } => *seq,
			MatchingEvent::OrderCancelled { seq, .. } => *seq,
			MatchingEvent::TradeExecuted { seq, .. } => *seq,
		}
	}

	/// Get the order_id associated with this event (if applicable)
	pub fn order_id(&self) -> Option<&str> {
		match self {
			MatchingEvent::OrderAccepted { order_id, .. } => Some(order_id),
			MatchingEvent::OrderRejected { order_id, .. } => Some(order_id),
			MatchingEvent::OrderFilled { order_id, .. } => Some(order_id),
			MatchingEvent::OrderPartiallyFilled { order_id, .. } => Some(order_id),
			MatchingEvent::OrderCancelled { order_id, .. } => Some(order_id),
			MatchingEvent::TradeExecuted { .. } => None,
		}
	}

	/// Get the market associated with this event
	pub fn market(&self) -> &str {
		match self {
			MatchingEvent::OrderAccepted { market, .. } => market,
			MatchingEvent::OrderRejected { market, .. } => market,
			MatchingEvent::OrderFilled { market, .. } => market,
			MatchingEvent::OrderPartiallyFilled { market, .. } => market,
			MatchingEvent::OrderCancelled { market, .. } => market,
			MatchingEvent::TradeExecuted { trade, .. } => &trade.market,
		}
	}

	/// Check if this event marks order completion
	///
	/// Returns true for events that indicate an order has completed its lifecycle:
	/// - OrderFilled (fully matched)
	/// - OrderCancelled (removed from book)
	/// - OrderRejected (never entered book)
	pub fn is_order_complete(&self) -> bool {
		matches!(
			self,
			MatchingEvent::OrderFilled { .. }
				| MatchingEvent::OrderCancelled { .. }
				| MatchingEvent::OrderRejected { .. }
		)
	}
}

/// Batch of events for efficient processing
///
/// Events are typically processed in batches to reduce overhead
/// and improve throughput.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBatch {
	pub events: Vec<MatchingEvent>,
	pub batch_timestamp: u64,
}

impl EventBatch {
	pub fn new(events: Vec<MatchingEvent>) -> Self {
		let batch_timestamp = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		Self {
			events,
			batch_timestamp,
		}
	}

	pub fn is_empty(&self) -> bool {
		self.events.is_empty()
	}

	pub fn len(&self) -> usize {
		self.events.len()
	}
}
