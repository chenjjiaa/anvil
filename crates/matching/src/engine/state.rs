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

use crate::{OrderBook, event::SequenceNumber};

/// Matching engine state
///
/// This structure holds the complete state of the matching engine:
/// - Orderbook (all active orders)
/// - Sequence counter for events
///
/// The state is owned by the matching loop and can be snapshotted
/// for crash recovery.
pub struct MatchingEngineState {
	/// The orderbook for this market
	pub orderbook: OrderBook,
	/// Next event sequence number to assign
	pub next_sequence: SequenceNumber,
}

impl MatchingEngineState {
	pub fn new(market: String) -> Self {
		Self {
			orderbook: OrderBook::new(market),
			next_sequence: 1,
		}
	}

	/// Reset state to initial conditions
	pub fn reset(&mut self, market: String) {
		self.orderbook = OrderBook::new(market);
		self.next_sequence = 1;
	}
}
